// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a move of the whole came from: how much each series moved, and how much each value of a
//! label moved when its series are added up.
//!
//! A series' move is its mean after less its mean before; a side where the series has no points
//! counts as zero, so a series that appeared or went away moves by all it has. A label value's
//! share is its move over the move of the whole; a label's concentration is how much of the
//! movement, counted without its sign, its largest value carries.

use std::collections::{BTreeMap, BTreeSet};

/// The labels of a series.
pub(super) type Labels = BTreeMap<String, String>;

/// Labels with more values than this are not split by on their own.
const MAX_LABEL_VALUES: usize = 50;

/// The label every series of one metric shares, which says nothing about where a move is.
const NAME_LABEL: &str = "__name__";

/// How one series moved.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SeriesMove {
    pub(super) labels: Labels,
    /// Its mean before and after; `None` on a side where it had no points.
    pub(super) before: Option<f64>,
    pub(super) after: Option<f64>,
    pub(super) delta: f64,
}

/// How the series that share one value of a label moved, added up.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ValueMove {
    /// `None` for the series without the label.
    pub(super) value: Option<String>,
    pub(super) before: f64,
    pub(super) after: f64,
    pub(super) delta: f64,
}

/// How the values of one label moved, the largest move first.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LabelMove {
    pub(super) label: String,
    pub(super) values: Vec<ValueMove>,
    pub(super) concentration: f64,
}

/// The moves of the series, the largest first. `before` and `after` hold each series' mean on
/// its side.
pub(super) fn series_moves(
    before: &BTreeMap<Labels, f64>,
    after: &BTreeMap<Labels, f64>,
) -> Vec<SeriesMove> {
    let keys = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();

    let mut moves = keys
        .into_iter()
        .map(|labels| {
            let earlier = before.get(labels).copied();
            let later = after.get(labels).copied();

            SeriesMove {
                labels: labels.clone(),
                before: earlier,
                after: later,
                delta: later.unwrap_or(0.0) - earlier.unwrap_or(0.0),
            }
        })
        .collect::<Vec<_>>();

    moves.sort_by(|left, right| right.delta.abs().total_cmp(&left.delta.abs()));
    moves
}

/// The labels worth splitting a move by: those with two values or more, and not too many to read,
/// leaving out the metric's name.
pub(super) fn split_labels(moves: &[SeriesMove]) -> Vec<String> {
    let mut values = BTreeMap::<&str, BTreeSet<&str>>::new();

    for series in moves {
        for (label, value) in &series.labels {
            values
                .entry(label.as_str())
                .or_default()
                .insert(value.as_str());
        }
    }

    values
        .into_iter()
        .filter(|(label, values)| {
            *label != NAME_LABEL && values.len() >= 2 && values.len() <= MAX_LABEL_VALUES
        })
        .map(|(label, _)| label.to_string())
        .collect()
}

/// The moves of the values of each label in `labels`, at most `values_limit` values each; the
/// labels whose movement is most concentrated in one value first.
pub(super) fn label_moves(
    moves: &[SeriesMove],
    labels: &[String],
    values_limit: usize,
) -> Vec<LabelMove> {
    let mut found = labels
        .iter()
        .map(|label| label_move(moves, label, values_limit))
        .collect::<Vec<_>>();

    found.sort_by(|left, right| right.concentration.total_cmp(&left.concentration));
    found
}

fn label_move(moves: &[SeriesMove], label: &str, values_limit: usize) -> LabelMove {
    let mut sums = BTreeMap::<Option<&str>, (f64, f64)>::new();

    for series in moves {
        let entry = sums
            .entry(series.labels.get(label).map(String::as_str))
            .or_default();

        entry.0 += series.before.unwrap_or(0.0);
        entry.1 += series.after.unwrap_or(0.0);
    }

    let mut values = sums
        .into_iter()
        .map(|(value, (before, after))| ValueMove {
            value: value.map(str::to_string),
            before,
            after,
            delta: after - before,
        })
        .collect::<Vec<_>>();

    values.sort_by(|left, right| right.delta.abs().total_cmp(&left.delta.abs()));

    let movement = values.iter().map(|value| value.delta.abs()).sum::<f64>();

    let concentration = match values.first() {
        Some(largest) if movement > 0.0 => largest.delta.abs() / movement,
        _ => 0.0,
    };

    values.truncate(values_limit);

    LabelMove {
        label: label.to_string(),
        values,
        concentration,
    }
}

/// A move's share of the move of the whole, `None` when the whole did not move.
pub(super) fn share(delta: f64, total: f64) -> Option<f64> {
    (total.abs() > f64::EPSILON).then(|| delta / total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> Labels {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    /// A series' labels, its value before and its value after.
    type Side<'a> = (&'a [(&'a str, &'a str)], Option<f64>, Option<f64>);

    fn sides(rows: &[Side<'_>]) -> Vec<SeriesMove> {
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();

        for (pairs, earlier, later) in rows {
            if let Some(value) = earlier {
                before.insert(labels(pairs), *value);
            }

            if let Some(value) = later {
                after.insert(labels(pairs), *value);
            }
        }

        series_moves(&before, &after)
    }

    #[test]
    fn the_series_that_moved_most_comes_first_and_carries_most_of_the_move() {
        let moves = sides(&[
            (&[("host", "b"), ("route", "/pay")], Some(10.0), Some(12.0)),
            (&[("host", "a"), ("route", "/pay")], Some(10.0), Some(50.0)),
        ]);

        assert_eq!(moves[0].labels["host"], "a");
        assert_eq!(moves[0].delta, 40.0);

        let total = moves.iter().map(|series| series.delta).sum::<f64>();
        assert!((share(moves[0].delta, total).unwrap() - 40.0 / 42.0).abs() < 1e-9);

        let by_label = label_moves(&moves, &split_labels(&moves), 5);

        // `route` has one value, so the move is split by `host` alone.
        assert_eq!(by_label.len(), 1);
        assert_eq!(by_label[0].label, "host");
        assert_eq!(by_label[0].values[0].value.as_deref(), Some("a"));
        assert!((by_label[0].concentration - 40.0 / 42.0).abs() < 1e-9);
    }

    #[test]
    fn a_series_that_appeared_moves_by_all_it_has() {
        let moves = sides(&[
            (&[("pod", "p1")], Some(5.0), Some(5.0)),
            (&[("pod", "p2")], None, Some(7.0)),
        ]);

        assert_eq!(moves[0].labels["pod"], "p2");
        assert_eq!((moves[0].before, moves[0].after), (None, Some(7.0)));
        assert_eq!(moves[0].delta, 7.0);
        assert_eq!(moves[1].delta, 0.0);
    }

    #[test]
    fn the_metric_name_and_labels_with_one_value_are_not_split_by() {
        let moves = sides(&[
            (
                &[("__name__", "x"), ("env", "prod"), ("pod", "1")],
                Some(1.0),
                Some(2.0),
            ),
            (
                &[("__name__", "y"), ("env", "prod"), ("pod", "2")],
                Some(1.0),
                Some(3.0),
            ),
        ]);

        assert_eq!(split_labels(&moves), ["pod"]);
    }

    #[test]
    fn a_label_most_of_whose_move_is_one_value_comes_before_one_whose_move_is_spread() {
        let moves = sides(&[
            (&[("zone", "z1"), ("host", "h1")], Some(0.0), Some(10.0)),
            (&[("zone", "z1"), ("host", "h2")], Some(0.0), Some(10.0)),
            (&[("zone", "z2"), ("host", "h3")], Some(0.0), Some(1.0)),
        ]);

        let found = label_moves(&moves, &["host".to_string(), "zone".to_string()], 2);

        assert_eq!(found[0].label, "zone");
        assert_eq!(found[0].values[0].delta, 20.0);
        assert_eq!(found[1].values.len(), 2);
        assert_eq!(share(1.0, 0.0), None);
    }
}
