// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Robust statistics of one run of values: its level, its spread, and where its level shifted.

use serde_json::{Value, json};

/// The median absolute deviation times this is the standard deviation of normally distributed
/// values.
const MAD_TO_SIGMA: f64 = 1.4826;

/// A side of a split shorter than this has no level of its own.
const MIN_SEGMENT: usize = 3;

/// The strength given to a shift between two flat runs, which no spread can measure.
const FLAT_SHIFT_STRENGTH: f64 = 999.0;

/// A shift is significant from this many robust standard deviations.
pub(super) const SIGNIFICANT_STRENGTH: f64 = 3.0;

pub(super) fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);

    let middle = sorted.len() / 2;

    Some(if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

pub(super) fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// The standard deviation of `values` around `center`, read from their median absolute
/// deviation; the root mean square deviation when more than half of them sit at the center, as
/// counts that are mostly zero do.
pub(super) fn robust_sigma(values: &[f64], center: f64) -> f64 {
    let deviations = values
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();

    let robust = median(&deviations).unwrap_or(0.0) * MAD_TO_SIGMA;

    if robust > 0.0 || deviations.is_empty() {
        return robust;
    }

    (deviations
        .iter()
        .map(|deviation| deviation * deviation)
        .sum::<f64>()
        / deviations.len() as f64)
        .sqrt()
}

/// Where a run of values changed level.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ChangePoint {
    /// The first value of the new level.
    pub(super) index: usize,
    pub(super) before: f64,
    pub(super) after: f64,
    /// The shift in robust standard deviations of the values around their own side's level.
    pub(super) strength: f64,
}

impl ChangePoint {
    pub(super) fn significant(&self) -> bool {
        self.strength >= SIGNIFICANT_STRENGTH && self.after != self.before
    }
}

/// Where the level of `values` shifted the most: of every split into a before and an after, the
/// one whose medians differ the most, measured in robust standard deviations of the values around
/// their own side's median. Each side keeps at least [`MIN_SEGMENT`] values, and a tenth of them
/// when there are more, so a spike at either end is not taken for a shift. `None` when there are
/// too few values to split.
pub(super) fn change_point(values: &[f64]) -> Option<ChangePoint> {
    let edge = MIN_SEGMENT.max(values.len() / 10);

    if values.len() < edge * 2 {
        return None;
    }

    let mut best: Option<ChangePoint> = None;

    for index in edge..=values.len() - edge {
        let (left, right) = values.split_at(index);

        let (Some(before), Some(after)) = (median(left), median(right)) else {
            continue;
        };

        let residuals = left
            .iter()
            .map(|value| value - before)
            .chain(right.iter().map(|value| value - after))
            .collect::<Vec<_>>();

        let noise = robust_sigma(&residuals, 0.0);
        let shift = (after - before).abs();

        let strength = if noise > 0.0 {
            (shift / noise).min(FLAT_SHIFT_STRENGTH)
        } else if shift > 0.0 {
            FLAT_SHIFT_STRENGTH
        } else {
            0.0
        };

        if best.as_ref().is_none_or(|best| strength > best.strength) {
            best = Some(ChangePoint {
                index,
                before,
                after,
                strength,
            });
        }
    }

    best
}

/// `after` relative to `before`, `None` when `before` is zero.
pub(super) fn relative(before: f64, after: f64) -> Option<f64> {
    (before != 0.0).then(|| (after - before) / before.abs())
}

pub(super) fn direction(before: f64, after: f64) -> &'static str {
    if after > before {
        "up"
    } else if after < before {
        "down"
    } else {
        "flat"
    }
}

/// A number for the report: six significant digits, `null` when it is not a finite number.
pub(super) fn num(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }

    if value == 0.0 {
        return json!(0.0);
    }

    let digits = 5 - value.abs().log10().floor() as i32;

    let rounded = if digits > 0 {
        let scale = 10_f64.powi(digits.min(12));
        (value * scale).round() / scale
    } else {
        value.round()
    };

    json!(rounded)
}

pub(super) fn opt_num(value: Option<f64>) -> Value {
    value.map_or(Value::Null, num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_value_or_between_the_two_middle_ones() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median(&[]), None);
        assert_eq!(mean(&[1.0, 2.0, 6.0]), Some(3.0));
    }

    #[test]
    fn the_spread_ignores_an_outlier_and_falls_back_when_most_values_agree() {
        let sigma = robust_sigma(&[1.0, 2.0, 3.0, 4.0, 100.0], 3.0);
        assert!((sigma - MAD_TO_SIGMA).abs() < 1e-9);

        // Mostly zero: the median deviation is zero, the spread is not.
        assert!(robust_sigma(&[0.0, 0.0, 0.0, 0.0, 10.0], 0.0) > 0.0);
        assert_eq!(robust_sigma(&[5.0, 5.0], 5.0), 0.0);
    }

    #[test]
    fn a_step_between_two_flat_runs_is_found_where_it_happened() {
        let values = [[10.0; 20], [20.0; 20]].concat();
        let point = change_point(&values).unwrap();

        assert_eq!(point.index, 20);
        assert_eq!((point.before, point.after), (10.0, 20.0));
        assert!(point.significant());
    }

    #[test]
    fn a_step_in_noisy_values_is_significant() {
        let values = (0..60)
            .map(|index| {
                let noise = if index % 2 == 0 { 0.5 } else { -0.5 };
                let level = if index < 35 { 100.0 } else { 130.0 };
                level + noise
            })
            .collect::<Vec<_>>();

        let point = change_point(&values).unwrap();

        assert_eq!(point.index, 35);
        assert!(point.significant());
        assert_eq!(direction(point.before, point.after), "up");
    }

    #[test]
    fn noise_around_one_level_is_no_significant_shift() {
        let values = (0..60)
            .map(|index| 50.0 + [0.0, 1.0, -1.0, 2.0, -2.0][index % 5])
            .collect::<Vec<_>>();

        assert!(!change_point(&values).unwrap().significant());
    }

    #[test]
    fn a_spike_at_the_end_is_not_a_shift_and_a_short_run_has_no_split() {
        let mut values = vec![10.0; 30];
        values.push(500.0);

        let point = change_point(&values).unwrap();
        assert!(!point.significant());
        assert_eq!(change_point(&[1.0, 2.0, 3.0, 4.0, 5.0]), None);
    }

    #[test]
    fn numbers_keep_six_significant_digits() {
        assert_eq!(num(1_234.567_89), json!(1_234.57));
        assert_eq!(num(0.000_123_456_789), json!(0.000_123_457));
        assert_eq!(num(f64::NAN), Value::Null);
        assert_eq!(num(0.0), json!(0.0));
        assert_eq!(relative(0.0, 5.0), None);
        assert_eq!(relative(10.0, 15.0), Some(0.5));
    }
}
