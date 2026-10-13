// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The stretches of time the commonest values of a field are counted over.
//!
//! A stretch is a length of time a reader counts in (a minute, five, an hour)
//! and begins on a multiple of it, so the points of the trend fall on round
//! moments. There are few enough of them to be drawn in a narrow panel.

/// About how many points a trend is drawn in.
const TARGET_STRETCHES: i64 = 48;
/// A plan with more stretches than this is a mistake of arithmetic, not a chart.
const MAX_STRETCHES: i64 = 128;
const SECOND: i64 = 1_000_000;

/// The lengths a stretch can have, in seconds: each a whole number of the one
/// a reader would count the next in.
const LENGTHS: [i64; 17] = [
    1, 5, 10, 15, 30, 60, 300, 600, 900, 1_800, 3_600, 7_200, 10_800, 21_600, 43_200, 86_400,
    604_800,
];

/// Stretches of one length covering `[from, to)`, the first beginning at
/// `origin`, at or before `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TrendPlan {
    origin: i64,
    stretch: i64,
    stretches: usize,
}

impl TrendPlan {
    /// `None` for a range that is empty, or too long to count in weeks.
    pub(super) fn new(from: i64, to: i64) -> Option<Self> {
        let span = to.checked_sub(from).filter(|span| *span > 0)?;

        let stretch = LENGTHS
            .iter()
            .map(|seconds| seconds * SECOND)
            .find(|stretch| (span - 1) / stretch < TARGET_STRETCHES)
            .unwrap_or(LENGTHS[LENGTHS.len() - 1] * SECOND);

        let origin = from.div_euclid(stretch).checked_mul(stretch)?;
        let stretches = (to - 1).checked_sub(origin)? / stretch + 1;

        (1..=MAX_STRETCHES).contains(&stretches).then_some(Self {
            origin,
            stretch,
            stretches: stretches as usize,
        })
    }

    pub(super) fn origin(&self) -> i64 {
        self.origin
    }

    pub(super) fn stretch(&self) -> i64 {
        self.stretch
    }

    pub(super) fn stretches(&self) -> usize {
        self.stretches
    }

    /// The moment the last stretch ends: nothing at or after it is counted.
    pub(super) fn end(&self) -> i64 {
        self.origin + self.stretch * self.stretches as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60 * SECOND;
    const HOUR: i64 = 60 * MINUTE;

    #[test]
    fn an_hour_is_counted_in_minutes_past_the_hour() {
        // Seven past to seven past: the first stretch begins on the five before.
        let plan = TrendPlan::new(7 * MINUTE, HOUR + 7 * MINUTE).expect("plan");

        assert_eq!(plan.stretch(), 5 * MINUTE);
        assert_eq!(plan.origin(), 5 * MINUTE);
        assert_eq!(plan.stretches(), 13);
        assert_eq!(plan.end(), HOUR + 10 * MINUTE);
    }

    #[test]
    fn a_day_is_counted_in_half_hours() {
        let plan = TrendPlan::new(0, 24 * HOUR).expect("plan");

        assert_eq!(plan.stretch(), 30 * MINUTE);
        assert_eq!(plan.stretches(), 48);
    }

    #[test]
    fn a_moment_before_the_epoch_still_begins_on_a_multiple() {
        let plan = TrendPlan::new(-90 * SECOND, 30 * SECOND).expect("plan");

        assert_eq!(plan.stretch(), 5 * SECOND);
        assert_eq!(plan.origin(), -90 * SECOND);
        assert_eq!(plan.stretches(), 24);
    }

    #[test]
    fn no_range_or_too_long_a_range_has_no_trend() {
        assert_eq!(TrendPlan::new(5, 5), None);
        assert_eq!(TrendPlan::new(9, 5), None);
        assert_eq!(TrendPlan::new(0, 400 * 7 * 24 * HOUR), None);
        assert_eq!(TrendPlan::new(i64::MIN, i64::MAX), None);
    }
}
