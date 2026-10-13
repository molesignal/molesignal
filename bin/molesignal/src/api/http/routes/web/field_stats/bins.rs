// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bars a number's distribution is counted into.
//!
//! A bar is as wide as a number a reader counts in (1, 2, 5, 10 and their
//! tenfolds) and begins on a multiple of that width, so the edges of the bars
//! are the labels of the axis.

/// About how many bars a distribution is drawn in.
const TARGET_BARS: f64 = 24.0;
/// A plan with more bars than this is a mistake of arithmetic, not a chart.
const MAX_BARS: usize = 64;

/// Bars of one width covering `[min, max]`, the first beginning at `origin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BinPlan {
    min: f64,
    max: f64,
    origin: f64,
    width: f64,
    bars: usize,
}

impl BinPlan {
    /// `None` where there is no distribution to draw: no values, values that
    /// are not numbers, or one value only.
    ///
    /// `whole` says the values are whole numbers: a bar is then at least one
    /// wide, so that no bar stands between two values that can occur.
    pub(super) fn new(min: f64, max: f64, whole: bool) -> Option<Self> {
        if !min.is_finite() || !max.is_finite() || max <= min {
            return None;
        }

        let wanted = (max - min) / TARGET_BARS;
        // A whole power of ten, by division where it is below one: 0.01 and not a neighbour of it.
        let magnitude = 10f64.powi(wanted.log10().floor() as i32);

        let steps: &[f64] = if whole {
            &[1.0, 2.0, 5.0, 10.0]
        } else {
            &[1.0, 2.0, 2.5, 5.0, 10.0]
        };

        let width = steps
            .iter()
            .map(|step| step * magnitude)
            .find(|width| *width >= wanted)
            .unwrap_or(10.0 * magnitude);

        let width = if whole { width.max(1.0) } else { width };
        let origin = (min / width).floor() * width;
        let bars = ((max - origin) / width).floor();

        // A range too wide for a double to step through has no bars to count.
        if !origin.is_finite() || !bars.is_finite() || bars < 0.0 || bars >= MAX_BARS as f64 {
            return None;
        }

        Some(Self {
            min,
            max,
            origin,
            width,
            bars: bars as usize + 1,
        })
    }

    pub(super) fn min(&self) -> f64 {
        self.min
    }

    pub(super) fn max(&self) -> f64 {
        self.max
    }

    pub(super) fn origin(&self) -> f64 {
        self.origin
    }

    pub(super) fn width(&self) -> f64 {
        self.width
    }

    /// How many bars there are from the first to the one the largest value is in.
    pub(super) fn bins(&self) -> usize {
        self.bars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_are_a_round_width_and_begin_on_a_multiple_of_it() {
        let plan = BinPlan::new(3.0, 1_187.0, true).expect("plan");

        assert_eq!(plan.width(), 50.0);
        assert_eq!(plan.origin(), 0.0);
        assert_eq!(plan.bins(), 24);
    }

    #[test]
    fn whole_numbers_are_never_counted_into_bars_narrower_than_one() {
        // 200, 201, 204: three values a tenth of a bar apart would leave nine bars empty.
        let plan = BinPlan::new(200.0, 204.0, true).expect("plan");

        assert_eq!(plan.width(), 1.0);
        assert_eq!(plan.origin(), 200.0);
        assert_eq!(plan.bins(), 5);
    }

    #[test]
    fn fractions_are_counted_into_bars_as_fine_as_they_are() {
        let plan = BinPlan::new(0.012, 0.98, false).expect("plan");

        assert_eq!(plan.width(), 0.05);
        assert_eq!(plan.origin(), 0.0);
        assert_eq!(plan.bins(), 20);
    }

    #[test]
    fn values_below_zero_begin_on_a_multiple_below_them() {
        let plan = BinPlan::new(-7.5, 7.5, false).expect("plan");

        assert_eq!(plan.width(), 1.0);
        assert_eq!(plan.origin(), -8.0);
        assert_eq!(plan.bins(), 16);
    }

    #[test]
    fn one_value_or_no_number_has_no_distribution() {
        assert_eq!(BinPlan::new(5.0, 5.0, true), None);
        assert_eq!(BinPlan::new(5.0, 1.0, true), None);
        assert_eq!(BinPlan::new(f64::NAN, 1.0, false), None);
        assert_eq!(BinPlan::new(0.0, f64::INFINITY, false), None);
    }
}
