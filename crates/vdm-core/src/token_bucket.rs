//! Token-bucket rate limiter.
//!
//! Two-level design (XDM-style): every task owns a per-task bucket and all tasks
//! additionally share one global bucket owned by the engine. A worker that wants
//! to read `n` bytes must obtain `n` tokens from both buckets, which yields
//! per-download caps and a global cap that leaves bandwidth free for other apps.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct TokenBucket {
    /// Bytes per second; 0 = unlimited.
    rate: AtomicU64,
    burst: u64,
    state: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(rate_bps: u64, burst: u64) -> Self {
        TokenBucket {
            rate: AtomicU64::new(rate_bps),
            burst: burst.max(1),
            state: Mutex::new(BucketState {
                tokens: burst as f64,
                last: Instant::now(),
            }),
        }
    }

    pub fn unlimited() -> Self {
        Self::new(0, 1)
    }

    pub fn set_rate(&self, rate_bps: u64) {
        self.rate.store(rate_bps, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Take `n` tokens if available without waiting.
    pub fn try_take(&self, n: u64) -> bool {
        if n == 0 {
            return true;
        }
        let rate = self.rate();
        if rate == 0 {
            return true;
        }
        let mut st = self.state.lock().unwrap();
        Self::refill(&mut st, rate, self.burst);
        if st.tokens >= n as f64 {
            st.tokens -= n as f64;
            true
        } else {
            false
        }
    }

    /// Block until `n` tokens can be taken (paced read loop).
    pub fn wait_for(&self, n: u64) {
        if n == 0 {
            return;
        }
        let mut remaining = n;
        while remaining > 0 {
            let rate = self.rate();
            if rate == 0 {
                return;
            }
            // Never sleep longer than needed for the deficit at current rate.
            let want = remaining.min(self.burst);
            let deficit = {
                let mut st = self.state.lock().unwrap();
                Self::refill(&mut st, rate, self.burst);
                if st.tokens >= want as f64 {
                    st.tokens -= want as f64;
                    remaining -= want;
                    continue;
                } else {
                    want as f64 - st.tokens
                }
            };
            let sleep = (deficit / rate as f64 * 1000.0).ceil() as u64;
            std::thread::sleep(Duration::from_millis(sleep.clamp(1, 250)));
        }
    }

    fn refill(st: &mut BucketState, rate: u64, burst: u64) {
        let now = Instant::now();
        let elapsed = now.duration_since(st.last).as_secs_f64();
        if elapsed > 0.0 {
            st.tokens = (st.tokens + elapsed * rate as f64).min(burst as f64);
            st.last = now;
        }
    }
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn unlimited_is_instant() {
        let b = TokenBucket::unlimited();
        let t0 = Instant::now();
        b.wait_for(10_000_000);
        assert!(t0.elapsed().as_millis() < 50);
        assert!(b.try_take(999_999));
    }

    #[test]
    fn rate_limit_paces() {
        // 10_000 B/s with 2000 burst: taking 6000 total must cost >= ~0.4s.
        let b = TokenBucket::new(10_000, 2_000);
        let t0 = Instant::now();
        b.wait_for(6_000);
        let ms = t0.elapsed().as_millis();
        assert!(ms >= 300, "expected pacing, took {}ms", ms);
        assert!(ms < 1_500, "pacing too aggressive: {}ms", ms);
    }

    #[test]
    fn try_take_respects_burst() {
        let b = TokenBucket::new(1_000, 100);
        assert!(b.try_take(100));
        assert!(!b.try_take(1));
    }

    #[test]
    fn set_rate_live() {
        let b = TokenBucket::new(1_000, 10);
        assert!(!b.try_take(50));
        b.set_rate(0);
        assert!(b.try_take(1_000_000));
    }
}
