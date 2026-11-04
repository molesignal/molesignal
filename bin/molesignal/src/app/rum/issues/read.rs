// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the issue pages read out of the errors themselves: how many sessions and people an issue
//! touched in a range, how often it happened over the range, and the latest errors it was made of.
//! The reads go through the query engine, so what it masks stays masked.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use super::select::{Numbers, Sort};
use crate::{
    app::rum::{
        aggregate::number,
        reader::{Reader, Span},
    },
    domain::rum::{ERRORS_FILTER, event::RumStream},
    infra::rum::{
        issues::identity::{FrameView, frame_views},
        query::{
            AggregateSpec, BucketPlan, Condition, ListRequest, MeasureSpec, Plan, Scope, list_sql,
            rows,
        },
    },
    shared::Result,
};

/// How many issues are read to be filtered, ordered and cut to a page.
pub const CANDIDATES: usize = 250;

/// How many equal parts the range of a list is cut into for the trend of each issue.
pub const TREND_BARS: i64 = 12;

/// How many sessions an issue is shown with as examples.
pub const SAMPLES: usize = 3;

/// How many of the latest errors of an issue are read to find those sessions: an error that
/// happens again in a session is the same example.
const SAMPLE_ROWS: usize = 12;

/// The shortest part of a trend: the least a bar of the engine can be.
const MIN_BAR_MICROS: i64 = 1_000;

const ISSUE: &str = "error.issue_id";

fn measure(function: &str, field: Option<&str>, fallback: Option<&str>, name: &str) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: field.map(str::to_string),
        fallback: fallback.map(str::to_string),
        filters: Vec::new(),
        name: Some(name.into()),
    }
}

/// What an issue is counted by: the sessions it touched, the people (a person without an id is
/// told by their anonymous id) and how often it happened.
fn counts() -> Vec<MeasureSpec> {
    vec![
        measure("count_distinct", Some("session.id"), None, "sessions"),
        measure(
            "count_distinct",
            Some("usr.id"),
            Some("usr.anonymous_id"),
            "users",
        ),
        measure("count", None, None, "occurrences"),
    ]
}

/// The statement that counts each issue, the ones that rank first by `sort` being read.
pub fn counting(sort: Sort) -> Result<Plan> {
    Plan::new(
        RumStream::Errors,
        &AggregateSpec {
            measures: &counts(),
            group_by: &[ISSUE.to_string()],
            bars: None,
            order_by: Some(sort.measure()),
            descending: Some(true),
            limit: Some(CANDIDATES),
        },
    )
}

/// The statement that counts all the errors it is run over, as one issue.
pub fn totals() -> Result<Plan> {
    Plan::new(
        RumStream::Errors,
        &AggregateSpec {
            measures: &counts(),
            group_by: &[],
            bars: None,
            order_by: None,
            descending: None,
            limit: None,
        },
    )
}

fn numbers_of(values: &[Value]) -> Numbers {
    Numbers {
        sessions: number(values.first()),
        users: number(values.get(1)),
        occurrences: number(values.get(2)),
    }
}

/// What each issue with errors in the range came to, the ones ranking first first. `None` when
/// nothing was ever written.
pub async fn count_issues(
    reader: &Reader<'_>,
    span: Span,
    scope: &Scope<'_>,
    plan: &Plan,
) -> Result<Option<Vec<(String, Numbers)>>> {
    let Some(result) = reader
        .run_tracked(RumStream::Errors, span, plan.first_sql(scope))
        .await?
    else {
        return Ok(None);
    };

    let answer = plan.answer(&result.columns, &result.rows)?;

    Ok(Some(
        answer
            .rows
            .iter()
            .filter_map(|row| {
                let id = row.group.first()?.clone()?;

                Some((id, numbers_of(&row.values)))
            })
            .collect(),
    ))
}

/// What the errors the scope selects come to as one issue.
pub async fn count_one(reader: &Reader<'_>, span: Span, scope: &Scope<'_>) -> Result<Numbers> {
    let plan = totals()?;

    let Some(result) = reader
        .run_tracked(RumStream::Errors, span, plan.first_sql(scope))
        .await?
    else {
        return Ok(Numbers::default());
    };

    let answer = plan.answer(&result.columns, &result.rows)?;

    Ok(answer
        .rows
        .first()
        .map(|row| numbers_of(&row.values))
        .unwrap_or_default())
}

/// How often each of `ids` happened in each part of the range, oldest part first.
pub async fn trends(
    reader: &Reader<'_>,
    span: Span,
    scope: &Scope<'_>,
    ids: &[String],
) -> Result<HashMap<String, Vec<u64>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let length = span.to - span.from;
    let bucket = ((length + TREND_BARS - 1) / TREND_BARS).max(MIN_BAR_MICROS);
    let parts = ((length + bucket - 1) / bucket).max(1) as usize;

    let bars = BucketPlan::new(Some(span.from), Some(span.to), Some(bucket), None)?;

    let plan = Plan::new(
        RumStream::Errors,
        &AggregateSpec {
            measures: &[measure("count", None, None, "occurrences")],
            group_by: &[ISSUE.to_string()],
            bars: Some(bars),
            order_by: None,
            descending: None,
            limit: Some(1),
        },
    )?;

    let groups = ids
        .iter()
        .map(|id| vec![Some(id.clone())])
        .collect::<Vec<_>>();

    let Some(result) = reader
        .run_tracked(RumStream::Errors, span, plan.series_sql(scope, &groups))
        .await?
    else {
        return Ok(HashMap::new());
    };

    let answer = plan.answer(&result.columns, &result.rows)?;

    // Time masked for this reader: there is no trend to draw.
    if answer.unavailable.is_some() {
        return Ok(HashMap::new());
    }

    let mut trends: HashMap<String, Vec<u64>> = HashMap::new();

    for row in &answer.rows {
        let (Some(Some(id)), Some(start)) = (row.group.first(), row.start_time) else {
            continue;
        };

        let at = ((start - span.from) / bucket) as usize;

        if at < parts {
            trends.entry(id.clone()).or_insert_with(|| vec![0; parts])[at] +=
                number(row.values.first());
        }
    }

    Ok(trends)
}

/// A trend of an issue that had no error in the range: as flat as the others are long.
pub fn flat(span: Span) -> Vec<u64> {
    let length = span.to - span.from;
    let bucket = ((length + TREND_BARS - 1) / TREND_BARS).max(MIN_BAR_MICROS);

    vec![0; (((length + bucket - 1) / bucket).max(1)) as usize]
}

/// One of the latest errors of an issue, as far as it says who met it and where.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub timestamp: i64,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub anonymous_id: Option<String>,
    pub page: Option<String>,
    pub browser: Option<String>,
    pub system: Option<String>,
    pub version: Option<String>,
    pub has_replay: bool,
    /// Whether the session the error happened in was kept. The errors of a session that was not
    /// are kept alone, and there is no session to open.
    pub retained: bool,
}

/// The stack of the latest error of an issue that has one, and what is said with it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    pub message: Option<String>,
    pub text: Option<String>,
    pub frames: Vec<FrameView>,
    /// How far the frames were resolved: `completed`, `partial` or `missing`; none when nothing
    /// was tried.
    pub symbolication: Option<String>,
    pub handling: Option<String>,
    pub is_crash: bool,
    pub category: Option<String>,
}

const SAMPLE_COLUMNS: &[&str] = &[
    "session.id",
    "usr.id",
    "usr.anonymous_id",
    "view.url_path",
    "browser.name",
    "browser.version_major",
    "os.name",
    "os.version_major",
    "version",
    "session.has_replay",
    "retention.filter",
    "error.message",
    "error.stack",
    "error.frames",
    "error.symbolication",
    "error.handling",
    "error.is_crash",
    "error.category",
];

fn text(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn named(name: Option<String>, version: Option<String>) -> Option<String> {
    match (name, version) {
        (Some(name), Some(version)) => Some(format!("{name} {version}")),
        (name, version) => name.or(version),
    }
}

/// The latest sessions the errors the scope selects happened in, newest first, with the stack of
/// the first error that has one.
pub async fn samples(
    reader: &Reader<'_>,
    span: Span,
    conditions: &[Condition],
) -> Result<(Vec<Sample>, Option<Stack>)> {
    let scope = Scope {
        stream: RumStream::Errors,
        from: span.from,
        to: span.to,
        conditions,
    };

    let sql = list_sql(&ListRequest {
        scope,
        columns: SAMPLE_COLUMNS,
        limit: SAMPLE_ROWS,
        seek: None,
        oldest_first: false,
    });

    let Some(result) = reader
        .run(RumStream::Errors, span, sql, Some(SAMPLE_ROWS))
        .await?
    else {
        return Ok((Vec::new(), None));
    };

    let latest = rows(RumStream::Errors, result);

    let mut sessions = HashSet::new();

    let samples = latest
        .iter()
        .filter(|row| text(&row.fields, "session.id").is_none_or(|id| sessions.insert(id)))
        .take(SAMPLES)
        .map(|row| {
            let fields = &row.fields;

            Sample {
                timestamp: row.timestamp_micros,
                session_id: text(fields, "session.id"),
                user_id: text(fields, "usr.id"),
                anonymous_id: text(fields, "usr.anonymous_id"),
                page: text(fields, "view.url_path"),
                browser: named(
                    text(fields, "browser.name"),
                    text(fields, "browser.version_major"),
                ),
                system: named(text(fields, "os.name"), text(fields, "os.version_major")),
                version: text(fields, "version"),
                has_replay: fields
                    .get("session.has_replay")
                    .and_then(Value::as_bool)
                    .unwrap_or_default(),
                retained: text(fields, "retention.filter").as_deref() != Some(ERRORS_FILTER),
            }
        })
        .collect();

    let framed = latest.iter().find(|row| {
        row.fields
            .get("error.frames")
            .and_then(Value::as_array)
            .is_some_and(|frames| !frames.is_empty())
    });

    let stack = framed.or_else(|| latest.first()).map(|row| {
        let fields = &row.fields;

        Stack {
            message: text(fields, "error.message"),
            text: text(fields, "error.stack"),
            frames: fields
                .get("error.frames")
                .map(frame_views)
                .unwrap_or_default(),
            symbolication: fields
                .get("error.symbolication")
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .map(str::to_string),
            handling: text(fields, "error.handling"),
            is_crash: fields
                .get("error.is_crash")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
            category: text(fields, "error.category"),
        }
    });

    Ok((samples, stack))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn counts_are_read_in_the_order_of_the_measures() {
        assert_eq!(
            numbers_of(&[json!(5), json!(4), json!(30)]),
            Numbers {
                sessions: 5,
                users: 4,
                occurrences: 30
            }
        );
    }

    #[test]
    fn the_statements_count_what_the_page_shows() {
        let counting = counting(Sort::Users).unwrap();
        let totals = totals().unwrap();

        assert!(!counting.needs_series());
        assert!(!totals.needs_series());
    }

    #[test]
    fn a_name_and_a_version_make_a_label() {
        assert_eq!(
            named(Some("Chrome".into()), Some("129".into())),
            Some("Chrome 129".into())
        );
        assert_eq!(named(Some("Chrome".into()), None), Some("Chrome".into()));
        assert_eq!(named(None, None), None);
    }
}
