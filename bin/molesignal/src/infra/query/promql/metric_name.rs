// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where a series keeps its metric name (`__name__`) and where it loses it, as in Prometheus.
//!
//! A selector's series always carry `__name__`. Functions drop it, except the ones that only
//! pick, sort or relabel samples; arithmetic operators and `bool` comparisons drop it, filtering
//! comparisons and set operators keep it; `without` aggregations and unary minus drop it.
//! Vector matching never compares `__name__` unless `on(...)` names it.

use std::collections::HashSet;

use super::*;

pub(super) const METRIC_NAME_LABEL: &str = "__name__";

/// Functions whose output keeps the input's metric name: they pass samples through unchanged
/// (`last_over_time`, the sorts) or edit labels themselves (`label_replace`, `label_join`).
fn function_keeps_name(name: &str) -> bool {
    matches!(
        name,
        "label_replace"
            | "label_join"
            | "last_over_time"
            | "first_over_time"
            | "sort"
            | "sort_desc"
            | "sort_by_label"
            | "sort_by_label_desc"
    )
}

/// Whether a binary operator's result loses the metric name: arithmetic does, and so does a
/// comparison that returns `bool`; a filtering comparison returns its left-hand samples as they
/// are.
pub(super) fn binary_drops_name(op: TokenType, return_bool: bool) -> bool {
    match op.id() {
        T_ADD | T_SUB | T_MUL | T_DIV | T_MOD | T_POW => true,
        T_EQLC | T_NEQ | T_GTR | T_GTE | T_LSS | T_LTE => return_bool,
        _ => false,
    }
}

/// The instant result of function `name`, with the metric name dropped unless the function keeps
/// it.
pub(super) fn after_function_instant(name: &str, vector: InstantVector) -> Result<InstantVector> {
    if function_keeps_name(name) {
        return Ok(vector);
    }

    drop_name_instant(vector)
}

/// The range result of function `name`, with the metric name dropped unless the function keeps it.
pub(super) fn after_function_range(name: &str, vector: RangeVector) -> Result<RangeVector> {
    if function_keeps_name(name) {
        return Ok(vector);
    }

    drop_name_range(vector)
}

/// Removes `__name__` from every sample. Two samples of different metrics that differ in nothing
/// else would become one series, which Prometheus refuses; so is this.
pub(super) fn drop_name_instant(mut vector: InstantVector) -> Result<InstantVector> {
    let mut names = DroppedNames::default();

    for (labels, _) in &mut vector.items {
        names.note(labels.remove(METRIC_NAME_LABEL));
    }

    if names.may_collide() {
        let mut seen = HashSet::with_capacity(vector.items.len());

        for (labels, _) in &vector.items {
            if !seen.insert(labels) {
                return Err(same_labelset());
            }
        }
    }

    Ok(vector)
}

/// [`drop_name_instant`] for every step of a range result.
pub(super) fn drop_name_range(mut vector: RangeVector) -> Result<RangeVector> {
    let mut names = DroppedNames::default();

    for point in &mut vector.points {
        names.note(point.labels.remove(METRIC_NAME_LABEL));
    }

    if names.may_collide() {
        let mut seen = HashSet::with_capacity(vector.points.len());

        for point in &vector.points {
            if !seen.insert((point.ts_us, &point.labels)) {
                return Err(same_labelset());
            }
        }
    }

    Ok(vector)
}

pub(super) fn drop_name(labels: &mut LabelSet) {
    labels.remove(METRIC_NAME_LABEL);
}

fn same_labelset() -> Error {
    Error::invalid("promql vector cannot contain metrics with the same labelset")
}

/// The metric names removed from one result: with at most one, no two samples can merge.
#[derive(Default)]
struct DroppedNames {
    first: Option<String>,
    several: bool,
}

impl DroppedNames {
    fn note(&mut self, name: Option<String>) {
        let Some(name) = name else {
            return;
        };

        match &self.first {
            None => self.first = Some(name),
            Some(first) if *first != name => self.several = true,
            Some(_) => {}
        }
    }

    fn may_collide(&self) -> bool {
        self.several
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(pairs: &[(&str, &str)]) -> LabelSet {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn functions_drop_the_name_unless_they_keep_samples_as_they_are() {
        let vector = || InstantVector {
            items: vec![(ls(&[("__name__", "up"), ("job", "api")]), 1.0)],
        };

        let rated = after_function_instant("rate", vector()).unwrap();
        assert_eq!(rated.items[0].0, ls(&[("job", "api")]));

        let last = after_function_instant("last_over_time", vector()).unwrap();
        assert_eq!(last.items[0].0, ls(&[("__name__", "up"), ("job", "api")]));
    }

    #[test]
    fn dropping_names_that_merge_two_series_is_refused() {
        let vector = InstantVector {
            items: vec![
                (ls(&[("__name__", "a"), ("job", "api")]), 1.0),
                (ls(&[("__name__", "b"), ("job", "api")]), 2.0),
            ],
        };

        let error = drop_name_instant(vector).unwrap_err();
        assert!(error.to_string().contains("same labelset"), "{error}");

        let range = RangeVector {
            points: vec![
                RangePoint {
                    ts_us: 1,
                    labels: ls(&[("__name__", "a"), ("job", "api")]),
                    value: 1.0,
                },
                RangePoint {
                    ts_us: 2,
                    labels: ls(&[("__name__", "b"), ("job", "api")]),
                    value: 1.0,
                },
            ],
        };

        assert!(
            drop_name_range(range).is_ok(),
            "different steps never merge"
        );
    }
}
