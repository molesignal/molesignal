// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM sessions and backend traces, both ways.
//!
//! `GET /rum/sessions/{id}/related-traces` names the traces made while a session ran: first those
//! whose id a request of the session carried (`resource.trace_id`, from the `traceparent` the SDK
//! sent), and when there are none, the traces that ran within the session's time. The answer says
//! which kind each is, so the page can show how sure it is.
//!
//! `GET /rum/traces/{trace_id}/session` goes the other way: the session whose request carried the
//! id of a backend trace.

use std::collections::HashSet;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    entity::{EntityQuery, find, lookup_span},
    reader::{Reader, Row, Span, rows},
};
use crate::{
    api::{AppState, http::routes::web::trace::resolve_traces_stream},
    app::iam::IamContext,
    domain::{iam::permission, rum::event::RumStream},
    infra::{
        rum::query::{Condition, ListRequest, Scope, list_sql},
        traces::summary_reader::{
            SummaryOrder, TraceSummaryQuery, TraceSummaryReader, TraceSummaryRecord,
        },
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// A request is stamped a little before or after the trace it made.
const GRACE_MICROS: i64 = 5 * 60 * 1_000_000;

/// A session lasts at least this long when looking for the traces made in it.
const MIN_SESSION_MICROS: i64 = 60 * 1_000_000;

/// Without a window, a trace's session is looked for over the last day.
const TRACE_SESSION_LOOKBACK_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// The most requests of one session read for the trace ids they carry.
const MAX_REQUESTS: usize = 1_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RelatedTrace {
    trace_id: String,
    service: Option<String>,
    span_count: u64,
    duration_ms: Option<f64>,
    started_at: Option<i64>,
    /// `direct`: a request of the session carried the trace id. `time-correlated`: it ran while
    /// the session did, and may be someone else's.
    relation: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RelatedTraces {
    session_id: String,
    primary_service: Option<String>,
    traces: Vec<RelatedTrace>,
}

#[permission(any("rum.sessions.read", "sys.telemetry.read"))]
pub(super) async fn related_traces(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(session_id): Path<String>,
    Query(query): Query<EntityQuery>,
) -> Result<Json<RelatedTraces>> {
    let reader = Reader::new(&state, &iam);
    let lookup = lookup_span(&query, TimestampMicros::now().0)?;

    let none = |session_id: String| {
        Json(RelatedTraces {
            session_id,
            primary_service: None,
            traces: Vec::new(),
        })
    };

    let Some(session) = find(&reader, RumStream::Sessions, &session_id, lookup).await? else {
        return Ok(none(session_id));
    };

    let window = session_window(&session);

    let carried = carried_trace_ids(&reader, &session_id, window).await?;

    let primary_service = carried
        .iter()
        .find_map(|(_, service)| service.clone())
        .filter(|service| !service.is_empty());

    if !carried.is_empty() {
        let ids = carried.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let traces = summaries(&state, &iam, Some(&ids), window, "direct").await?;

        return Ok(Json(RelatedTraces {
            session_id,
            primary_service,
            traces: with_services(traces, &carried),
        }));
    }

    let traces = summaries(&state, &iam, None, window, "time-correlated").await?;

    Ok(Json(RelatedTraces {
        session_id,
        primary_service: None,
        traces,
    }))
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TraceSessionQuery {
    /// When the trace ran, in epoch microseconds.
    from: Option<i64>,
    to: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceSession {
    session_id: String,
    /// When the request that made the trace's request was sent.
    timestamp: i64,
    application: Option<String>,
    /// The path of the page it was sent from.
    page: Option<String>,
    view_id: Option<String>,
    action_id: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceSessionAnswer {
    trace_id: String,
    session: Option<TraceSession>,
}

/// The RUM session a backend trace was made in: the session of the request that carried the
/// trace's id in `traceparent`. Of several such requests (one retried with its trace), the
/// earliest.
#[permission(any("rum.sessions.read", "sys.telemetry.read"))]
pub(super) async fn trace_session(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(trace_id): Path<String>,
    Query(query): Query<TraceSessionQuery>,
) -> Result<Json<TraceSessionAnswer>> {
    let trace_id = trace_id.trim().to_string();

    if trace_id.is_empty() {
        return Err(Error::invalid("empty trace id"));
    }

    let span = trace_session_span(query.from, query.to, TimestampMicros::now().0)?;
    let reader = Reader::new(&state, &iam);

    let conditions = [Condition::equals("resource.trace_id", &trace_id)];

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream: RumStream::Resources,
            from: span.from,
            to: span.to,
            conditions: &conditions,
        },
        columns: &[
            "session.id",
            "application.id",
            "view.id",
            "view.url_path",
            "action.id",
            "resource.url",
        ],
        limit: 1,
        seek: None,
        oldest_first: true,
    });

    let found = reader
        .run(RumStream::Resources, span, statement, Some(1))
        .await?
        .map(|result| rows(RumStream::Resources, result))
        .and_then(|found| found.into_iter().next());

    Ok(Json(TraceSessionAnswer {
        trace_id,
        session: found.and_then(trace_session_of),
    }))
}

fn trace_session_of(row: Row) -> Option<TraceSession> {
    let text = |name: &str| {
        row.fields
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    Some(TraceSession {
        session_id: text("session.id")?,
        timestamp: row.timestamp_micros,
        application: text("application.id"),
        page: text("view.url_path"),
        view_id: text("view.id"),
        action_id: text("action.id"),
        url: text("resource.url"),
    })
}

/// Where the requests of a trace are looked for: the window it ran in, widened by the grace a
/// request is stamped late by, at most a day of it; without a window, the last day.
fn trace_session_span(from: Option<i64>, to: Option<i64>, now: i64) -> Result<Span> {
    let (start, end) = match (from, to) {
        (Some(from), Some(to)) if to >= from => (from.max(to - TRACE_SESSION_LOOKBACK_MICROS), to),
        (None, None) => (now - TRACE_SESSION_LOOKBACK_MICROS, now),
        _ => return Err(Error::invalid("give both from and to, from before to")),
    };

    Ok(Span {
        from: start.saturating_sub(GRACE_MICROS),
        to: end.saturating_add(GRACE_MICROS),
    })
}

/// When a session ran, with the grace requests are stamped within.
fn session_window(session: &Row) -> Span {
    let duration = session
        .fields
        .get("session.duration")
        .and_then(Value::as_i64)
        .map_or(MIN_SESSION_MICROS, |ns| {
            (ns.max(0) / 1_000).max(MIN_SESSION_MICROS)
        });

    Span {
        from: session.timestamp_micros,
        to: session
            .timestamp_micros
            .saturating_add(duration)
            .saturating_add(GRACE_MICROS),
    }
}

/// The trace ids the requests of a session carried, each with the service that sent it, in the
/// order they were first seen.
async fn carried_trace_ids(
    reader: &Reader<'_>,
    session_id: &str,
    window: Span,
) -> Result<Vec<(String, Option<String>)>> {
    let conditions = [
        Condition::equals("session.id", session_id),
        Condition::raw(
            "(\"resource.trace_id\" IS NOT NULL AND \"resource.trace_id\" <> '')",
            &["resource.trace_id"],
            false,
        ),
    ];

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream: RumStream::Resources,
            from: window.from,
            to: window.to,
            conditions: &conditions,
        },
        columns: &["resource.trace_id", "service"],
        limit: MAX_REQUESTS,
        seek: None,
        oldest_first: true,
    });

    let Some(result) = reader
        .run(RumStream::Resources, window, statement, Some(MAX_REQUESTS))
        .await?
    else {
        return Ok(Vec::new());
    };

    let mut seen = HashSet::new();
    let mut carried = Vec::new();

    for row in rows(RumStream::Resources, result) {
        let Some(id) = row
            .fields
            .get("resource.trace_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };

        if seen.insert(id.to_string()) {
            let service = row
                .fields
                .get("service")
                .and_then(Value::as_str)
                .map(str::to_string);

            carried.push((id.to_string(), service));
        }
    }

    Ok(carried)
}

/// The traces of the organization within a window: those named, or those that ran entirely in it.
async fn summaries(
    state: &AppState,
    iam: &IamContext,
    ids: Option<&[String]>,
    window: Span,
    relation: &'static str,
) -> Result<Vec<RelatedTrace>> {
    let Some(stream) = resolve_traces_stream(state, &iam.org_id).await else {
        return Ok(Vec::new());
    };

    let named = ids.map(|ids| ids.iter().cloned().collect::<HashSet<_>>());

    let reader = TraceSummaryReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());

    let records = reader
        .scan(
            &iam.org_id,
            &stream,
            TimeRange::new(TimestampMicros(window.from), TimestampMicros(window.to)),
            TraceSummaryQuery {
                trace_ids: named.as_ref(),
                require_contained: named.is_none(),
                order: if named.is_some() {
                    SummaryOrder::Latest
                } else {
                    SummaryOrder::Earliest
                },
                limit: named.as_ref().map_or(20, |named| named.len().min(200)),
            },
        )
        .await?;

    Ok(records
        .into_iter()
        .map(|record| entry(record, relation))
        .collect())
}

fn entry(record: TraceSummaryRecord, relation: &'static str) -> RelatedTrace {
    RelatedTrace {
        trace_id: record.trace_id,
        service: record.service,
        span_count: record.span_count,
        duration_ms: Some(record.duration_ns as f64 / 1_000_000.0),
        started_at: Some(record.start_ns / 1_000),
        relation,
    }
}

/// A trace the backend gave no service takes the service of the request that carried its id.
fn with_services(
    mut traces: Vec<RelatedTrace>,
    carried: &[(String, Option<String>)],
) -> Vec<RelatedTrace> {
    for trace in &mut traces {
        if trace.service.is_some() {
            continue;
        }

        trace.service = carried
            .iter()
            .find(|(id, _)| *id == trace.trace_id)
            .and_then(|(_, service)| service.clone())
            .filter(|service| !service.is_empty());
    }

    traces
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn trace(id: &str, service: Option<&str>) -> RelatedTrace {
        RelatedTrace {
            trace_id: id.into(),
            service: service.map(str::to_string),
            span_count: 1,
            duration_ms: None,
            started_at: None,
            relation: "direct",
        }
    }

    fn row(fields: Value) -> Row {
        Row {
            timestamp_micros: 1_000,
            tie: String::new(),
            fields: match fields {
                Value::Object(fields) => fields,
                _ => Map::new(),
            },
        }
    }

    #[test]
    fn a_trace_session_is_looked_for_around_the_trace() {
        let span = trace_session_span(Some(10_000_000_000), Some(10_002_000_000), 0).unwrap();

        assert_eq!(span.from, 10_000_000_000 - GRACE_MICROS);
        assert_eq!(span.to, 10_002_000_000 + GRACE_MICROS);
    }

    #[test]
    fn a_trace_session_window_is_at_most_a_day() {
        let to = 100 * TRACE_SESSION_LOOKBACK_MICROS;
        let span = trace_session_span(Some(0), Some(to), 0).unwrap();

        assert_eq!(span.from, to - TRACE_SESSION_LOOKBACK_MICROS - GRACE_MICROS);
    }

    #[test]
    fn a_trace_session_without_a_window_is_looked_for_over_the_last_day() {
        let now = 50 * TRACE_SESSION_LOOKBACK_MICROS;
        let span = trace_session_span(None, None, now).unwrap();

        assert_eq!(
            span.from,
            now - TRACE_SESSION_LOOKBACK_MICROS - GRACE_MICROS
        );
        assert_eq!(span.to, now + GRACE_MICROS);
    }

    #[test]
    fn a_trace_session_window_needs_both_ends_in_order() {
        assert!(trace_session_span(Some(10), None, 0).is_err());
        assert!(trace_session_span(Some(10), Some(5), 0).is_err());
    }

    #[test]
    fn a_session_runs_at_least_a_minute() {
        let short = session_window(&row(json!({ "session.duration": 5_000_000_000_i64 })));

        assert_eq!(short.from, 1_000);
        assert_eq!(short.to, 1_000 + MIN_SESSION_MICROS + GRACE_MICROS);

        let long = session_window(&row(json!({ "session.duration": 600_000_000_000_i64 })));

        assert_eq!(long.to, 1_000 + 600_000_000 + GRACE_MICROS);
        assert_eq!(session_window(&row(json!({}))).to, short.to);
    }

    #[test]
    fn a_request_names_the_session_it_was_made_in() {
        let found = trace_session_of(row(json!({
            "session.id": "s-1",
            "application.id": "shop",
            "view.url_path": "/cart",
            "resource.url": "https://api.example.com/cart",
        })))
        .unwrap();

        assert_eq!(found.session_id, "s-1");
        assert_eq!(found.timestamp, 1_000);
        assert_eq!(found.page.as_deref(), Some("/cart"));
        assert_eq!(found.view_id, None);
        assert!(trace_session_of(row(json!({ "session.id": "" }))).is_none());
    }

    #[test]
    fn a_trace_without_a_service_takes_the_one_of_the_request_that_carried_it() {
        let carried = vec![
            ("a".to_string(), Some("web".to_string())),
            ("b".to_string(), Some(String::new())),
        ];

        let traces = with_services(
            vec![trace("a", None), trace("b", None), trace("c", Some("api"))],
            &carried,
        );

        assert_eq!(traces[0].service.as_deref(), Some("web"));
        assert_eq!(traces[1].service, None);
        assert_eq!(traces[2].service.as_deref(), Some("api"));
    }
}
