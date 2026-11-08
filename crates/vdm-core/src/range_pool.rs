//! Work-stealing range pool for segmented HTTP downloads (XDM acceleration model).
//!
//! The file is modeled as a set of remaining byte intervals. A worker calls
//! [`RangePool::acquire`] to lease a chunk taken from the **largest** remaining
//! interval, so when one connection finishes early it splits the biggest piece
//! left and no connection sits idle. Released tails (partial leases after an
//! error) are merged back for resume.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Interval {
    pub start: u64,
    pub end: u64, // inclusive
}

struct Inner {
    intervals: Vec<Interval>, // sorted by start, disjoint
}

pub struct RangePool {
    inner: Mutex<Inner>,
}

impl RangePool {
    /// New pool for a file of `total` bytes (0 = unknown -> single open-ended lease).
    pub fn new(total: u64) -> Self {
        let intervals = if total > 0 {
            vec![Interval { start: 0, end: total - 1 }]
        } else {
            Vec::new()
        };
        RangePool {
            inner: Mutex::new(Inner { intervals }),
        }
    }

    pub fn total(&self) -> u64 {
        0 // tracked externally; pool only knows remaining
    }

    /// Remaining bytes to download.
    pub fn remaining(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.intervals.iter().map(|i| i.end - i.start + 1).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().intervals.is_empty()
    }

    /// Lease up to `max_len` bytes, taken from the largest interval's high end.
    /// Returns `(start, end)` inclusive.
    pub fn acquire(&self, max_len: u64) -> Option<(u64, u64)> {
        let mut inner = self.inner.lock().unwrap();
        if inner.intervals.is_empty() {
            return None;
        }
        let idx = inner
            .intervals
            .iter()
            .enumerate()
            .max_by_key(|(_, iv)| iv.end - iv.start)
            .map(|(i, _)| i)
            .unwrap();
        let iv = inner.intervals[idx].clone();
        let len = iv.end - iv.start + 1;
        if len <= max_len {
            inner.intervals.remove(idx);
            Some((iv.start, iv.end))
        } else {
            let split = iv.end - max_len + 1;
            inner.intervals[idx].end = split - 1;
            Some((split, iv.end))
        }
    }

    /// Return an unconsumed tail `(start, end)` back to the pool (merge neighbors).
    pub fn release(&self, start: u64, end: u64) {
        if end < start {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.intervals.push(Interval { start, end });
        inner.intervals.sort_by_key(|i| i.start);
        // merge overlapping/touching
        let mut merged: Vec<Interval> = Vec::with_capacity(inner.intervals.len());
        for iv in inner.intervals.drain(..) {
            match merged.last_mut() {
                Some(last) if iv.start <= last.end.saturating_add(1) => {
                    last.end = last.end.max(iv.end);
                }
                _ => merged.push(iv),
            }
        }
        inner.intervals = merged;
    }

    /// Snapshot of remaining intervals (for the `.vdmctl` resume file).
    pub fn intervals(&self) -> Vec<Interval> {
        self.inner.lock().unwrap().intervals.clone()
    }

    /// Restore from a saved snapshot (resume).
    pub fn from_intervals(total: u64, remaining: Vec<Interval>) -> Self {
        let pool = RangePool::new(0);
        {
            let mut inner = pool.inner.lock().unwrap();
            let _ = total;
            inner.intervals = remaining;
            inner.intervals.sort_by_key(|i| i.start);
        }
        pool
    }

    /// How much of the file is done, given the total size.
    pub fn done(&self, total: u64) -> u64 {
        total.saturating_sub(self.remaining())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_splits_largest() {
        let pool = RangePool::new(1_000);
        let a = pool.acquire(100).unwrap();
        assert_eq!(a, (900, 999)); // from high end of largest
        let b = pool.acquire(100).unwrap();
        // largest now [0,899] len 900 -> take 800..899
        assert_eq!(b, (800, 899));
        assert_eq!(pool.remaining(), 800);
    }

    #[test]
    fn exhaustion_and_empty() {
        let pool = RangePool::new(50);
        assert_eq!(pool.acquire(50), Some((0, 49)));
        assert!(pool.is_empty());
        assert_eq!(pool.acquire(1), None);
    }

    #[test]
    fn release_merges_back() {
        let pool = RangePool::new(100);
        let a = pool.acquire(40).unwrap(); // 60..99
        let b = pool.acquire(60).unwrap(); // 0..59
        assert_eq!((a, b), ((60, 99), (0, 59)));
        pool.release(70, 99); // give back tail
        assert_eq!(pool.remaining(), 30);
        let c = pool.acquire(10).unwrap(); // leases from the HIGH end: (90,99)
        assert_eq!(c, (90, 99));
        pool.release(0, 69); // merges with [70..89] -> [0..89]
        assert_eq!(pool.intervals(), vec![Interval { start: 0, end: 89 }]);
    }

    #[test]
    fn zero_total_is_empty() {
        let pool = RangePool::new(0);
        assert!(pool.is_empty());
    }

    #[test]
    fn snapshot_roundtrip() {
        let pool = RangePool::new(10_000);
        let _ = pool.acquire(1_000);
        let _ = pool.acquire(1_000);
        let ivs = pool.intervals();
        assert_eq!(pool.remaining(), 8_000);
        let restored = RangePool::from_intervals(10_000, ivs);
        assert_eq!(restored.remaining(), 8_000);
        assert_eq!(restored.done(10_000), 2_000);
    }

    #[test]
    fn many_workers_drain_exactly() {
        let pool = RangePool::new(7 * 64 * 1024 + 13); // non-multiple of lease
        let mut leased = 0u64;
        while let Some((s, e)) = pool.acquire(64 * 1024) {
            leased += e - s + 1;
        }
        assert_eq!(leased, 7 * 64 * 1024 + 13);
        assert!(pool.is_empty());
    }
}
