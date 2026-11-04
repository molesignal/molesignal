// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/trace/:trace_id/linked` — the Spans of other traces that
//! link to this one.
//!
//! A Span's links point at Spans it follows from: the message it consumes,
//! the request a batch was cut from. Those come with the Span itself. The
//! other way round, the Spans elsewhere whose links name this trace, are
//! found here, in the window the reader gives. A Span keeps its links as a
//! document, so the window is read for the trace id in that text: the reading
//! runs as a tracked query inside the histogram's time budget, and says why
//! when it cannot finish.

use axum::{
    Extension,
    extract::{Path, Query, State},
    response::Json,
};
use serde::Serialize;
use serde_json::Value;

use super::{
    filter::TraceCondition,
    red::statements::Source,
    request::{TraceWindow, validate_trace_id},
    resolve_traces_stream_definition,
};
use crate::{
    api::{
        AppState,
        http::routes::web::{histogram::within_budget, trace::red::RedScope},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{StreamDefinition, StreamType},
    },
    infra::query::{escape_sql_ident, scan_limit::is_over_scan_limit},
    shared::{Error, Result, time::TimestampMicros},
};

/// The most linking Spans one answer lists.
const MAX_LINKED: usize = 200;

/// A Span of another trace with a link to a Span of this one.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedSpan {
    pub trace_id: String,
    pub span_id: String,
    pub service: String,
    pub operation: String,
    pub start_ns: i64,
    pub duration_ns: i64,
    pub status: String,
    /// The Span of this trace the link points at.
    pub linked_span_id: String,
    /// The link's own attributes.
    pub attributes: Value,
}

/// Why the linking Spans could not be read. The client words it for its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkedUnavailable {
    TimeBudget,
    Busy,
    ScanLimit,
}

#[derive(Debug, Default, Serialize)]
pub struct LinkedResponse {
    /// The newest first.
    spans: Vec<LinkedSpan>,
    /// More Spans link here than are listed.
    more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<LinkedUnavailable>,
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn linked(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(trace_id): Path<String>,
    Query(window): Query<TraceWindow>,
) -> Result<Json<LinkedResponse>> {
    validate_trace_id(&trace_id).map_err(Error::invalid)?;

    let range = window.range(TimestampMicros::now())?;

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(LinkedResponse::default()));
    };

    let Some(statement) = statement(&definition, &trace_id)? else {
        return Ok(Json(LinkedResponse::default()));
    };

    let request = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement,
        time_range: range,
        stream: Some(StreamHint {
            name: definition.name.clone(),
            stream_type: StreamType::TRACES,
        }),
        limit: Some(MAX_LINKED + 1),
        step: None,
        federation_clusters: Vec::new(),
    };

    let read = state
        .query
        .run_tracked(request, ctx.user_id.clone(), ctx.organization_role_key());

    let result = match within_budget(read).await {
        Ok(Some(result)) => result,
        Ok(None) => return Ok(Json(unavailable(LinkedUnavailable::TimeBudget))),
        Err(Error::ResourceExhausted(_)) => return Ok(Json(unavailable(LinkedUnavailable::Busy))),
        Err(error) if is_over_scan_limit(&error) => {
            return Ok(Json(unavailable(LinkedUnavailable::ScanLimit)));
        }
        Err(error) => return Err(error),
    };

    let more = result.rows.len() > MAX_LINKED;
    let mut spans = linking_spans(&result, &trace_id);
    spans.truncate(MAX_LINKED);

    Ok(Json(LinkedResponse {
        spans,
        more,
        unavailable: None,
    }))
}

fn unavailable(reason: LinkedUnavailable) -> LinkedResponse {
    LinkedResponse {
        unavailable: Some(reason),
        ..LinkedResponse::default()
    }
}

/// The statement that finds the Spans of other traces whose links name the
/// trace, the newest first. `None` for a stream that keeps no links.
fn statement(definition: &StreamDefinition, trace_id: &str) -> Result<Option<String>> {
    let declared = |name: &str| {
        definition
            .schema
            .fields
            .iter()
            .any(|field| field.name == name)
    };

    if !declared("links") || !declared("trace_id") || !declared("span_id") {
        return Ok(None);
    }

    let source = Source::new(
        definition,
        RedScope::All,
        &TraceCondition::of(&[], None, None),
    )?;

    let start = if declared("start_time_unix_nano") {
        "CAST(\"start_time_unix_nano\" AS BIGINT)".to_string()
    } else {
        "(CAST(_timestamp AS BIGINT) * 1000)".to_string()
    };

    let selected = [
        "trace_id",
        "span_id",
        "service.name",
        "name",
        "status_code",
        "links",
    ]
    .into_iter()
    .filter(|name| declared(name))
    .map(quoted)
    .chain([
        format!("{start} AS __linked_start"),
        format!("{} AS __linked_duration", source.duration()),
    ])
    .collect::<Vec<_>>();

    // The id is checked to be letters, digits, `-` and `_`: it needs no escaping in a pattern.
    let conditions = [
        format!("\"trace_id\" <> {}", literal(trace_id)),
        format!(
            "CAST(\"links\" AS VARCHAR) LIKE {}",
            literal(&format!("%{trace_id}%"))
        ),
    ];

    Ok(Some(format!(
        "SELECT {} {} ORDER BY {start} DESC LIMIT {}",
        selected.join(", "),
        source.reading_with(&conditions),
        MAX_LINKED + 1,
    )))
}

/// A row for each link of a result's Spans that points into the trace.
fn linking_spans(result: &QueryResult, trace_id: &str) -> Vec<LinkedSpan> {
    let position = |name: &str| result.columns.iter().position(|column| column == name);
    let text = |row: &[Value], name: &str| {
        position(name)
            .and_then(|index| row.get(index))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let number = |row: &[Value], name: &str| {
        position(name)
            .and_then(|index| row.get(index))
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
            .unwrap_or_default()
    };

    let mut spans = Vec::new();

    for row in &result.rows {
        let links = position("links")
            .and_then(|index| row.get(index))
            .map(document)
            .unwrap_or(Value::Null);

        for link in links.as_array().into_iter().flatten() {
            if link.get("trace_id").and_then(Value::as_str) != Some(trace_id) {
                continue;
            }

            spans.push(LinkedSpan {
                trace_id: text(row, "trace_id"),
                span_id: text(row, "span_id"),
                service: text(row, "service.name"),
                operation: text(row, "name"),
                start_ns: number(row, "__linked_start"),
                duration_ns: number(row, "__linked_duration").max(0),
                status: text(row, "status_code"),
                linked_span_id: link
                    .get("span_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                attributes: link
                    .get("attributes")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default())),
            });
        }
    }

    spans
}

/// A document kept as text, read back; one kept as itself, as it is.
fn document(value: &Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", escape_sql_ident(name))
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, trace::normalization::canonical_trace_schema};

    fn definition() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("spans"),
            org_id: Id::from_string("org"),
            name: "spans".into(),
            stream_type: StreamType::TRACES,
            schema: canonical_trace_schema(),
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn looks_for_the_trace_in_the_links_of_other_traces() {
        let definition = definition();
        let sql = statement(&definition, "abc123").unwrap().unwrap();

        assert!(sql.contains("\"trace_id\" <> 'abc123'"), "{sql}");
        assert!(
            sql.contains("CAST(\"links\" AS VARCHAR) LIKE '%abc123%'"),
            "{sql}"
        );
        assert!(sql.ends_with(&format!("LIMIT {}", MAX_LINKED + 1)), "{sql}");
    }

    #[test]
    fn lists_each_link_into_the_trace_with_the_span_that_holds_it() {
        let result = QueryResult {
            columns: vec![
                "trace_id".into(),
                "span_id".into(),
                "service.name".into(),
                "name".into(),
                "status_code".into(),
                "links".into(),
                "__linked_start".into(),
                "__linked_duration".into(),
            ],
            rows: vec![vec![
                json!("other"),
                json!("consumer"),
                json!("fraud"),
                json!("orders process"),
                json!("OK"),
                json!(json!([
                    {"trace_id": "abc123", "span_id": "publish", "attributes": {"messaging.operation": "process"}},
                    {"trace_id": "elsewhere", "span_id": "x", "attributes": {}}
                ])
                .to_string()),
                json!(5_000),
                json!(1_200),
            ]],
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };

        assert_eq!(
            linking_spans(&result, "abc123"),
            vec![LinkedSpan {
                trace_id: "other".into(),
                span_id: "consumer".into(),
                service: "fraud".into(),
                operation: "orders process".into(),
                start_ns: 5_000,
                duration_ns: 1_200,
                status: "OK".into(),
                linked_span_id: "publish".into(),
                attributes: json!({"messaging.operation": "process"}),
            }]
        );
    }
}
