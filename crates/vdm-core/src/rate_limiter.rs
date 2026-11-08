// SPDX-License-Identifier: GPL-3.0-or-later
//! Token-bucket rate limiter shared across workers (global) and per task.
//! Rate semantics: `rate_bps` bytes per second sustained, with a seed burst;
//! partial grants are the contract (callers write exactly what was granted).
//! 0 disables limiting.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub struct TokenBucket {
    rate_bps: AtomicU64,
    state: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last: Instant,
}

const MIN_BURST: f64 = 64.0 * 1024.0;

impl TokenBucket {
    pub fn new(rate_bps: u64) -> Self {
        TokenBucket {
            rate_bps: AtomicU64::new(rate_bps),
            state: Mutex::new(BucketState { tokens: f64::MAX, last: Instant::now() }),
        }
    }

    pub fn set_rate(&self, bps: u64) {
        self.rate_bps.store(bps, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate_bps.load(Ordering::Relaxed)
    }

    pub fn is_limited(&self) -> bool {
        self.rate() > 0
    }

    /// Refills the bucket and grants up to `want` bytes. Returns 0 when the
    /// bucket is empty; callers sleep briefly and retry (polling keeps the
    /// implementation simple and the wake-up cadence bounded at 20 ms).
    /// Partial grants are the contract: callers write exactly what was
    /// granted and loop for the rest.
    pub fn try_grant(&self, want: u64) -> u64 {
        let rate = self.rate();
        if rate == 0 {
            return want;
        }
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(state.last).as_secs_f64();
        state.last = now;
        if state.tokens >= f64::MAX {
            // First use: seed with a starting burst instead of an unbounded
            // bucket (order matters: seed before the refill cap).
            state.tokens = rate as f64 * 0.2 + MIN_BURST;
        } else if elapsed > 0.0 {
            state.tokens = (state.tokens + elapsed * rate as f64).min(rate as f64 * 2.0 + MIN_BURST);
        }
        let grant = (want as f64).min(state.tokens);
        if grant >= 1.0 {
            state.tokens -= grant;
            grant as u64
        } else {
            0
        }
    }

    /// Grants at least one byte, sleeping in short slices until the bucket
    /// can satisfy the request.
    pub async fn acquire(&self, want: u64) -> u64 {
        let want = want.max(1);
        loop {
            let granted = self.try_grant(want);
            if granted > 0 {
                return granted;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_grants_everything() {
        let bucket = TokenBucket::new(0);
        assert_eq!(bucket.try_grant(10_000_000), 10_000_000);
    }

    #[test]
    fn first_grant_is_the_seed_burst() {
        let bucket = TokenBucket::new(1_000_000);
        let granted = bucket.try_grant(10_000_000);
        assert!(granted >= MIN_BURST as u64 + 200_000, "seed burst expected, got {}", granted);
    }

    #[test]
    fn bucket_refills_over_time() {
        let bucket = TokenBucket::new(1_000_000); // 1 MB/s
        // Drain the seed burst.
        loop {
            let g = bucket.try_grant(1 << 20);
            if g == 0 {
                break;
            }
            if g < (1 << 20) {
                break;
            }
        }
        assert_eq!(bucket.try_grant(1 << 20), 0, "drained bucket must refuse");
        std::thread::sleep(std::time::Duration::from_millis(120));
        let after = bucket.try_grant(1 << 20);
        assert!(
            after >= 100_000 && after <= 1_000_000,
            "expected ~120 KB refill, got {}",
            after
        );
    }

    #[tokio::test]
    async fn acquire_paces_against_the_rate() {
        // acquire may return partial grants; the caller loops. Verify the
        // aggregate grant rate respects the bucket rate (50 KB/s for 200 KB).
        let bucket = TokenBucket::new(50_000);
        let start = std::time::Instant::now();
        let mut granted_total: u64 = 0;
        while granted_total < 200_000 {
            granted_total += bucket.acquire(200_000 - granted_total).await;
        }
        let elapsed = start.elapsed();
        assert_eq!(granted_total, 200_000);
        assert!(elapsed >= std::time::Duration::from_millis(1500), "paced grant took {:?}", elapsed);
    }

    #[tokio::test]
    async fn unlimited_acquire_returns_immediately() {
        let bucket = TokenBucket::new(0);
        let start = std::time::Instant::now();
        let granted = bucket.acquire(10_000_000).await;
        assert_eq!(granted, 10_000_000);
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }
}
