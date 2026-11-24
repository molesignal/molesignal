// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a chosen set of requests differs from the rest, field by field: the slow ones, or the
//! failed ones, against the others of the same query. For every field the share each value has
//! in the two sets is compared, and the fields are ranked by how far apart the shares are (the
//! total variation distance, half the sum of the differences: 0 when the two sets have the same
//! mix of values, 1 when they share none). A field whose values are nearly all different from
//! row to row, such as an id, says nothing about a set and is left out.
//!
//! Both sets are read from a sample of each, and say how many rows they hold.

mod spans;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde::Serialize;
pub use spans::{Selection, SpanComparison, candidates, compare_spans};

/// The most fields told, the furthest apart first.
pub const TOLD_DIMENSIONS: usize = 20;
/// The most values told of a field, those whose shares differ most first.
pub const TOLD_VALUES: usize = 5;
/// A field with distinct values in more than this share of the rows is left out.
const MOSTLY_DISTINCT: f64 = 0.5;
/// Below this many rows in both sets together, a field is not left out for being distinct.
const FEW_ROWS: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Rows read of the chosen set.
    pub selected: usize,
    /// Rows read of the rest.
    pub baseline: usize,
    /// Either set had more rows than were read.
    pub sampled: bool,
    pub dimensions: Vec<Dimension>,
}

/// One field and how its values are shared in the two sets.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    pub field: String,
    /// How far apart the two sets are on this field, from 0 to 1.
    pub score: f64,
    pub values: Vec<ValueShare>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueShare {
    /// `None` for the rows without a value.
    pub value: Option<String>,
    /// Its share of the chosen set, from 0 to 1.
    pub selected: f64,
    /// Its share of the rest.
    pub baseline: f64,
}

/// The rows of a set, a value (or none) for each field in the order the fields are given.
pub type Rows = Vec<Vec<Option<String>>>;

/// The fields ranked by how far apart the two sets are on them.
pub fn compare(fields: &[String], selected: &Rows, baseline: &Rows, sampled: bool) -> Comparison {
    let mut dimensions = Vec::new();

    if !selected.is_empty() && !baseline.is_empty() {
        for (index, field) in fields.iter().enumerate() {
            if let Some(dimension) = dimension(field, index, selected, baseline) {
                dimensions.push(dimension);
            }
        }
    }

    dimensions.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.field.cmp(&right.field))
    });
    dimensions.truncate(TOLD_DIMENSIONS);

    Comparison {
        selected: selected.len(),
        baseline: baseline.len(),
        sampled,
        dimensions,
    }
}

fn dimension(field: &str, index: usize, selected: &Rows, baseline: &Rows) -> Option<Dimension> {
    let chosen = counts(selected, index);
    let rest = counts(baseline, index);

    let mut values = chosen.keys().chain(rest.keys()).collect::<Vec<_>>();
    values.sort();
    values.dedup();

    let present = values.iter().filter(|value| value.is_some()).count();
    let rows = selected.len() + baseline.len();

    // Nothing told it, or an id: nearly every row has a value of its own.
    if present == 0 || (rows >= FEW_ROWS && present as f64 > MOSTLY_DISTINCT * rows as f64) {
        return None;
    }

    let share = |counts: &HashMap<Option<&str>, usize>, value: &Option<&str>, of: usize| {
        counts.get(value).copied().unwrap_or(0) as f64 / of as f64
    };

    let mut shares = values
        .into_iter()
        .map(|value| ValueShare {
            value: value.map(str::to_string),
            selected: share(&chosen, value, selected.len()),
            baseline: share(&rest, value, baseline.len()),
        })
        .collect::<Vec<_>>();

    let score = shares
        .iter()
        .map(|share| (share.selected - share.baseline).abs())
        .sum::<f64>()
        / 2.0;

    if score <= f64::EPSILON {
        return None;
    }

    shares.sort_by(|left, right| {
        (right.selected - right.baseline)
            .abs()
            .total_cmp(&(left.selected - left.baseline).abs())
            .then_with(|| right.selected.total_cmp(&left.selected))
            .then_with(|| left.value.cmp(&right.value))
    });
    shares.truncate(TOLD_VALUES);

    Some(Dimension {
        field: field.to_string(),
        score,
        values: shares,
    })
}

fn counts(rows: &Rows, index: usize) -> HashMap<Option<&str>, usize> {
    let mut counts = HashMap::new();

    for row in rows {
        let value = row.get(index).and_then(Option::as_deref);
        *counts.entry(value).or_default() += 1;
    }

    counts
}
