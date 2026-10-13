// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements a field is read with.
//!
//! Every result column is written over the field itself and named with a
//! prefix no field has. A result column inherits the masking of what it is
//! derived from, so the counts of a masked field come back masked with it.

use super::{Analysed, FieldKind, Scope, bins::BinPlan, trend::TrendPlan};
use crate::infra::query::escape_sql_ident;

/// When a row happened, in microseconds. `_timestamp` is stored as a moment,
/// which the engine does not compare with a whole number.
const MOMENT: &str = "CAST(_timestamp AS BIGINT)";

pub(super) const TOTAL: &str = "__fs_total";
pub(super) const PRESENT: &str = "__fs_present";
pub(super) const DISTINCT: &str = "__fs_distinct";
pub(super) const MIN: &str = "__fs_min";
pub(super) const MAX: &str = "__fs_max";
pub(super) const AVG: &str = "__fs_avg";
pub(super) const Q1: &str = "__fs_q1";
pub(super) const MEDIAN: &str = "__fs_median";
pub(super) const Q3: &str = "__fs_q3";
pub(super) const STDDEV_SAMP: &str = "__fs_stddev_samp";
pub(super) const STDDEV_POP: &str = "__fs_stddev_pop";
pub(super) const VALUE: &str = "__fs_value";
pub(super) const COUNT: &str = "__fs_count";
pub(super) const BAR: &str = "__fs_bar";
pub(super) const STRETCH: &str = "__fs_stretch";
pub(super) const MOMENT_2: &str = "__fs_m2";
pub(super) const MOMENT_3: &str = "__fs_m3";
pub(super) const MOMENT_4: &str = "__fs_m4";

/// A field as the statements read it.
struct Column {
    /// The field itself, quoted.
    name: String,
    /// `NULL` where a row has no value. An empty string is no value either: it
    /// is what the explorer's own field list counts as missing.
    present: String,
    /// The value as text, which is how values are told apart and listed.
    text: String,
    /// The value as a number a sum can be taken of. `None` for what has none.
    number: Option<String>,
}

impl Column {
    fn of(field: &Analysed<'_>) -> Self {
        let name = format!("\"{}\"", escape_sql_ident(field.name));

        match field.kind {
            FieldKind::Text => Self {
                present: format!("NULLIF({name}, '')"),
                text: format!("NULLIF({name}, '')"),
                number: None,
                name,
            },
            // A moment is stored as a count of microseconds, and read as one.
            FieldKind::Time => Self {
                present: name.clone(),
                text: format!("CAST(CAST({name} AS BIGINT) AS VARCHAR)"),
                number: None,
                name,
            },
            FieldKind::Bool => Self {
                present: name.clone(),
                text: format!("CAST({name} AS VARCHAR)"),
                number: None,
                name,
            },
            FieldKind::Number => Self {
                present: name.clone(),
                text: format!("CAST({name} AS VARCHAR)"),
                number: Some(format!("CAST({name} AS DOUBLE)")),
                name,
            },
        }
    }
}

fn source(scope: &Scope) -> String {
    format!(
        "FROM \"{}\" WHERE {}",
        escape_sql_ident(&scope.stream),
        scope.clauses.join(" AND "),
    )
}

/// A number as a SQL literal that is read back as the same number. Written
/// with an exponent, it is a double whatever its size.
fn literal(value: f64) -> String {
    format!("({value:e})")
}

/// One row: how many rows there are, how many have a value, about how many
/// different values, and for a number or a moment what the values run from and
/// to. A number also has its average, its spread and its quartiles.
///
/// The different values are estimated, which takes one pass and a fixed amount
/// of memory whatever the field holds. So are the quartiles.
pub(super) fn summary(scope: &Scope, field: &Analysed<'_>) -> String {
    let column = Column::of(field);

    let mut select = vec![
        format!("COUNT(*) AS {TOTAL}"),
        format!("COUNT({}) AS {PRESENT}", column.present),
        format!("approx_distinct({}) AS {DISTINCT}", column.text),
    ];

    match (field.kind, &column.number) {
        (FieldKind::Number, Some(number)) => {
            let name = &column.name;

            select.extend([
                // Kept as written: a whole number past 2^53 is not a double.
                format!("CAST(MIN({name}) AS VARCHAR) AS {MIN}"),
                format!("CAST(MAX({name}) AS VARCHAR) AS {MAX}"),
                format!("AVG({number}) AS {AVG}"),
                format!("stddev_samp({number}) AS {STDDEV_SAMP}"),
                format!("stddev_pop({number}) AS {STDDEV_POP}"),
            ]);

            select.extend(
                [(0.25, Q1), (0.5, MEDIAN), (0.75, Q3)].map(|(share, alias)| {
                    format!(
                        "approx_percentile_cont({share}) WITHIN GROUP (ORDER BY {number}) AS \
                         {alias}"
                    )
                }),
            );
        }
        (FieldKind::Time, _) => {
            let micros = format!("CAST({} AS BIGINT)", column.name);

            select.extend([
                format!("CAST(MIN({micros}) AS VARCHAR) AS {MIN}"),
                format!("CAST(MAX({micros}) AS VARCHAR) AS {MAX}"),
            ]);
        }
        _ => {}
    }

    format!("SELECT {} {}", select.join(", "), source(scope))
}

/// The commonest values with how many rows hold each, commonest first; values
/// equally common are in the order of the values themselves.
pub(super) fn top_values(scope: &Scope, field: &Analysed<'_>, limit: usize) -> String {
    let column = Column::of(field);

    format!(
        "SELECT {text} AS {VALUE}, COUNT(*) AS {COUNT} {source} AND {present} IS NOT NULL GROUP \
         BY 1 ORDER BY 2 DESC, 1 ASC LIMIT {limit}",
        text = column.text,
        present = column.present,
        source = source(scope),
    )
}

/// The values of a number counted into the bars of `plan`, one row for each
/// bar that holds any.
///
/// Each row also carries what its values add to the second, third and fourth
/// moments about `mean`. Taken about the mean the powers stay small: the
/// moments of large values read from raw sums would be lost to rounding.
pub(super) fn bars(scope: &Scope, field: &Analysed<'_>, plan: &BinPlan, mean: f64) -> String {
    let column = Column::of(field);
    // Only a number has bars; the caller plans none for anything else.
    let number = column.number.unwrap_or(column.name);
    let offset = format!("({number} - {})", literal(mean));

    format!(
        "SELECT CAST(FLOOR(({number} - {origin}) / {width}) AS BIGINT) AS {BAR}, COUNT(*) AS \
         {COUNT}, SUM(POWER({offset}, 2)) AS {MOMENT_2}, SUM(POWER({offset}, 3)) AS {MOMENT_3}, \
         SUM(POWER({offset}, 4)) AS {MOMENT_4} {source} AND {number} >= {low} AND {number} <= \
         {high} GROUP BY 1",
        origin = literal(plan.origin()),
        width = literal(plan.width()),
        // What is not a number at all (NaN) lies outside every bar.
        low = literal(plan.min()),
        high = literal(plan.max()),
        source = source(scope),
    )
}

/// How many rows hold each of `values` in each stretch of `plan`, one row for
/// each value and stretch that has any.
///
/// The values are the commonest ones as the ranking returned them, so they are
/// compared in the form they were read in.
pub(super) fn trend(
    scope: &Scope,
    field: &Analysed<'_>,
    plan: &TrendPlan,
    values: &[&str],
) -> String {
    let column = Column::of(field);

    let listed = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "SELECT ({MOMENT} - {origin}) / {stretch} AS {STRETCH}, {text} AS {VALUE}, COUNT(*) AS \
         {COUNT} {source} AND {text} IN ({listed}) AND {MOMENT} >= {origin} AND {MOMENT} < {end} \
         GROUP BY 1, 2",
        origin = plan.origin(),
        stretch = plan.stretch(),
        end = plan.end(),
        text = column.text,
        source = source(scope),
    )
}
