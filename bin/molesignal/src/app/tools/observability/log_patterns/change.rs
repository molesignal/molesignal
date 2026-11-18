// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Whether a pattern is new, surging, dropping or steady against the baseline stretch.

use serde::Serialize;

/// From this many times as many logs as in the baseline, a pattern surges; from this many times
/// fewer, it drops.
const SURGE_RATIO: f64 = 2.0;

/// A move of fewer logs than this is noise, whatever its ratio.
const MIN_MOVE: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Change {
    New,
    Surging,
    Dropping,
    Steady,
}

impl Change {
    /// Where the change puts a pattern in the list: new and surging first.
    pub(super) fn rank(self) -> u8 {
        match self {
            Self::New => 0,
            Self::Surging => 1,
            Self::Steady | Self::Dropping => 2,
        }
    }
}

/// How a pattern moved from the baseline to the window, both counts estimated over the whole of
/// their stretch, which are equally long. The ratio is the window's count over the baseline's,
/// `None` when the baseline had none.
pub(super) fn classify(window: f64, baseline: f64) -> (Change, Option<f64>) {
    if baseline <= 0.0 {
        let change = if window > 0.0 {
            Change::New
        } else {
            Change::Steady
        };

        return (change, None);
    }

    // One log either way is added to both, so a handful of logs does not make a large ratio.
    let smoothed = (window + 1.0) / (baseline + 1.0);

    let change = if smoothed >= SURGE_RATIO && window - baseline >= MIN_MOVE {
        Change::Surging
    } else if smoothed <= 1.0 / SURGE_RATIO && baseline - window >= MIN_MOVE {
        Change::Dropping
    } else {
        Change::Steady
    };

    (change, Some(window / baseline))
}

/// How many logs of a stretch a pattern has, estimated from how many of those read do: `read`
/// logs were read of the stretch's `total`.
pub(super) fn estimate(count: usize, read: usize, total: u64) -> f64 {
    if read == 0 {
        0.0
    } else {
        count as f64 * total as f64 / read as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_the_baseline_did_not_have_is_new() {
        assert_eq!(classify(3.0, 0.0), (Change::New, None));
        assert_eq!(classify(0.0, 0.0).0, Change::Steady);
    }

    #[test]
    fn a_pattern_that_at_least_doubles_by_enough_logs_surges() {
        let (change, ratio) = classify(40.0, 10.0);

        assert_eq!(change, Change::Surging);
        assert_eq!(ratio, Some(4.0));
    }

    #[test]
    fn a_few_logs_more_are_steady_whatever_the_ratio() {
        assert_eq!(classify(3.0, 1.0).0, Change::Steady);
        assert_eq!(classify(1.0, 4.0).0, Change::Steady);
    }

    #[test]
    fn a_pattern_that_falls_to_half_by_enough_logs_drops() {
        assert_eq!(classify(2.0, 20.0).0, Change::Dropping);
        assert_eq!(classify(0.0, 9.0).0, Change::Dropping);
    }

    #[test]
    fn new_and_surging_patterns_come_first() {
        assert!(Change::New.rank() < Change::Surging.rank());
        assert!(Change::Surging.rank() < Change::Steady.rank());
        assert_eq!(Change::Steady.rank(), Change::Dropping.rank());
    }

    #[test]
    fn a_count_in_a_sample_is_scaled_to_the_stretch() {
        assert_eq!(estimate(10, 100, 1_000), 100.0);
        assert_eq!(estimate(10, 0, 1_000), 0.0);
        assert_eq!(estimate(7, 7, 7), 7.0);
    }
}
