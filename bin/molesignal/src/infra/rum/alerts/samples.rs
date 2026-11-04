// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The sessions an incident is told by: a few that show the figure at its worst, so whoever picks
//! the incident up can open a replay of what it was like.

use std::collections::HashSet;

use serde_json::Value;

use super::groups::calls;
use crate::{
    domain::{
        query::QueryResult,
        rum::{RumAlertMetric, RumAlertSample, event::RumStream},
    },
    infra::rum::query::{
        Condition, ListRequest, Scope, TS_COLUMN, conditions as filter_conditions, list_sql,
        ranked_sql,
    },
    shared::{Result, time::TimestampMicros},
};

/// How many sessions an incident is told by.
pub(super) const SAMPLES: usize = 3;

/// How many rows are read to find them: several rows may belong to one session.
const CANDIDATES: usize = 12;

/// The longest detail kept, in characters.
const MAX_DETAIL: usize = 160;

const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;

/// What to read to find the sessions, and how to read them.
#[derive(Debug)]
pub(super) struct Statement {
    pub(super) stream: RumStream,
    pub(super) sql: String,
    /// The column that holds the figure of a row and what divides it into the unit of the metric.
    value: Option<(&'static str, f64)>,
    detail: Detail,
}

#[derive(Debug, Clone, Copy)]
enum Detail {
    None,
    /// A column of text: the element, the message.
    Column(&'static str),
    /// The request: its method and path.
    Request,
}

/// The statement that finds the sessions of a figure at its worst; none for a figure that is not
/// about sessions. `issues` are the issues the new or regressed ones are told by.
pub(super) fn statement(
    metric: RumAlertMetric,
    scope: &[Condition],
    (from, to): (i64, i64),
    issues: &[String],
) -> Result<Option<Statement>> {
    let latest = |stream: RumStream, extra: Vec<Condition>, columns: &[&str]| {
        let mut conditions = scope.to_vec();

        conditions.extend(extra);

        list_sql(&ListRequest {
            scope: Scope {
                stream,
                from,
                to,
                conditions: &conditions,
            },
            columns,
            limit: CANDIDATES,
            seek: None,
            oldest_first: false,
        })
    };

    let worst = |rank_by: &'static str, columns: &[&str]| {
        ranked_sql(
            &Scope {
                stream: RumStream::Views,
                from,
                to,
                conditions: scope,
            },
            columns,
            rank_by,
            CANDIDATES,
        )
    };

    let statement = match metric {
        RumAlertMetric::LcpP75 => Statement {
            stream: RumStream::Views,
            sql: worst(
                "view.performance.lcp.timestamp",
                &[
                    "session.id",
                    "view.performance.lcp.timestamp",
                    "view.performance.lcp.target_selector",
                ],
            ),
            value: Some((
                "view.performance.lcp.timestamp",
                NANOSECONDS_PER_MILLISECOND,
            )),
            detail: Detail::Column("view.performance.lcp.target_selector"),
        },
        RumAlertMetric::InpP75 => Statement {
            stream: RumStream::Views,
            sql: worst(
                "view.performance.inp.duration",
                &[
                    "session.id",
                    "view.performance.inp.duration",
                    "view.performance.inp.target_selector",
                ],
            ),
            value: Some(("view.performance.inp.duration", NANOSECONDS_PER_MILLISECOND)),
            detail: Detail::Column("view.performance.inp.target_selector"),
        },
        RumAlertMetric::ClsP75 => Statement {
            stream: RumStream::Views,
            sql: worst(
                "view.performance.cls.score",
                &[
                    "session.id",
                    "view.performance.cls.score",
                    "view.performance.cls.target_selector",
                ],
            ),
            value: Some(("view.performance.cls.score", 1.0)),
            detail: Detail::Column("view.performance.cls.target_selector"),
        },
        RumAlertMetric::TtfbP75 => Statement {
            stream: RumStream::Views,
            sql: worst("view.first_byte", &["session.id", "view.first_byte"]),
            value: Some(("view.first_byte", NANOSECONDS_PER_MILLISECOND)),
            detail: Detail::None,
        },
        // The errors themselves tell more than the views that met them.
        RumAlertMetric::ErrorRate | RumAlertMetric::ErrorCount => Statement {
            stream: RumStream::Errors,
            sql: latest(
                RumStream::Errors,
                Vec::new(),
                &["session.id", "error.message"],
            ),
            value: None,
            detail: Detail::Column("error.message"),
        },
        RumAlertMetric::NewIssues
        | RumAlertMetric::RegressedIssues
        | RumAlertMetric::IssueImpact => {
            if issues.is_empty() {
                return Ok(None);
            }

            let of_issues = filter_conditions(
                RumStream::Errors,
                &[crate::infra::rum::query::Filter {
                    field: "error.issue_id".into(),
                    op: "in".into(),
                    value: serde_json::to_string(issues).unwrap_or_default(),
                    quoted: false,
                }],
            )?;

            Statement {
                stream: RumStream::Errors,
                sql: latest(
                    RumStream::Errors,
                    of_issues,
                    &["session.id", "error.message"],
                ),
                value: None,
                detail: Detail::Column("error.message"),
            }
        }
        RumAlertMetric::CrashFreeSessions => Statement {
            stream: RumStream::Sessions,
            sql: latest(
                RumStream::Sessions,
                vec![Condition::raw(
                    "\"session.crash.count\" > 0",
                    &["session.crash.count"],
                    false,
                )],
                &["session.id", "session.crash.count"],
            ),
            value: Some(("session.crash.count", 1.0)),
            detail: Detail::None,
        },
        RumAlertMetric::FrustrationRate => Statement {
            stream: RumStream::Sessions,
            sql: latest(
                RumStream::Sessions,
                vec![Condition::raw(
                    "\"session.frustration.count\" > 0",
                    &["session.frustration.count"],
                    false,
                )],
                &["session.id", "session.frustration.count"],
            ),
            value: Some(("session.frustration.count", 1.0)),
            detail: Detail::None,
        },
        RumAlertMetric::FailedRequestRate => {
            let mut failed = filter_conditions(RumStream::Resources, &[calls()])?;

            failed.push(Condition::raw(
                "(\"resource.status_code\" = 0 OR \"resource.status_code\" >= 500)",
                &["resource.status_code"],
                false,
            ));

            Statement {
                stream: RumStream::Resources,
                sql: latest(
                    RumStream::Resources,
                    failed,
                    &[
                        "session.id",
                        "resource.method",
                        "resource.url_path",
                        "resource.status_code",
                    ],
                ),
                value: Some(("resource.status_code", 1.0)),
                detail: Detail::Request,
            }
        }
        // A vital tells by how long it took; a timing has no column to rank by, so the latest
        // views that marked it tell.
        RumAlertMetric::CustomVital => Statement {
            stream: RumStream::Vitals,
            sql: ranked_sql(
                &Scope {
                    stream: RumStream::Vitals,
                    from,
                    to,
                    conditions: scope,
                },
                &["session.id", "vital.duration"],
                "vital.duration",
                CANDIDATES,
            ),
            value: Some(("vital.duration", NANOSECONDS_PER_MILLISECOND)),
            detail: Detail::None,
        },
        RumAlertMetric::CustomTiming => Statement {
            stream: RumStream::Views,
            sql: latest(RumStream::Views, Vec::new(), &["session.id"]),
            value: None,
            detail: Detail::None,
        },
        // A query is told by the rows it matched (`searched`).
        RumAlertMetric::SessionCount | RumAlertMetric::ViewCount | RumAlertMetric::Query => {
            return Ok(None);
        }
    };

    Ok(Some(statement))
}

/// The statement that finds the sessions of the newest rows an explorer query matched.
pub(super) fn searched(
    stream: RumStream,
    scope: &[Condition],
    (from, to): (i64, i64),
) -> Statement {
    Statement {
        stream,
        sql: list_sql(&ListRequest {
            scope: Scope {
                stream,
                from,
                to,
                conditions: scope,
            },
            columns: &["session.id"],
            limit: CANDIDATES,
            seek: None,
            oldest_first: false,
        }),
        value: None,
        detail: Detail::None,
    }
}

/// The sessions a result names, in its order, each once.
pub(super) fn samples_of(statement: &Statement, result: &QueryResult) -> Vec<RumAlertSample> {
    let column = |name: &str| result.columns.iter().position(|found| found == name);

    let Some(session) = column("session.id") else {
        return Vec::new();
    };

    let at = column(TS_COLUMN);
    let value = statement
        .value
        .and_then(|(name, divisor)| column(name).map(|index| (index, divisor)));
    let method = column("resource.method");
    let path = column("resource.url_path");

    let text = |row: &[Value], index: Option<usize>| {
        index
            .and_then(|index| row.get(index))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };

    let mut seen = HashSet::new();
    let mut samples = Vec::new();

    for row in &result.rows {
        let Some(id) = text(row, Some(session)) else {
            continue;
        };

        if !seen.insert(id.clone()) {
            continue;
        }

        let detail = match statement.detail {
            Detail::None => None,
            Detail::Column(name) => text(row, column(name)),
            Detail::Request => match (text(row, method), text(row, path)) {
                (Some(method), Some(path)) => Some(format!("{method} {path}")),
                (None, Some(path)) => Some(path),
                _ => None,
            },
        };

        samples.push(RumAlertSample {
            session_id: id,
            at: TimestampMicros(
                at.and_then(|index| row.get(index))
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            ),
            value: value.and_then(|(index, divisor)| {
                row.get(index)
                    .and_then(Value::as_f64)
                    .map(|number| number / divisor)
            }),
            detail: detail.map(|detail| detail.chars().take(MAX_DETAIL).collect()),
        });

        if samples.len() == SAMPLES {
            break;
        }
    }

    samples
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::infra::rum::alerts::scope;

    fn scoped() -> Vec<Condition> {
        scope::conditions(
            &crate::domain::rum::RumAlertQuery {
                application_id: "shop-web".into(),
                environment: Some("prod".into()),
                page: None,
                version: None,
                metric: RumAlertMetric::LcpP75,
                min_samples: None,
                custom_name: None,
                statistic: None,
                search: None,
            },
            crate::domain::rum::RumPlatform::Browser,
        )
        .unwrap()
    }

    fn built(metric: RumAlertMetric, issues: &[String]) -> Option<Statement> {
        statement(metric, &scoped(), (100, 200), issues).unwrap()
    }

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|name| (*name).to_string()).collect(),
            scanned_rows: rows.len() as u64,
            rows,
            took_ms: 1,
            federation: None,
        }
    }

    #[test]
    fn a_figure_that_is_not_about_sessions_has_none_to_show() {
        assert!(built(RumAlertMetric::SessionCount, &[]).is_none());
        assert!(built(RumAlertMetric::ViewCount, &[]).is_none());
        assert!(built(RumAlertMetric::NewIssues, &[]).is_none());
    }

    #[test]
    fn the_worst_views_of_a_vital_are_ranked_by_it() {
        let found = built(RumAlertMetric::LcpP75, &[]).unwrap();

        assert_eq!(found.stream, RumStream::Views);
        assert!(
            found
                .sql
                .contains("ORDER BY \"view.performance.lcp.timestamp\" DESC")
        );
        assert!(found.sql.contains("\"env\" = 'prod'"));
    }

    #[test]
    fn the_errors_of_new_issues_are_found_by_the_issues() {
        let found = built(RumAlertMetric::NewIssues, &["a1".to_string()]).unwrap();

        assert_eq!(found.stream, RumStream::Errors);
        assert!(found.sql.contains("\"error.issue_id\" IN ('a1')"));
    }

    #[test]
    fn failed_requests_are_the_calls_with_no_answer_or_a_server_error() {
        let found = built(RumAlertMetric::FailedRequestRate, &[]).unwrap();

        assert!(found.sql.contains("\"resource.type\" IN ('fetch', 'xhr')"));
        assert!(
            found
                .sql
                .contains("(\"resource.status_code\" = 0 OR \"resource.status_code\" >= 500)")
        );
    }

    #[test]
    fn a_session_is_named_once_and_the_figure_is_in_the_unit_of_the_metric() {
        let found = built(RumAlertMetric::LcpP75, &[]).unwrap();

        let samples = samples_of(
            &found,
            &result(
                &[
                    "session.id",
                    "view.performance.lcp.timestamp",
                    "view.performance.lcp.target_selector",
                    TS_COLUMN,
                ],
                vec![
                    vec![
                        json!("s-1"),
                        json!(7_200_000_000.0),
                        json!("img.hero"),
                        json!(10),
                    ],
                    vec![
                        json!("s-1"),
                        json!(6_000_000_000.0),
                        json!("img.hero"),
                        json!(11),
                    ],
                    vec![json!("s-2"), json!(5_800_000_000.0), Value::Null, json!(12)],
                    vec![Value::Null, json!(5_000_000_000.0), Value::Null, json!(13)],
                    vec![json!("s-3"), json!(5_100_000_000.0), json!(" "), json!(14)],
                    vec![json!("s-4"), json!(5_000_000_000.0), json!("p"), json!(15)],
                ],
            ),
        );

        assert_eq!(
            samples,
            vec![
                RumAlertSample {
                    session_id: "s-1".into(),
                    at: TimestampMicros(10),
                    value: Some(7_200.0),
                    detail: Some("img.hero".into()),
                },
                RumAlertSample {
                    session_id: "s-2".into(),
                    at: TimestampMicros(12),
                    value: Some(5_800.0),
                    detail: None,
                },
                RumAlertSample {
                    session_id: "s-3".into(),
                    at: TimestampMicros(14),
                    value: Some(5_100.0),
                    detail: None,
                },
            ]
        );
    }

    #[test]
    fn a_failed_request_is_told_by_its_method_and_path() {
        let found = built(RumAlertMetric::FailedRequestRate, &[]).unwrap();

        let samples = samples_of(
            &found,
            &result(
                &[
                    "session.id",
                    "resource.method",
                    "resource.url_path",
                    "resource.status_code",
                    TS_COLUMN,
                ],
                vec![vec![
                    json!("s-1"),
                    json!("POST"),
                    json!("/api/cart"),
                    json!(503),
                    json!(7),
                ]],
            ),
        );

        assert_eq!(samples[0].detail.as_deref(), Some("POST /api/cart"));
        assert_eq!(samples[0].value, Some(503.0));
    }

    #[test]
    fn a_result_without_sessions_names_none() {
        let found = built(RumAlertMetric::ErrorCount, &[]).unwrap();

        assert!(samples_of(&found, &result(&["error.message"], vec![vec![json!("x")]])).is_empty());
    }
}
