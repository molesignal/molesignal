// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One number of an answer: what a measure is asked as, and the statement it is written as.
//!
//! A measure counts what is stored, or estimates what there would be had every session been kept:
//! each row then counts for as many as the share it was kept with says (`retention.rate`; a row
//! without one counts for one). Counts, sums and averages are made of the shares; a percentile is
//! weighed by them; a smallest or a largest is a value that was seen; the people and the
//! sessions that were seen are counted as they were, and the sessions are then scaled by the
//! mean share of the rows they are told from.

use serde::{Deserialize, Serialize};

use super::super::{
    filter::{Condition, Filter, conditions},
    member::{Column, resolved},
    sql::{WEIGHT, WEIGHT_COLUMN},
};
use crate::{
    domain::{rum::event::RumStream, stream::FieldType},
    shared::{Error, Result},
};

/// Longest name a request gives a measure.
const MAX_NAME_LEN: usize = 64;

/// A measure as a page asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MeasureSpec {
    /// `count`, `count_distinct`, `sum`, `avg`, `min`, `max`, `p50`, `p75`, `p90`, `p95` or `p99`;
    /// or `stored`, which counts the rows that are stored whatever share they were kept with.
    #[serde(rename = "fn")]
    pub function: String,
    /// What is measured. `count` without one counts the rows; with one, the rows that have it.
    #[serde(default)]
    pub field: Option<String>,
    /// For `count_distinct`: the column to take where `field` has no value, so that users
    /// without an id are told apart by their anonymous id.
    #[serde(default)]
    pub fallback: Option<String>,
    /// Only the rows that meet these are measured.
    #[serde(default, rename = "where")]
    pub filters: Vec<Filter>,
    /// What the answer calls the measure: the function and the field by default.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Function {
    Count,
    /// The rows that are stored, not what they stand for.
    Stored,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
    /// The value that this many percent of the values are at or under.
    Percentile(u8),
}

impl Function {
    fn parse(text: &str) -> Result<Self> {
        Ok(match text.trim().to_ascii_lowercase().as_str() {
            "count" => Self::Count,
            "stored" => Self::Stored,
            "count_distinct" => Self::CountDistinct,
            "sum" => Self::Sum,
            "avg" => Self::Avg,
            "min" => Self::Min,
            "max" => Self::Max,
            "p50" => Self::Percentile(50),
            "p75" => Self::Percentile(75),
            "p90" => Self::Percentile(90),
            "p95" => Self::Percentile(95),
            "p99" => Self::Percentile(99),
            other => {
                return Err(Error::invalid(format!(
                    "unsupported measure `{other}`: use count, stored, count_distinct, sum, avg, \
                     min, max, p50, p75, p90, p95 or p99"
                )));
            }
        })
    }

    pub(super) fn name(self) -> String {
        match self {
            Self::Count => "count".into(),
            Self::Stored => "stored".into(),
            Self::CountDistinct => "count_distinct".into(),
            Self::Sum => "sum".into(),
            Self::Avg => "avg".into(),
            Self::Min => "min".into(),
            Self::Max => "max".into(),
            Self::Percentile(share) => format!("p{share}"),
        }
    }
}

/// One number of an answer, its statement written once.
#[derive(Debug, Clone)]
pub struct Measure {
    pub(super) name: String,
    pub(super) function: Function,
    pub(super) field: Option<String>,
    pub(super) unit: &'static str,
    pub(super) sql: String,
    /// Every column the statement reads for it, the ones of its own conditions too.
    pub(super) columns: Vec<String>,
}

/// An estimate written as a whole number, and none as nought.
fn rounded(estimate: &str) -> String {
    format!("CAST(COALESCE(ROUND({estimate}), 0) AS BIGINT)")
}

impl Measure {
    /// The measure `spec` asks for, of the rows that are stored (`estimate` false) or of the
    /// sessions they stand for.
    pub fn new(stream: RumStream, spec: &MeasureSpec, estimate: bool) -> Result<Self> {
        let function = Function::parse(&spec.function)?;
        let column = named(stream, spec.field.as_deref())?;
        let fallback = named(stream, spec.fallback.as_deref())?;

        if fallback.is_some() && function != Function::CountDistinct {
            return Err(Error::invalid("`fallback` goes with count_distinct"));
        }

        let conditions = conditions(stream, &spec.filters)?;
        let when = taken(&conditions);
        let guard = |value: String| guarded(when.as_deref(), value);

        let (sql, unit) = match (function, column.as_ref()) {
            (Function::Count | Function::Stored, None) => (
                match (when.as_deref(), estimate && function == Function::Count) {
                    (None, false) => "COUNT(*)".to_string(),
                    (Some(when), false) => format!("COUNT(CASE WHEN {when} THEN 1 END)"),
                    (None, true) => rounded(&format!("SUM({WEIGHT})")),
                    (Some(when), true) => {
                        rounded(&format!("SUM(CASE WHEN {when} THEN {WEIGHT} END)"))
                    }
                },
                "count",
            ),
            (Function::Count | Function::Stored, Some(column)) => {
                let value = guard(column.value());

                (
                    if estimate && function == Function::Count {
                        rounded(&format!(
                            "SUM(CASE WHEN {value} IS NOT NULL THEN {WEIGHT} END)"
                        ))
                    } else {
                        format!("COUNT({value})")
                    },
                    "count",
                )
            }
            (Function::CountDistinct, Some(column)) => {
                for used in std::iter::once(column).chain(fallback.as_ref()) {
                    if used.data_type() == FieldType::Json {
                        return Err(Error::invalid(format!(
                            "`{}` has no values to tell apart",
                            used.name()
                        )));
                    }
                }

                let distinct = format!("COUNT(DISTINCT {})", guard(key(column, fallback.as_ref())));

                // Every row of a session says the share it was kept with, so the sessions that
                // were seen stand for as many as the mean of the rows says. People are not
                // scaled: how many people a share of the sessions saw is not known.
                let sql = if estimate && column.name() == "session.id" {
                    rounded(&format!("{distinct} * AVG({})", guard(WEIGHT.to_string())))
                } else {
                    distinct
                };

                (sql, "count")
            }
            (function, None) => {
                return Err(Error::invalid(format!("{} needs a field", function.name())));
            }
            (function, Some(column)) => {
                if !column.is_numeric() {
                    return Err(Error::invalid(format!(
                        "`{}` is not a number",
                        column.name()
                    )));
                }

                let value = guard(column.number());

                let sql = match function {
                    Function::Sum if estimate => {
                        format!("SUM(CAST({value} AS DOUBLE) * {WEIGHT})")
                    }
                    Function::Sum => format!("SUM({value})"),
                    Function::Avg if estimate => format!(
                        "SUM(CAST({value} AS DOUBLE) * {WEIGHT}) / \
                         SUM(CASE WHEN {value} IS NOT NULL THEN {WEIGHT} END)"
                    ),
                    Function::Avg => format!("AVG(CAST({value} AS DOUBLE))"),
                    Function::Min => format!("MIN({value})"),
                    Function::Max => format!("MAX({value})"),
                    Function::Percentile(share) if estimate => format!(
                        "approx_percentile_cont_with_weight({WEIGHT}, {}) WITHIN GROUP \
                         (ORDER BY CAST({value} AS DOUBLE))",
                        f64::from(share) / 100.0
                    ),
                    Function::Percentile(share) => format!(
                        "approx_percentile_cont({}) WITHIN GROUP (ORDER BY CAST({value} AS DOUBLE))",
                        f64::from(share) / 100.0
                    ),
                    Function::Count | Function::Stored | Function::CountDistinct => {
                        return Err(Error::internal("a count was read as a number"));
                    }
                };

                (sql, column.unit())
            }
        };

        let name = spec
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| match &column {
                Some(column) => format!("{}({})", function.name(), column.name()),
                None => function.name(),
            });

        if name.chars().count() > MAX_NAME_LEN {
            return Err(Error::invalid(format!(
                "a measure name is at most {MAX_NAME_LEN} characters"
            )));
        }

        let columns = column
            .iter()
            .chain(fallback.iter())
            .map(|column| column.stored_name().to_string())
            .chain(
                conditions
                    .iter()
                    .flat_map(|condition| condition.columns.iter().cloned()),
            )
            .chain((estimate && function != Function::Stored).then(|| WEIGHT_COLUMN.to_string()))
            .collect();

        Ok(Self {
            name,
            function,
            field: column.map(|column| column.name().to_string()),
            unit,
            sql,
            columns,
        })
    }
}

/// The column a request names, checked against the stream. No name is no column.
pub(super) fn named(stream: RumStream, name: Option<&str>) -> Result<Option<Column>> {
    name.map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| resolved(stream, name))
        .transpose()
}

/// What tells two rows apart: the column, or the other one where it has no value.
fn key(column: &Column, fallback: Option<&Column>) -> String {
    match fallback {
        Some(fallback) => format!("COALESCE({}, {})", column.text(), fallback.text()),
        None => column.text(),
    }
}

/// The conditions of a measure as one: `None` when it takes every row.
fn taken(conditions: &[Condition]) -> Option<String> {
    (!conditions.is_empty()).then(|| {
        conditions
            .iter()
            .map(|condition| format!("({})", condition.sql))
            .collect::<Vec<_>>()
            .join(" AND ")
    })
}

/// `value` for the rows a measure takes and null for the others, which an aggregate skips.
fn guarded(taken: Option<&str>, value: String) -> String {
    match taken {
        Some(when) => format!("CASE WHEN {when} THEN {value} END"),
        None => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(function: &str, field: Option<&str>) -> MeasureSpec {
        MeasureSpec {
            function: function.into(),
            field: field.map(str::to_string),
            fallback: None,
            filters: Vec::new(),
            name: None,
        }
    }

    fn sql_of(stream: RumStream, spec: &MeasureSpec) -> String {
        Measure::new(stream, spec, false).expect("a measure").sql
    }

    fn estimate_of(stream: RumStream, spec: &MeasureSpec) -> String {
        Measure::new(stream, spec, true).expect("a measure").sql
    }

    fn filter(field: &str, op: &str, value: &str) -> Filter {
        Filter {
            field: field.into(),
            op: op.into(),
            value: value.into(),
            quoted: false,
        }
    }

    #[test]
    fn a_measure_is_a_function_of_a_column_of_the_stream() {
        assert_eq!(
            sql_of(RumStream::Views, &measure("count", None)),
            "COUNT(*)"
        );
        assert_eq!(
            sql_of(
                RumStream::Views,
                &measure("count", Some("view.error.count"))
            ),
            "COUNT(\"view.error.count\")"
        );
        assert_eq!(
            sql_of(RumStream::Views, &measure("sum", Some("view.error.count"))),
            "SUM(\"view.error.count\")"
        );
        assert_eq!(
            sql_of(RumStream::Views, &measure("AVG", Some("view.loading_time"))),
            "AVG(CAST(\"view.loading_time\" AS DOUBLE))"
        );
        assert_eq!(
            sql_of(RumStream::Views, &measure("max", Some("view.loading_time"))),
            "MAX(\"view.loading_time\")"
        );
        assert_eq!(
            sql_of(
                RumStream::Views,
                &measure("p75", Some("view.performance.lcp.timestamp"))
            ),
            "approx_percentile_cont(0.75) WITHIN GROUP \
             (ORDER BY CAST(\"view.performance.lcp.timestamp\" AS DOUBLE))"
        );
        assert_eq!(
            sql_of(RumStream::Views, &measure("p99", Some("view.loading_time"))),
            "approx_percentile_cont(0.99) WITHIN GROUP \
             (ORDER BY CAST(\"view.loading_time\" AS DOUBLE))"
        );
    }

    #[test]
    fn a_measure_may_take_only_some_of_the_rows() {
        let mut errors = measure("count", None);
        errors.filters = vec![filter("view.error.count", ">", "0")];

        assert_eq!(
            sql_of(RumStream::Views, &errors),
            "COUNT(CASE WHEN (\"view.error.count\" > 0) THEN 1 END)"
        );

        let mut slow = measure("p75", Some("view.loading_time"));
        slow.filters = vec![
            filter("env", "=", "prod"),
            filter("view.error.count", "=", "0"),
        ];

        assert_eq!(
            sql_of(RumStream::Views, &slow),
            "approx_percentile_cont(0.75) WITHIN GROUP (ORDER BY CAST(CASE WHEN \
             (\"env\" = 'prod') AND (\"view.error.count\" = 0) THEN \"view.loading_time\" END \
             AS DOUBLE))"
        );

        // What it reads is read for it, whether the statement asks for it itself or not.
        let columns = Measure::new(RumStream::Views, &slow, false)
            .unwrap()
            .columns;

        assert_eq!(columns, ["view.loading_time", "env", "view.error.count"]);
    }

    #[test]
    fn users_are_told_apart_by_an_id_or_else_by_the_anonymous_one() {
        let mut users = measure("count_distinct", Some("usr.id"));
        users.fallback = Some("usr.anonymous_id".into());

        assert_eq!(
            sql_of(RumStream::Sessions, &users),
            "COUNT(DISTINCT COALESCE(NULLIF(CAST(\"usr.id\" AS VARCHAR), ''), \
             NULLIF(CAST(\"usr.anonymous_id\" AS VARCHAR), '')))"
        );
        assert_eq!(
            sql_of(
                RumStream::Sessions,
                &measure("count_distinct", Some("session.id"))
            ),
            "COUNT(DISTINCT NULLIF(CAST(\"session.id\" AS VARCHAR), ''))"
        );
    }

    #[test]
    fn a_measure_that_asks_what_the_stream_cannot_answer_is_refused() {
        let refused = |spec: MeasureSpec| {
            Measure::new(RumStream::Views, &spec, false)
                .expect_err("refused")
                .to_string()
        };

        assert!(refused(measure("median", None)).contains("unsupported measure `median`"));
        assert!(refused(measure("sum", Some("view.nope"))).contains("not a field of rum_views"));
        assert!(refused(measure("sum", None)).contains("sum needs a field"));
        assert!(refused(measure("count_distinct", None)).contains("needs a field"));
        assert!(refused(measure("avg", Some("env"))).contains("not a number"));
        assert!(refused(measure("p95", Some("context"))).contains("not a number"));
        assert!(refused(measure("count_distinct", Some("context"))).contains("no values"));

        let mut fallback = measure("sum", Some("view.error.count"));
        fallback.fallback = Some("usr.id".into());

        assert!(refused(fallback).contains("goes with count_distinct"));

        let mut unknown = measure("count", None);
        unknown.filters = vec![filter("view.nope", "=", "1")];

        assert!(refused(unknown).contains("not a field of rum_views"));

        let mut long = measure("count", None);
        long.name = Some("x".repeat(65));

        assert!(refused(long).contains("at most 64"));
    }

    #[test]
    fn a_measure_is_called_by_its_name_or_else_by_what_it_does() {
        let named = |spec: &MeasureSpec| Measure::new(RumStream::Views, spec, true).unwrap().name;

        assert_eq!(named(&measure("count", None)), "count");
        assert_eq!(
            named(&measure("p75", Some("view.loading_time"))),
            "p75(view.loading_time)"
        );

        let mut lcp = measure("p75", Some("view.performance.lcp.timestamp"));
        lcp.name = Some(" lcp ".into());

        assert_eq!(named(&lcp), "lcp");
    }

    #[test]
    fn an_estimate_counts_each_row_for_as_many_as_the_share_it_was_kept_with_says() {
        let weighted = |inner: String| format!("CAST(COALESCE(ROUND({inner}), 0) AS BIGINT)");

        assert_eq!(
            estimate_of(RumStream::Views, &measure("count", None)),
            weighted(format!("SUM({WEIGHT})"))
        );
        assert_eq!(
            estimate_of(RumStream::Views, &measure("count", Some("view.first_byte"))),
            weighted(format!(
                "SUM(CASE WHEN \"view.first_byte\" IS NOT NULL THEN {WEIGHT} END)"
            ))
        );

        let mut errors = measure("count", None);
        errors.filters = vec![filter("view.error.count", ">", "0")];

        assert_eq!(
            estimate_of(RumStream::Views, &errors),
            weighted(format!(
                "SUM(CASE WHEN (\"view.error.count\" > 0) THEN {WEIGHT} END)"
            ))
        );

        assert_eq!(
            estimate_of(RumStream::Views, &measure("sum", Some("view.error.count"))),
            format!("SUM(CAST(\"view.error.count\" AS DOUBLE) * {WEIGHT})")
        );
        assert_eq!(
            estimate_of(RumStream::Views, &measure("avg", Some("view.loading_time"))),
            format!(
                "SUM(CAST(\"view.loading_time\" AS DOUBLE) * {WEIGHT}) / \
                 SUM(CASE WHEN \"view.loading_time\" IS NOT NULL THEN {WEIGHT} END)"
            )
        );
        assert_eq!(
            estimate_of(RumStream::Views, &measure("p75", Some("view.loading_time"))),
            format!(
                "approx_percentile_cont_with_weight({WEIGHT}, 0.75) WITHIN GROUP \
                 (ORDER BY CAST(\"view.loading_time\" AS DOUBLE))"
            )
        );

        // A value that was seen is not made larger by the share it was kept with.
        for function in ["min", "max"] {
            assert_eq!(
                estimate_of(
                    RumStream::Views,
                    &measure(function, Some("view.loading_time"))
                ),
                sql_of(
                    RumStream::Views,
                    &measure(function, Some("view.loading_time"))
                )
            );
        }
    }

    #[test]
    fn the_sessions_that_were_seen_stand_for_the_mean_share_of_their_rows_and_people_for_themselves()
     {
        let mut errored = measure("count_distinct", Some("session.id"));
        errored.filters = vec![filter("view.error.count", ">", "0")];

        let sql = estimate_of(RumStream::Views, &errored);

        assert!(
            sql.starts_with("CAST(COALESCE(ROUND(COUNT(DISTINCT CASE WHEN"),
            "{sql}"
        );
        assert!(sql.contains(&format!(
            ") * AVG(CASE WHEN (\"view.error.count\" > 0) THEN {WEIGHT} END)"
        )));

        let mut people = measure("count_distinct", Some("usr.id"));
        people.fallback = Some("usr.anonymous_id".into());

        assert_eq!(
            estimate_of(RumStream::Sessions, &people),
            sql_of(RumStream::Sessions, &people)
        );
    }

    #[test]
    fn the_rows_that_are_stored_are_counted_as_they_are_whatever_is_asked() {
        assert_eq!(
            estimate_of(RumStream::Views, &measure("stored", None)),
            "COUNT(*)"
        );
        assert_eq!(
            estimate_of(
                RumStream::Views,
                &measure("stored", Some("view.first_byte"))
            ),
            "COUNT(\"view.first_byte\")"
        );

        let mut errors = measure("stored", None);
        errors.filters = vec![filter("view.error.count", ">", "0")];

        assert_eq!(
            estimate_of(RumStream::Views, &errors),
            "COUNT(CASE WHEN (\"view.error.count\" > 0) THEN 1 END)"
        );

        // It reads no share, so a stream that has none is not asked for one.
        let columns = Measure::new(RumStream::Views, &measure("stored", None), true)
            .unwrap()
            .columns;

        assert!(columns.is_empty());
    }

    #[test]
    fn an_estimate_reads_the_column_that_says_the_share() {
        let columns = |estimate: bool| {
            Measure::new(RumStream::Views, &measure("count", None), estimate)
                .unwrap()
                .columns
        };

        assert_eq!(columns(true), [WEIGHT_COLUMN]);
        assert!(columns(false).is_empty());
    }
}
