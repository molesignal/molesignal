// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where the requests a trace query matches spend their time, read from a
//! sample of them: the latest such requests (at most [`SAMPLE`]) and, in
//! their traces, the Spans under each: calls out by what they call (a
//! database, a cache, HTTP, RPC, a message system), the service's internal
//! work, and the request's own time, each exclusive of what runs under it.
//! Calls made in parallel can add up past a request's length; the parts are
//! then scaled to it.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use self::spend::{Request, SpanRow, TimePart, classify, kind_of, spend};
use super::{
    filter::{self, TraceCondition},
    list::{MAX_TRACE_FILTERS, clean_input},
    red::{RedScope, RedUnavailable, run_tracked, settled, statements::Source},
    resolve_traces_stream_definition,
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{BucketPlan, within_budget},
    },
    app::iam::IamContext,
    domain::{iam::permission, query::QueryResult, stream::Schema},
    infra::query::escape_sql_ident,
    shared::Result,
};

pub(super) mod spend;

/// The most requests a reading samples.
const SAMPLE: usize = 200;
/// The most Spans of the sampled traces a reading looks at.
const MAX_SPANS: usize = 20_000;

/// The attributes that say what a call out calls, as the stream may keep them.
const CALL_FIELDS: [&str; 9] = [
    "db.system.name",
    "db.system",
    "messaging.system",
    "rpc.service",
    "rpc.system",
    "http.request.method",
    "http.method",
    "server.address",
    "peer.service",
];

#[derive(Debug, Deserialize)]
pub(super) struct TimeSpentQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    #[serde(rename = "where")]
    condition: Option<String>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimeSpentResponse {
    /// How many requests were read.
    sampled: usize,
    /// Their time together, in microseconds.
    total_time: u64,
    /// Where it went, the largest part first.
    parts: Vec<TimePart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<RedUnavailable>,
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn time_spent(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TimeSpentQuery>,
) -> Result<Json<TimeSpentResponse>> {
    let plan = BucketPlan::new(query.from, query.to, None, None)?;
    let mut response = TimeSpentResponse::default();

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(response));
    };

    let filters = filter::parse(
        query.filters.as_deref(),
        &definition.schema,
        MAX_TRACE_FILTERS,
    )?;

    let condition = filter::condition::parse(
        query.condition.as_deref(),
        &definition.schema,
        MAX_TRACE_FILTERS - filters.len(),
    )?;

    let text = query.q.as_deref().and_then(clean_input);
    let asked = TraceCondition::of(&filters, text.as_deref(), condition.as_ref());
    let source = Source::new(&definition, RedScope::Entry, &asked)?;
    let run = |statement| run_tracked(&state, &ctx, &definition.name, &plan, statement);

    let reading = async {
        let requests = sampled(&run(sample_statement(&source)).await?);

        if requests.is_empty() {
            return Ok::<_, crate::shared::Error>((requests, Vec::new()));
        }

        let traces = requests
            .iter()
            .map(|request| request.trace.as_str())
            .collect::<Vec<_>>();
        let statement = spans_statement(&source, &definition.schema, &traces);
        let spans = span_rows(&run(statement).await?);

        Ok((requests, spans))
    };

    match settled(within_budget(reading).await)? {
        Ok((requests, spans)) => {
            response.sampled = requests.len();
            (response.total_time, response.parts) = spend(&requests, &spans);
        }
        Err(reason) => response.unavailable = Some(reason),
    }

    Ok(Json(response))
}

fn sample_statement(source: &Source) -> String {
    format!(
        "SELECT \"trace_id\" AS __ts_trace, \"span_id\" AS __ts_span, {} AS __ts_duration {} ORDER BY _timestamp DESC LIMIT {SAMPLE}",
        source.duration(),
        source.reading_with(&[]),
    )
}

fn spans_statement(source: &Source, schema: &Schema, traces: &[&str]) -> String {
    let mut columns = vec![
        "\"trace_id\" AS __ts_trace".to_owned(),
        "\"span_id\" AS __ts_span".to_owned(),
        format!("{} AS __ts_duration", source.duration()),
    ];

    for (field, alias) in [("parent_span_id", "__ts_parent"), ("kind", "__ts_kind")] {
        if declared(schema, field) {
            columns.push(format!("\"{field}\" AS {alias}"));
        }
    }

    for (index, field) in CALL_FIELDS.iter().enumerate() {
        if declared(schema, field) {
            columns.push(format!(
                "\"{}\" AS __ts_call{index}",
                escape_sql_ident(field)
            ));
        }
    }

    let traces = traces
        .iter()
        .map(|trace| format!("'{}'", trace.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "SELECT {} FROM {} WHERE \"trace_id\" IN ({traces}) LIMIT {MAX_SPANS}",
        columns.join(", "),
        source.table(),
    )
}

fn declared(schema: &Schema, field: &str) -> bool {
    schema
        .fields
        .iter()
        .any(|candidate| candidate.name == field)
}

fn column(result: &QueryResult, name: &str) -> Option<usize> {
    result.columns.iter().position(|column| column == name)
}

fn text(row: &[Value], index: Option<usize>) -> Option<String> {
    match row.get(index?)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn micros(row: &[Value], index: Option<usize>) -> u64 {
    row.get(index.unwrap_or(usize::MAX))
        .and_then(Value::as_f64)
        .map_or(0, |nanos| (nanos.max(0.0) / 1_000.0) as u64)
}

fn sampled(result: &QueryResult) -> Vec<Request> {
    let (trace, span, duration) = (
        column(result, "__ts_trace"),
        column(result, "__ts_span"),
        column(result, "__ts_duration"),
    );

    result
        .rows
        .iter()
        .filter_map(|row| {
            Some(Request {
                trace: text(row, trace)?,
                span: text(row, span)?,
                micros: micros(row, duration),
            })
        })
        .collect()
}

fn span_rows(result: &QueryResult) -> Vec<SpanRow> {
    let index = |name: &str| column(result, name);
    let calls = CALL_FIELDS
        .iter()
        .enumerate()
        .map(|(position, field)| (*field, index(&format!("__ts_call{position}"))))
        .collect::<Vec<_>>();

    result
        .rows
        .iter()
        .filter_map(|row| {
            let kind = kind_of(index("__ts_kind").and_then(|at| row.get(at)));
            let said = calls
                .iter()
                .filter_map(|(field, at)| text(row, *at).map(|value| (*field, value)))
                .collect::<Vec<_>>();

            Some(SpanRow {
                trace: text(row, index("__ts_trace"))?,
                span: text(row, index("__ts_span"))?,
                parent: text(row, index("__ts_parent")).unwrap_or_default(),
                kind,
                micros: micros(row, index("__ts_duration")),
                calls: classify(kind, &said),
            })
        })
        .collect()
}
