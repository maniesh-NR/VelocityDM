// SPDX-License-Identifier: GPL-3.0-or-later
//! AIMD (additive increase / multiplicative decrease) tuning of the
//! connection count per download, in the spirit of TFRC (RFC 3448) and
//! Chiu & Jain's analysis of AIMD convergence:
//! - any error in the sample window halves the target (multiplicative
//!   decrease), floored at the configured minimum;
//! - stable-or-improving throughput grows the target by one connection
//!   (additive increase), capped at the configured maximum;
//! - throughput degradation with more connections holds the target
//!   (cross-traffic contention detection).

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Grow,
    Hold,
    Shrink,
}

pub struct ConnTuner {
    min: u32,
    max: u32,
    target: AtomicU32,
}

impl ConnTuner {
    pub fn new(min: u32, max: u32) -> Self {
        let min = min.max(1);
        let max = max.max(min);
        ConnTuner { min, max, target: AtomicU32::new(max) }
    }

    pub fn target(&self) -> u32 {
        self.target.load(Ordering::Relaxed)
    }

    pub fn bounds(&self) -> (u32, u32) {
        (self.min, self.max)
    }

    /// Feeds one sample window (typically 1 s). `speed_bps` is the window's
    /// throughput, `last_speed` the previous window's throughput (updated in
    /// place), `errors` the number of worker errors observed.
    pub fn sample(&self, speed_bps: f64, last_speed: &mut f64, errors: u32) -> Direction {
        if errors > 0 {
            let new = (self.target() / 2).max(self.min);
            self.target.store(new, Ordering::Relaxed);
            *last_speed = speed_bps;
            return Direction::Shrink;
        }
        let growing_or_stable = *last_speed <= 0.0 || speed_bps >= *last_speed * 0.95;
        *last_speed = speed_bps;
        if growing_or_stable {
            let new = (self.target() + 1).min(self.max);
            let grew = new != self.target();
            self.target.store(new, Ordering::Relaxed);
            if grew {
                Direction::Grow
            } else {
                Direction::Hold
            }
        } else {
            Direction::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_max_and_grows_to_cap() {
        let tuner = ConnTuner::new(2, 4);
        assert_eq!(tuner.target(), 4);
        let mut last = 0.0;
        for _ in 0..10 {
            tuner.sample(1000.0, &mut last, 0);
        }
        assert_eq!(tuner.target(), 4, "additive increase is capped at max");
    }

    #[test]
    fn errors_halve_until_min() {
        let tuner = ConnTuner::new(2, 8);
        let mut last = 0.0;
        tuner.sample(1000.0, &mut last, 1);
        assert_eq!(tuner.target(), 4);
        tuner.sample(1000.0, &mut last, 1);
        assert_eq!(tuner.target(), 2);
        tuner.sample(1000.0, &mut last, 1);
        assert_eq!(tuner.target(), 2, "halving is floored at min");
    }

    #[test]
    fn degradation_holds_target() {
        let tuner = ConnTuner::new(1, 8);
        let mut last = 1000.0;
        let d = tuner.sample(500.0, &mut last, 0);
        assert_eq!(d, Direction::Hold);
        assert_eq!(tuner.target(), 8);
    }

    #[test]
    fn stability_grows_target() {
        let tuner = ConnTuner::new(1, 8);
        tuner.hack_set(3);
        let mut last = 1000.0;
        let d = tuner.sample(1100.0, &mut last, 0);
        assert_eq!(d, Direction::Grow);
        assert_eq!(tuner.target(), 4);
    }
}

impl ConnTuner {
    #[cfg(test)]
    fn hack_set(&self, v: u32) {
        self.target.store(v, Ordering::Relaxed);
    }
}
