// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How the values of a number fall across the rows a query matches: bars of one width from the
//! smallest value to a high percentile, so that one value far off does not squeeze the others into a
//! bar. What lies past that is counted in the last bar.
//!
//! Two statements: the first finds where the bars lie and the percentiles that mark them, the second
//! counts the rows of each bar. The rows are counted as they are stored, without estimates.

use domain::stream::FieldType;
use serde::Serialize;
use serde_json::Value;

use super::{
    member::Column,
    sql::{Relation, Scope},
};

/// The most bars a question gets unless it says, and the most it can ask for.
pub const DEFAULT_BARS: usize = 40;
pub const MAX_BARS: usize = 100;

/// Up to how many rows the percentiles are worked out exactly. Past it the estimate stands: it is
/// close at that size, and the exact figure keeps every value in memory.
pub const EXACT_PERCENTILES_UP_TO: u64 = 100_000;

/// How many bars the rows of a question fill unless it says: twice the cube root of the rows (the
/// Rice rule), so that three values make three bars and not two slivers at the ends of forty. From
/// 8,000 rows on, `DEFAULT_BARS`.
pub fn bars_for(rows: u64) -> usize {
    let bars = (2.0 * (rows as f64).cbrt()).ceil() as usize;

    bars.clamp(1, DEFAULT_BARS)
}

const ROWS: &str = "__rows";
const LOW: &str = "__low";
const TOP: &str = "__top";
const P50: &str = "__p50";
const P75: &str = "__p75";
const P95: &str = "__p95";
const P99: &str = "__p99";
const BAR: &str = "__bar";

/// One bar: the rows whose value is from `from` up to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DistributionBar {
    pub from: f64,
    pub to: f64,
    pub rows: u64,
}

/// What a number comes to across the rows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Distribution {
    pub rows: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub p50: Option<f64>,
    pub p75: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub bars: Vec<DistributionBar>,
}

/// The four percentiles that mark the bars, by `function` of DataFusion.
fn percentiles(function: &str, value: &str) -> String {
    [(0.5, P50), (0.75, P75), (0.95, P95), (0.99, P99)]
        .map(|(at, name)| format!("{function}({at}) WITHIN GROUP (ORDER BY {value}) AS \"{name}\""))
        .join(", ")
}

/// Where the bars lie: read by the first statement, with the percentiles estimated.
pub fn bounds_sql(scope: &Scope<'_>, column: &Column) -> String {
    let relation = Relation::of(scope, &[column.stored_name()]);
    let value = column.number();
    let present = [format!("{value} IS NOT NULL")];

    format!(
        "{}SELECT COUNT({value}) AS \"{ROWS}\", MIN({value}) AS \"{LOW}\", MAX({value}) AS \"{TOP}\", \
         {} FROM {} WHERE {}",
        relation.with,
        percentiles("approx_percentile_cont", &value),
        relation.from,
        relation.where_clause(&present),
    )
}

/// The percentiles worked out exactly, for a question of few rows. The estimate of the first
/// statement can fall where no value is: 25.5, 38.5, 51.5 and 51.5 seconds made a median of 48.25.
pub fn exact_percentiles_sql(scope: &Scope<'_>, column: &Column) -> String {
    let relation = Relation::of(scope, &[column.stored_name()]);
    let value = column.number();
    let present = [format!("{value} IS NOT NULL")];

    format!(
        "{}SELECT {} FROM {} WHERE {}",
        relation.with,
        percentiles("percentile_cont", &value),
        relation.from,
        relation.where_clause(&present),
    )
}

/// The exact percentiles in place of the estimates; an estimate stays where none came back.
pub fn read_exact_percentiles(
    mut found: Distribution,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Distribution {
    let Some(row) = rows.first() else {
        return found;
    };

    found.p50 = number(columns, row, P50).or(found.p50);
    found.p75 = number(columns, row, P75).or(found.p75);
    found.p95 = number(columns, row, P95).or(found.p95);
    found.p99 = number(columns, row, P99).or(found.p99);

    found
}

fn number(columns: &[String], row: &[Value], name: &str) -> Option<f64> {
    let at = columns.iter().position(|column| column == name)?;

    row.get(at)?.as_f64().filter(|value| value.is_finite())
}

/// What the first statement came to; the bars are still to be counted.
pub fn read_bounds(columns: &[String], rows: &[Vec<Value>]) -> Distribution {
    let Some(row) = rows.first() else {
        return Distribution::default();
    };

    Distribution {
        rows: number(columns, row, ROWS).map_or(0, |rows| rows.max(0.0) as u64),
        min: number(columns, row, LOW),
        max: number(columns, row, TOP),
        p50: number(columns, row, P50),
        p75: number(columns, row, P75),
        p95: number(columns, row, P95),
        p99: number(columns, row, P99),
        bars: Vec::new(),
    }
}

/// The bars to count a distribution in; none when every value is the same, which is one bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarPlan {
    low: f64,
    width: f64,
    count: usize,
}

impl BarPlan {
    /// A whole number is told in whole steps: half a click is not a thing.
    pub fn of(found: &Distribution, column: &Column, bars: usize) -> Option<Self> {
        let (low, high) = (found.min?, found.p99.or(found.max)?);
        let bars = bars.clamp(1, MAX_BARS);
        let span = high - low;

        if span <= 0.0 {
            return None;
        }

        let width = span / bars as f64;

        let width = if column.data_type() == FieldType::Int64 {
            width.ceil().max(1.0)
        } else {
            width
        };

        Some(Self {
            low,
            width,
            count: bars,
        })
    }
}

/// The rows of each bar.
pub fn bars_sql(scope: &Scope<'_>, column: &Column, plan: BarPlan) -> String {
    let relation = Relation::of(scope, &[column.stored_name()]);
    let value = column.number();
    let present = [format!("{value} IS NOT NULL")];

    format!(
        "{}SELECT LEAST({last}, CAST(FLOOR((CAST({value} AS DOUBLE) - {low:.6}) / {width:.6}) AS BIGINT)) \
         AS \"{BAR}\", COUNT(*) AS \"{ROWS}\" FROM {} WHERE {} GROUP BY 1 ORDER BY 1 ASC",
        relation.with,
        relation.from,
        relation.where_clause(&present),
        last = plan.count - 1,
        low = plan.low,
        width = plan.width,
    )
}

/// What the second statement came to, in the bars of the plan. The last bar reaches to the largest
/// value, since it counts what lies past the others.
pub fn read_bars(
    mut found: Distribution,
    plan: BarPlan,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Distribution {
    let mut counts = vec![0_u64; plan.count];

    for row in rows {
        let bar = number(columns, row, BAR).map_or(0, |bar| bar.max(0.0) as usize);

        if let Some(slot) = counts.get_mut(bar) {
            *slot = number(columns, row, ROWS).map_or(0, |rows| rows.max(0.0) as u64);
        }
    }

    let last = plan.count - 1;

    found.bars = counts
        .into_iter()
        .enumerate()
        .map(|(index, rows)| {
            let from = plan.low + index as f64 * plan.width;
            let to = from + plan.width;

            DistributionBar {
                from,
                to: if index == last {
                    found.max.map_or(to, |top| top.max(to))
                } else {
                    to
                },
                rows,
            }
        })
        .collect();

    found
}

/// The one bar of values that are all the same.
pub fn single_bar(mut found: Distribution) -> Distribution {
    if let (Some(from), true) = (found.min, found.rows > 0) {
        found.bars = vec![DistributionBar {
            from,
            to: found.max.unwrap_or(from),
            rows: found.rows,
        }];
    }

    found
}

#[cfg(test)]
mod tests {
    use domain::rum::event::RumStream;
    use serde_json::json;

    use super::*;
    use crate::infra::rum::query::{Condition, member::resolved};

    fn scope<'a>(stream: RumStream, conditions: &'a [Condition]) -> Scope<'a> {
        Scope {
            stream,
            from: 1,
            to: 2,
            conditions,
        }
    }

    fn found(min: f64, p99: f64, max: f64) -> Distribution {
        Distribution {
            rows: 10,
            min: Some(min),
            max: Some(max),
            p99: Some(p99),
            ..Distribution::default()
        }
    }

    #[test]
    fn the_first_statement_finds_where_the_bars_lie() {
        let column = resolved(RumStream::Resources, "resource.duration").unwrap();
        let sql = bounds_sql(&scope(RumStream::Resources, &[]), &column);

        assert!(
            sql.contains("COUNT(\"resource.duration\") AS \"__rows\""),
            "{sql}"
        );
        assert!(sql.contains("approx_percentile_cont(0.99)"), "{sql}");
        assert!(sql.contains("\"resource.duration\" IS NOT NULL"), "{sql}");
    }

    #[test]
    fn a_view_is_counted_at_its_latest_revision() {
        let column = resolved(RumStream::Views, "view.loading_time").unwrap();
        let sql = bounds_sql(&scope(RumStream::Views, &[]), &column);

        assert!(sql.starts_with("WITH "), "{sql}");
    }

    #[test]
    fn a_key_of_a_json_column_is_measured_as_a_number() {
        let column = resolved(RumStream::Views, "view.custom_timings.cart").unwrap();
        let sql = bounds_sql(&scope(RumStream::Views, &[]), &column);

        assert!(
            sql.contains("TRY_CAST(json_text(\"view.custom_timings\", 'cart') AS DOUBLE)"),
            "{sql}"
        );
    }

    #[test]
    fn bars_run_from_the_smallest_value_to_a_high_percentile_and_the_last_takes_the_rest() {
        let column = resolved(RumStream::Resources, "resource.size").unwrap();
        let plan = BarPlan::of(&found(0.0, 10.0, 500.0), &column, 5).unwrap();
        let sql = bars_sql(&scope(RumStream::Resources, &[]), &column, plan);

        assert!(sql.contains("LEAST(4, CAST(FLOOR("), "{sql}");
        assert!(sql.contains("/ 2.000000) AS BIGINT)"), "{sql}");

        let columns = [BAR.to_string(), ROWS.to_string()];

        let read = read_bars(
            found(0.0, 10.0, 500.0),
            plan,
            &columns,
            &[vec![json!(0), json!(3)], vec![json!(4), json!(7)]],
        );

        assert_eq!(read.bars.len(), 5);
        assert_eq!(
            read.bars[0],
            DistributionBar {
                from: 0.0,
                to: 2.0,
                rows: 3
            }
        );
        assert_eq!(read.bars[1].rows, 0);
        assert_eq!(
            read.bars[4],
            DistributionBar {
                from: 8.0,
                to: 500.0,
                rows: 7
            }
        );
    }

    #[test]
    fn a_whole_number_is_told_in_whole_steps() {
        let column = resolved(RumStream::Views, "view.action.count").unwrap();
        let plan = BarPlan::of(&found(0.0, 3.0, 3.0), &column, 40).unwrap();

        assert_eq!(plan.width, 1.0);
    }

    #[test]
    fn values_that_are_all_the_same_are_one_bar() {
        let column = resolved(RumStream::Views, "view.action.count").unwrap();

        assert!(BarPlan::of(&found(4.0, 4.0, 4.0), &column, 40).is_none());
        assert_eq!(
            single_bar(found(4.0, 4.0, 4.0)).bars,
            [DistributionBar {
                from: 4.0,
                to: 4.0,
                rows: 10
            }]
        );
        assert!(single_bar(Distribution::default()).bars.is_empty());
    }

    #[test]
    fn the_rows_say_how_many_bars_they_fill() {
        assert_eq!(bars_for(0), 1);
        assert_eq!(bars_for(3), 3);
        assert_eq!(bars_for(96), 10);
        assert_eq!(bars_for(8_000), DEFAULT_BARS);
        assert_eq!(bars_for(5_000_000), DEFAULT_BARS);
    }

    #[test]
    fn few_rows_have_their_percentiles_worked_out() {
        let column = resolved(RumStream::Resources, "resource.duration").unwrap();
        let sql = exact_percentiles_sql(&scope(RumStream::Resources, &[]), &column);

        assert!(sql.contains("percentile_cont(0.5) WITHIN GROUP"), "{sql}");
        assert!(!sql.contains("approx_"), "{sql}");

        let read = read_exact_percentiles(
            found(1.0, 9.0, 10.0),
            &["__p50".to_string()],
            &[vec![json!(5.0)]],
        );

        // What came back takes the estimate's place; what did not leaves it.
        assert_eq!(read.p50, Some(5.0));
        assert_eq!(read.p99, Some(9.0));
    }

    #[test]
    fn no_rows_come_to_nothing() {
        let read = read_bounds(&[], &[]);

        assert_eq!(read, Distribution::default());
        assert!(
            BarPlan::of(
                &read,
                &resolved(RumStream::Views, "view.action.count").unwrap(),
                40
            )
            .is_none()
        );
    }
}
