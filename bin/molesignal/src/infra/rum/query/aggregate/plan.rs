// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A request that has been checked, and the statements it stands for.

use std::collections::HashSet;

use serde_json::Value;

use super::{
    super::{
        histogram::{BucketPlan, MAX_BUCKETS, Unavailable},
        member::Column,
        sql::{Relation, Scope, literal},
    },
    answer::{Aggregate, MeasureInfo, Row, group_of},
    measure::{Measure, MeasureSpec, named},
};
use crate::{
    domain::{rum::event::RumStream, stream::FieldType},
    shared::{Error, Result},
};

/// Most measures one request asks for.
const MAX_MEASURES: usize = 16;
/// Most columns a request groups by.
const MAX_GROUPS: usize = 2;
/// How many groups an answer holds unless the request says.
const DEFAULT_LIMIT: usize = 10;
/// Most groups an answer holds: enough for every country.
const MAX_LIMIT: usize = 250;
/// Most groups a series over time follows.
const MAX_SERIES_GROUPS: usize = 20;
/// Most rows a series over time can have: every followed group in every bar.
const MAX_SERIES_ROWS: usize = MAX_SERIES_GROUPS * MAX_BUCKETS as usize;
const GROUP_PREFIX: &str = "__g";
const MEASURE_PREFIX: &str = "__m";
const BAR_COLUMN: &str = "__bar";

/// Everything a request says about the numbers it wants.
#[derive(Debug, Clone, Copy)]
pub struct Spec<'a> {
    pub measures: &'a [MeasureSpec],
    pub group_by: &'a [String],
    /// Bars of time: the answer has a row per group per bar.
    pub bars: Option<BucketPlan>,
    /// A measure, a group or `time` to order by.
    pub order_by: Option<&'a str>,
    pub descending: Option<bool>,
    /// How many groups to answer with.
    pub limit: Option<usize>,
}

fn direction(descending: bool) -> &'static str {
    if descending { "DESC" } else { "ASC" }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order {
    Measure { index: usize, descending: bool },
    Group { index: usize, descending: bool },
    Bar { descending: bool },
}

impl Order {
    fn parse(
        name: Option<&str>,
        descending: Option<bool>,
        groups: &[Column],
        measures: &[Measure],
        bars: bool,
    ) -> Result<Self> {
        let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
            return Ok(if bars && groups.is_empty() {
                Self::Bar {
                    descending: descending.unwrap_or(false),
                }
            } else {
                Self::Measure {
                    index: 0,
                    descending: descending.unwrap_or(true),
                }
            });
        };

        if name == "time" {
            if !bars {
                return Err(Error::invalid(
                    "`time` orders bars, and no bucket_micros was given",
                ));
            }

            // The groups of a series are the ones that rank first by the first measure; time
            // orders what is listed of them.
            return Ok(if groups.is_empty() {
                Self::Bar {
                    descending: descending.unwrap_or(false),
                }
            } else {
                Self::Measure {
                    index: 0,
                    descending: true,
                }
            });
        }

        if let Some(index) = measures.iter().position(|measure| measure.name == name) {
            return Ok(Self::Measure {
                index,
                descending: descending.unwrap_or(true),
            });
        }

        if let Some(index) = groups.iter().position(|group| group.name() == name) {
            return Ok(Self::Group {
                index,
                descending: descending.unwrap_or(false),
            });
        }

        Err(Error::invalid(format!(
            "`{name}` is neither a measure nor a group of this request"
        )))
    }
}

/// A request that has been checked, and the statements it stands for.
///
/// Without bars, one statement answers: a row per group, or a single row when nothing is grouped.
/// With bars and no groups, one statement answers a row per bar. With both, the groups that rank
/// first are found by one statement and followed through the bars by a second.
#[derive(Debug)]
pub struct Plan {
    pub(super) measures: Vec<Measure>,
    pub(super) groups: Vec<Column>,
    bars: Option<BucketPlan>,
    pub(super) order: Order,
    pub(super) limit: usize,
}

impl Plan {
    /// The plan of a request: its counts are estimates of what there would be had every session
    /// been kept, by the share each row was kept with.
    pub fn new(stream: RumStream, spec: &Spec<'_>) -> Result<Self> {
        Self::planned(stream, spec, true)
    }

    /// The plan of a request that counts the rows that are stored, as they are.
    pub fn stored(stream: RumStream, spec: &Spec<'_>) -> Result<Self> {
        Self::planned(stream, spec, false)
    }

    fn planned(stream: RumStream, spec: &Spec<'_>, estimate: bool) -> Result<Self> {
        if spec.measures.is_empty() {
            return Err(Error::invalid("at least one measure is required"));
        }

        if spec.measures.len() > MAX_MEASURES {
            return Err(Error::invalid(format!(
                "at most {MAX_MEASURES} measures are allowed"
            )));
        }

        if spec.group_by.len() > MAX_GROUPS {
            return Err(Error::invalid(format!(
                "group_by takes at most {MAX_GROUPS} fields"
            )));
        }

        let measures = spec
            .measures
            .iter()
            .map(|measure| Measure::new(stream, measure, estimate))
            .collect::<Result<Vec<_>>>()?;

        let mut names = HashSet::new();

        for measure in &measures {
            if !names.insert(measure.name.as_str()) {
                return Err(Error::invalid(format!(
                    "two measures are called `{}`: give one of them a name",
                    measure.name
                )));
            }
        }

        let mut groups = Vec::with_capacity(spec.group_by.len());

        for name in spec.group_by {
            let Some(column) = named(stream, Some(name.as_str()))? else {
                return Err(Error::invalid("a group_by field is empty"));
            };

            if column.data_type() == FieldType::Json {
                return Err(Error::invalid(format!(
                    "`{}` has no values to group by",
                    column.name()
                )));
            }

            if groups
                .iter()
                .any(|group: &Column| group.name() == column.name())
            {
                return Err(Error::invalid(format!(
                    "`{}` is grouped by twice",
                    column.name()
                )));
            }

            groups.push(column);
        }

        let order = Order::parse(
            spec.order_by,
            spec.descending,
            &groups,
            &measures,
            spec.bars.is_some(),
        )?;

        let cap = if spec.bars.is_some() {
            MAX_SERIES_GROUPS
        } else {
            MAX_LIMIT
        };

        Ok(Self {
            measures,
            groups,
            bars: spec.bars,
            order,
            limit: spec.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, cap),
        })
    }

    /// Whether a second statement follows the first: the groups are found, then followed.
    pub fn needs_series(&self) -> bool {
        self.bars.is_some() && !self.groups.is_empty()
    }

    /// The statement to run first.
    pub fn first_sql(&self, scope: &Scope<'_>) -> String {
        match &self.bars {
            Some(bars) if self.groups.is_empty() => {
                self.build(scope, Some(bars), &[], &self.series_tail())
            }
            _ => self.build(scope, None, &[], &self.ranked_tail()),
        }
    }

    /// The statement that follows `groups` through the bars.
    pub fn series_sql(&self, scope: &Scope<'_>, groups: &[Vec<Option<String>>]) -> String {
        self.build(scope, self.bars.as_ref(), groups, &self.series_tail())
    }

    /// The groups the first statement of a series found, the first of them ranking first.
    pub fn groups_of(
        &self,
        columns: &[String],
        rows: &[Vec<Value>],
    ) -> Result<Vec<Vec<Option<String>>>> {
        // A statement that found no rows says nothing of its columns: there are no groups.
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let positions = self.group_positions(columns)?;

        Ok(rows.iter().map(|row| group_of(&positions, row)).collect())
    }

    /// What the rows of a statement of this plan came to.
    pub fn answer(&self, columns: &[String], rows: &[Vec<Value>]) -> Result<Aggregate> {
        // The same: a result with no rows has no columns to look up, and answers no rows.
        if rows.is_empty() {
            return Ok(self.empty());
        }

        let position = |name: String| {
            columns
                .iter()
                .position(|column| *column == name)
                .ok_or_else(|| Error::internal(format!("aggregate statement returned no `{name}`")))
        };

        let groups = self.group_positions(columns)?;

        let measures = (0..self.measures.len())
            .map(|index| position(format!("{MEASURE_PREFIX}{index}")))
            .collect::<Result<Vec<_>>>()?;

        let bar = self
            .bars
            .as_ref()
            .map(|_| position(BAR_COLUMN.to_string()))
            .transpose()?;

        let mut answered = Vec::with_capacity(rows.len());

        for row in rows {
            let start_micros = match (bar, &self.bars) {
                (Some(bar), Some(bars)) => {
                    // A masked value comes back as text: there are no bars over a masked time.
                    let Some(index) = row.get(bar).and_then(Value::as_i64) else {
                        return Ok(self.unavailable(Unavailable::TimestampMasked));
                    };

                    // A bar outside the range was not asked for.
                    let Some(start) = bars.start_of(index) else {
                        continue;
                    };

                    Some(start)
                }
                _ => None,
            };

            answered.push(Row {
                group: group_of(&groups, row),
                start_time: start_micros,
                values: measures
                    .iter()
                    .map(|at| row.get(*at).cloned().unwrap_or(Value::Null))
                    .collect(),
            });
        }

        Ok(self.shell(answered, None))
    }

    /// The answer for a stream nothing was ever written to: no rows, and nothing missing.
    pub fn empty(&self) -> Aggregate {
        self.shell(Vec::new(), None)
    }

    pub fn unavailable(&self, reason: Unavailable) -> Aggregate {
        self.shell(Vec::new(), Some(reason))
    }

    fn shell(&self, rows: Vec<Row>, unavailable: Option<Unavailable>) -> Aggregate {
        Aggregate {
            group_by: self
                .groups
                .iter()
                .map(|group| group.name().to_string())
                .collect(),
            measures: self
                .measures
                .iter()
                .map(|measure| MeasureInfo {
                    name: measure.name.clone(),
                    function: measure.function.name(),
                    field: measure.field.clone(),
                    unit: measure.unit,
                })
                .collect(),
            bucket_time: self.bars.map(|bars| bars.bucket_micros),
            origin_time: self.bars.map(|bars| bars.origin_micros),
            rows,
            unavailable,
        }
    }

    fn group_positions(&self, columns: &[String]) -> Result<Vec<usize>> {
        (0..self.groups.len())
            .map(|index| {
                let name = format!("{GROUP_PREFIX}{index}");

                columns
                    .iter()
                    .position(|column| *column == name)
                    .ok_or_else(|| {
                        Error::internal(format!("aggregate statement returned no `{name}`"))
                    })
            })
            .collect()
    }

    /// The columns a statement has to read.
    fn needed(&self) -> Vec<&str> {
        self.groups
            .iter()
            .map(|group| -> &str { group.stored_name() })
            .chain(
                self.measures
                    .iter()
                    .flat_map(|measure| measure.columns.iter().map(String::as_str)),
            )
            .collect()
    }

    fn build(
        &self,
        scope: &Scope<'_>,
        bars: Option<&BucketPlan>,
        only: &[Vec<Option<String>>],
        tail: &str,
    ) -> String {
        let relation = Relation::of(scope, &self.needed());

        let mut select = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| format!("{} AS \"{GROUP_PREFIX}{index}\"", group.text()))
            .collect::<Vec<_>>();

        if let Some(bars) = bars {
            select.push(format!(
                "({} - {}) / {} AS \"{BAR_COLUMN}\"",
                relation.ts, bars.origin_micros, bars.bucket_micros
            ));
        }

        select.extend(
            self.measures
                .iter()
                .enumerate()
                .map(|(index, measure)| format!("{} AS \"{MEASURE_PREFIX}{index}\"", measure.sql)),
        );

        let keys = self.groups.len() + usize::from(bars.is_some());

        let grouping = if keys == 0 {
            String::new()
        } else {
            let ordinals = (1..=keys)
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
                .join(", ");

            format!(" GROUP BY {ordinals}")
        };

        let extra = if only.is_empty() {
            Vec::new()
        } else {
            vec![self.restriction(only)]
        };

        format!(
            "{}SELECT {} FROM {} WHERE {}{grouping}{tail}",
            relation.with,
            select.join(", "),
            relation.from,
            relation.where_clause(&extra),
        )
    }

    /// Rows of the groups listed, a group having no value where its column has none.
    fn restriction(&self, groups: &[Vec<Option<String>>]) -> String {
        let any = groups
            .iter()
            .map(|values| {
                let all = self
                    .groups
                    .iter()
                    .zip(values)
                    .map(|(group, value)| match value {
                        Some(value) => format!("{} = {}", group.text(), literal(value)),
                        None => format!("{} IS NULL", group.text()),
                    })
                    .collect::<Vec<_>>()
                    .join(" AND ");

                format!("({all})")
            })
            .collect::<Vec<_>>()
            .join(" OR ");

        format!("({any})")
    }

    /// The order and the length of a statement that answers a row per group.
    fn ranked_tail(&self) -> String {
        if self.groups.is_empty() {
            return String::new();
        }

        let first = match self.order {
            Order::Measure { index, descending } => Some(format!(
                "\"{MEASURE_PREFIX}{index}\" {} NULLS LAST",
                direction(descending)
            )),
            Order::Group { index, descending } => Some(format!(
                "\"{GROUP_PREFIX}{index}\" {} NULLS LAST",
                direction(descending)
            )),
            Order::Bar { .. } => None,
        };

        let mut keys = first.into_iter().collect::<Vec<_>>();

        // Groups that tie come in the order of their values, so an answer is the same each time.
        for index in 0..self.groups.len() {
            let key = format!("\"{GROUP_PREFIX}{index}\" ASC NULLS LAST");

            if !keys.contains(&key) {
                keys.push(key);
            }
        }

        format!(" ORDER BY {} LIMIT {}", keys.join(", "), self.limit)
    }

    /// The order and the length of a statement that answers a row per bar and group.
    fn series_tail(&self) -> String {
        let descending = matches!(self.order, Order::Bar { descending: true });

        let groups = (0..self.groups.len())
            .map(|index| format!(", \"{GROUP_PREFIX}{index}\" ASC NULLS LAST"))
            .collect::<String>();

        format!(
            " ORDER BY \"{BAR_COLUMN}\" {} NULLS LAST{groups} LIMIT {MAX_SERIES_ROWS}",
            direction(descending)
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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

    fn plan(stream: RumStream, measures: &[MeasureSpec], group_by: &[&str]) -> Result<Plan> {
        let group_by = group_by
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();

        Plan::stored(
            stream,
            &Spec {
                measures,
                group_by: &group_by,
                bars: None,
                order_by: None,
                descending: None,
                limit: None,
            },
        )
    }

    fn scope(stream: RumStream) -> Scope<'static> {
        Scope {
            stream,
            from: 100,
            to: 200,
            conditions: &[],
        }
    }

    #[test]
    fn a_plan_is_checked_before_it_becomes_a_statement() {
        let count = [measure("count", None)];
        let refused = |result: Result<Plan>| result.expect_err("refused").to_string();

        assert!(refused(plan(RumStream::Views, &[], &[])).contains("at least one measure"));
        assert!(
            refused(plan(
                RumStream::Views,
                &vec![measure("count", None); 17],
                &[]
            ))
            .contains("at most 16")
        );
        assert!(
            refused(plan(
                RumStream::Views,
                &count,
                &["env", "version", "browser.name"]
            ))
            .contains("at most 2")
        );
        assert!(refused(plan(RumStream::Views, &count, &["view.nope"])).contains("not a field"));
        assert!(refused(plan(RumStream::Views, &count, &["context"])).contains("no values"));
        assert!(refused(plan(RumStream::Views, &count, &["env", "env"])).contains("twice"));
        assert!(
            refused(plan(
                RumStream::Views,
                &[measure("count", None), measure("count", None)],
                &[]
            ))
            .contains("two measures are called `count`")
        );
    }

    #[test]
    fn groups_are_ranked_by_the_first_measure_unless_told_otherwise() {
        let measures = [
            measure("count", None),
            measure("p75", Some("view.performance.lcp.timestamp")),
        ];

        let ranked = plan(RumStream::Views, &measures, &["view.url_path_group"]).unwrap();
        let sql = ranked.first_sql(&scope(RumStream::Views));

        assert!(sql.starts_with("WITH \"latest\" AS (SELECT "));
        assert!(sql.contains(
            "SELECT NULLIF(CAST(\"view.url_path_group\" AS VARCHAR), '') AS \"__g0\", \
             COUNT(*) AS \"__m0\", approx_percentile_cont(0.75) WITHIN GROUP \
             (ORDER BY CAST(\"view.performance.lcp.timestamp\" AS DOUBLE)) AS \"__m1\" \
             FROM \"latest\" WHERE \"__rn\" = 1 GROUP BY 1 \
             ORDER BY \"__m0\" DESC NULLS LAST, \"__g0\" ASC NULLS LAST LIMIT 10"
        ));
        assert!(!ranked.needs_series());

        let by_value = Plan::stored(
            RumStream::Views,
            &Spec {
                measures: &measures,
                group_by: &["view.url_path_group".to_string()],
                bars: None,
                order_by: Some("p75(view.performance.lcp.timestamp)"),
                descending: Some(false),
                limit: Some(1_000),
            },
        )
        .unwrap();

        let sql = by_value.first_sql(&scope(RumStream::Views));

        assert!(
            sql.contains("ORDER BY \"__m1\" ASC NULLS LAST, \"__g0\" ASC NULLS LAST LIMIT 250")
        );

        let by_group = Plan::stored(
            RumStream::Views,
            &Spec {
                measures: &measures,
                group_by: &["env".to_string(), "version".to_string()],
                bars: None,
                order_by: Some("version"),
                descending: None,
                limit: Some(0),
            },
        )
        .unwrap();

        let sql = by_group.first_sql(&scope(RumStream::Views));

        assert!(sql.contains(
            "GROUP BY 1, 2 ORDER BY \"__g1\" ASC NULLS LAST, \"__g0\" ASC NULLS LAST LIMIT 1"
        ));
    }

    #[test]
    fn nothing_grouped_is_one_row_over_every_row() {
        let totals = plan(RumStream::Errors, &[measure("count", None)], &[]).unwrap();

        assert_eq!(
            totals.first_sql(&scope(RumStream::Errors)),
            "SELECT COUNT(*) AS \"__m0\" FROM \"rum_errors\" WHERE \
             CAST(_timestamp AS BIGINT) >= 100 AND CAST(_timestamp AS BIGINT) < 200"
        );
    }

    #[test]
    fn an_estimate_counts_each_row_for_the_share_it_was_kept_with() {
        let measures = [measure("count", None)];
        let spec = Spec {
            measures: &measures,
            group_by: &[],
            bars: None,
            order_by: None,
            descending: None,
            limit: None,
        };

        let estimated = Plan::new(RumStream::Errors, &spec).unwrap();
        let stored = Plan::stored(RumStream::Errors, &spec).unwrap();

        // A row that says nothing of its share counts for one; none at all counts for nought.
        assert_eq!(
            estimated.first_sql(&scope(RumStream::Errors)),
            "SELECT CAST(COALESCE(ROUND(SUM(COALESCE(100.0 / NULLIF(\"retention.rate\", 0.0), 1.0))), 0) \
             AS BIGINT) AS \"__m0\" FROM \"rum_errors\" WHERE \
             CAST(_timestamp AS BIGINT) >= 100 AND CAST(_timestamp AS BIGINT) < 200"
        );
        assert_eq!(
            stored.first_sql(&scope(RumStream::Errors)),
            "SELECT COUNT(*) AS \"__m0\" FROM \"rum_errors\" WHERE \
             CAST(_timestamp AS BIGINT) >= 100 AND CAST(_timestamp AS BIGINT) < 200"
        );
    }

    #[test]
    fn an_order_has_to_be_of_something_the_request_has() {
        let count = [measure("count", None)];
        let ordered = |by: &str, bars: bool| {
            Plan::stored(
                RumStream::Views,
                &Spec {
                    measures: &count,
                    group_by: &[],
                    bars: bars.then(|| {
                        BucketPlan::new(Some(0), Some(10_000), Some(1_000), None).unwrap()
                    }),
                    order_by: Some(by),
                    descending: None,
                    limit: None,
                },
            )
        };

        assert!(ordered("count", false).is_ok());
        assert!(ordered("time", true).is_ok());
        assert!(
            ordered("time", false)
                .unwrap_err()
                .to_string()
                .contains("no bucket_micros")
        );
        assert!(
            ordered("env", false)
                .unwrap_err()
                .to_string()
                .contains("neither a measure")
        );
    }

    #[test]
    fn a_series_follows_the_groups_it_was_given_through_the_bars() {
        let bars = BucketPlan::new(Some(0), Some(10_000), Some(1_000), None).unwrap();
        let measures = [measure("count", None)];

        let series = Plan::stored(
            RumStream::Errors,
            &Spec {
                measures: &measures,
                group_by: &["browser.name".to_string()],
                bars: Some(bars),
                order_by: Some("time"),
                descending: None,
                limit: Some(5),
            },
        )
        .unwrap();

        assert!(series.needs_series());

        let scope = Scope {
            stream: RumStream::Errors,
            from: 0,
            to: 10_000,
            conditions: &[],
        };

        // The groups come first, ranked by the first measure.
        assert!(series.first_sql(&scope).ends_with(
            "GROUP BY 1 ORDER BY \"__m0\" DESC NULLS LAST, \"__g0\" ASC NULLS LAST LIMIT 5"
        ));

        let sql = series.series_sql(
            &scope,
            &[
                vec![Some("Chrome".into())],
                vec![None],
                vec![Some("it's".into())],
            ],
        );

        assert_eq!(
            sql,
            "SELECT NULLIF(CAST(\"browser.name\" AS VARCHAR), '') AS \"__g0\", \
             (CAST(_timestamp AS BIGINT) - 0) / 1000 AS \"__bar\", COUNT(*) AS \"__m0\" \
             FROM \"rum_errors\" WHERE CAST(_timestamp AS BIGINT) >= 0 AND \
             CAST(_timestamp AS BIGINT) < 10000 AND \
             ((NULLIF(CAST(\"browser.name\" AS VARCHAR), '') = 'Chrome') OR \
             (NULLIF(CAST(\"browser.name\" AS VARCHAR), '') IS NULL) OR \
             (NULLIF(CAST(\"browser.name\" AS VARCHAR), '') = 'it''s')) \
             GROUP BY 1, 2 ORDER BY \"__bar\" ASC NULLS LAST, \"__g0\" ASC NULLS LAST \
             LIMIT 10240"
        );
    }

    #[test]
    fn bars_without_groups_are_one_statement() {
        let bars = BucketPlan::new(Some(0), Some(10_000), Some(1_000), None).unwrap();
        let measures = [measure("count", None)];

        let plain = Plan::stored(
            RumStream::Views,
            &Spec {
                measures: &measures,
                group_by: &[],
                bars: Some(bars),
                order_by: None,
                descending: Some(true),
                limit: None,
            },
        )
        .unwrap();

        assert!(!plain.needs_series());

        let sql = plain.first_sql(&scope(RumStream::Views));

        assert!(
            sql.contains(
                "(\"__ts\" - 0) / 1000 AS \"__bar\", COUNT(*) AS \"__m0\" FROM \"latest\""
            )
        );
        assert!(sql.ends_with("GROUP BY 1 ORDER BY \"__bar\" DESC NULLS LAST LIMIT 10240"));
    }

    fn columns(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn the_rows_of_a_statement_are_an_answer() {
        let measures = [
            measure("count", None),
            measure("p75", Some("view.performance.lcp.timestamp")),
        ];
        let ranked = plan(RumStream::Views, &measures, &["view.url_path_group"]).unwrap();

        let answer = ranked
            .answer(
                &columns(&["__m1", "__g0", "__m0"]),
                &[
                    vec![json!(1.9e9), json!("/"), json!(7)],
                    vec![Value::Null, Value::Null, json!(2)],
                ],
            )
            .unwrap();

        assert_eq!(
            serde_json::to_value(&answer).unwrap(),
            json!({
                "groupBy": ["view.url_path_group"],
                "measures": [
                    { "name": "count", "fn": "count", "unit": "count" },
                    {
                        "name": "p75(view.performance.lcp.timestamp)",
                        "fn": "p75",
                        "field": "view.performance.lcp.timestamp",
                        "unit": "ns",
                    },
                ],
                "rows": [
                    { "group": ["/"], "values": [7, 1.9e9] },
                    { "group": [null], "values": [2, null] },
                ],
            })
        );
    }

    #[test]
    fn the_rows_of_a_series_are_named_by_where_their_bar_begins() {
        let bars = BucketPlan::new(Some(10_000), Some(14_000), Some(1_000), Some(10_000)).unwrap();
        let measures = [measure("count", None)];

        let series = Plan::stored(
            RumStream::Errors,
            &Spec {
                measures: &measures,
                group_by: &[],
                bars: Some(bars),
                order_by: None,
                descending: None,
                limit: None,
            },
        )
        .unwrap();

        let answer = series
            .answer(
                &columns(&["__bar", "__m0"]),
                &[
                    vec![json!(0), json!(4)],
                    vec![json!(2), json!(1)],
                    // Past the range: not asked for.
                    vec![json!(9), json!(8)],
                ],
            )
            .unwrap();

        assert_eq!(answer.bucket_time, Some(1_000));
        assert_eq!(answer.origin_time, Some(10_000));
        assert_eq!(
            answer
                .rows
                .iter()
                .map(|row| (row.start_time, row.values[0].clone()))
                .collect::<Vec<_>>(),
            [(Some(10_000), json!(4)), (Some(12_000), json!(1))]
        );

        let masked = series
            .answer(
                &columns(&["__bar", "__m0"]),
                &[vec![json!("***"), json!(4)]],
            )
            .unwrap();

        assert_eq!(masked.unavailable, Some(Unavailable::TimestampMasked));
        assert!(masked.rows.is_empty());
    }

    #[test]
    fn the_groups_of_a_series_are_read_from_the_first_statement() {
        let bars = BucketPlan::new(Some(0), Some(10_000), Some(1_000), None).unwrap();
        let measures = [measure("count", None)];

        let series = Plan::stored(
            RumStream::Errors,
            &Spec {
                measures: &measures,
                group_by: &["browser.name".to_string(), "os.name".to_string()],
                bars: Some(bars),
                order_by: None,
                descending: None,
                limit: None,
            },
        )
        .unwrap();

        let groups = series
            .groups_of(
                &columns(&["__g0", "__g1", "__m0"]),
                &[
                    vec![json!("Chrome"), json!("macOS"), json!(9)],
                    vec![json!("Safari"), Value::Null, json!(3)],
                ],
            )
            .unwrap();

        assert_eq!(
            groups,
            [
                vec![Some("Chrome".to_string()), Some("macOS".to_string())],
                vec![Some("Safari".to_string()), None],
            ]
        );
        // Rows have to come with their columns; a statement that found none says nothing of them.
        assert!(
            series
                .groups_of(&columns(&["__m0"]), &[vec![json!(3)]])
                .is_err()
        );
        assert!(series.groups_of(&[], &[]).unwrap().is_empty());
    }

    #[test]
    fn an_answer_without_rows_says_why() {
        let totals = plan(RumStream::Errors, &[measure("count", None)], &[]).unwrap();

        assert!(totals.empty().rows.is_empty());
        assert_eq!(totals.empty().unavailable, None);
        assert_eq!(
            serde_json::to_value(totals.unavailable(Unavailable::TimeBudget)).unwrap(),
            json!({
                "groupBy": [],
                "measures": [{ "name": "count", "fn": "count", "unit": "count" }],
                "rows": [],
                "unavailable": "time_budget",
            })
        );
        assert!(
            totals
                .answer(&columns(&["other"]), &[vec![json!(1)]])
                .is_err()
        );
    }

    #[test]
    fn a_grouped_statement_that_found_no_rows_answers_no_rows() {
        // DataFusion gives no batches for it, so the result has no columns at all: grouped, in
        // bars, or both, the answer is an empty one and not an error.
        let measures = [measure("count", None)];
        let bars = BucketPlan::new(Some(0), Some(10_000), Some(1_000), None).unwrap();
        let spec = |group_by: &[String], bars: Option<BucketPlan>| {
            Plan::stored(
                RumStream::Views,
                &Spec {
                    measures: &measures,
                    group_by,
                    bars,
                    order_by: None,
                    descending: None,
                    limit: None,
                },
            )
            .unwrap()
        };

        let grouped = spec(&["view.url_path_group".to_string()], None);
        let answer = grouped.answer(&[], &[]).unwrap();

        assert!(answer.rows.is_empty());
        assert_eq!(answer.group_by, ["view.url_path_group"]);
        assert_eq!(answer.unavailable, None);
        assert!(grouped.groups_of(&[], &[]).unwrap().is_empty());

        let barred = spec(&[], Some(bars));

        assert!(barred.answer(&[], &[]).unwrap().rows.is_empty());
        assert!(!barred.needs_series());

        let both = spec(&["env".to_string()], Some(bars));

        assert!(both.needs_series());
        assert!(both.groups_of(&[], &[]).unwrap().is_empty());
        assert!(both.answer(&[], &[]).unwrap().rows.is_empty());
    }
}
