// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tool_runtime::ToolResult;

use super::ToolRuntime;
use crate::{
    app::trace::summary::carries_summaries,
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{Schema, StreamDefinition, StreamType},
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

pub(super) const DEFAULT_LOOKBACK_MICROS: i64 = 60 * 60 * 1_000_000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimeRangeArg {
    pub start_time: i64,
    pub end_time: i64,
}

pub(super) fn parse_args<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))
}

pub(super) fn json_result(value: &impl Serialize) -> Result<ToolResult> {
    serde_json::to_value(value)
        .map(ToolResult::json)
        .map_err(|error| Error::internal(format!("serialize tool result: {error}")))
}

pub(super) fn bounded<T>(mut values: Vec<T>, limit: Option<usize>, maximum: usize) -> Vec<T> {
    values.truncate(limit.unwrap_or(maximum.min(100)).clamp(1, maximum));
    values
}

pub(super) fn redact_credentials(value: &Value) -> Value {
    const SENSITIVE: [&str; 8] = [
        "token",
        "password",
        "secret",
        "authorization",
        "cookie",
        "apikey",
        "credential",
        "privatekey",
    ];

    match value {
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    // `apiKey` 与 `api_key` 一样算：去掉分隔符再找。
                    let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
                    let value = if SENSITIVE.iter().any(|needle| normalized.contains(needle)) {
                        Value::String("<redacted>".into())
                    } else {
                        redact_credentials(value)
                    };

                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_credentials).collect()),
        other => other.clone(),
    }
}

pub(super) fn time_range(value: TimeRangeArg) -> Result<TimeRange> {
    if value.start_time > value.end_time {
        return Err(Error::invalid(
            "timeRange.startTime must be less than or equal to endTime",
        ));
    }

    Ok(TimeRange::new(
        TimestampMicros(value.start_time),
        TimestampMicros(value.end_time),
    ))
}

pub(super) fn optional_time_range(value: Option<TimeRangeArg>) -> Result<TimeRange> {
    value.map_or_else(
        || {
            let end = TimestampMicros::now();

            Ok(TimeRange::new(
                TimestampMicros(end.0.saturating_sub(DEFAULT_LOOKBACK_MICROS)),
                end,
            ))
        },
        time_range,
    )
}

pub(super) async fn run_query(
    runtime: &ToolRuntime,
    org_id: &Id,
    language: QueryLanguage,
    statement: String,
    range: TimeRange,
    stream: Option<(&str, StreamType)>,
    limit: usize,
) -> Result<QueryResult> {
    runtime
        .observability
        .query
        .run(QueryRequest {
            org_id: org_id.clone(),
            language,
            statement,
            time_range: range,
            stream: stream.map(|(name, stream_type)| StreamHint {
                name: name.to_string(),
                stream_type,
            }),
            limit: Some(limit),
            step: None,
            federation_clusters: Vec::new(),
        })
        .await
}

/// A PromQL range query read every `step` microseconds, at most `limit` rows.
pub(super) async fn run_metric_range(
    runtime: &ToolRuntime,
    org_id: &Id,
    statement: String,
    range: TimeRange,
    step: i64,
    limit: usize,
) -> Result<QueryResult> {
    runtime
        .observability
        .query
        .run(QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Promql,
            statement,
            time_range: range,
            stream: None,
            limit: Some(limit),
            step: Some(step),
            federation_clusters: Vec::new(),
        })
        .await
}

pub(super) async fn run_optional_stream_query(
    runtime: &ToolRuntime,
    org_id: &Id,
    statement: String,
    range: TimeRange,
    stream: &str,
    stream_type: StreamType,
    limit: usize,
) -> Result<Option<QueryResult>> {
    match run_query(
        runtime,
        org_id,
        QueryLanguage::Sql,
        statement,
        range,
        Some((stream, stream_type)),
        limit,
    )
    .await
    {
        Ok(result) => Ok(Some(result)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(error)
            if error
                .to_string()
                .to_ascii_lowercase()
                .contains("stream not found") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn rows_as_objects(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| {
            Value::Object(
                result
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect(),
            )
        })
        .collect()
}

pub(super) fn query_result(result: QueryResult) -> ToolResult {
    ToolResult::json(serde_json::json!({
        "columns": result.columns,
        "rows": result.rows,
        "scannedRows": result.scanned_rows,
        "tookMs": result.took_ms,
        "federation": result.federation,
    }))
}

pub(super) fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(super) fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub(super) fn push_string_filter(filters: &mut Vec<String>, field: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        filters.push(format!("{} = '{}'", quote_ident(field), sql_literal(value)));
    }
}

pub(super) fn where_clause(filters: &[String]) -> String {
    if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    }
}

pub(super) fn schema_has(schema: &Schema, field: &str) -> bool {
    schema
        .fields
        .iter()
        .any(|candidate| candidate.name == field)
}

pub(super) async fn require_stream(
    runtime: &ToolRuntime,
    org_id: &Id,
    name: &str,
    stream_type: StreamType,
) -> Result<StreamDefinition> {
    // A tool call carries nobody's audit.read, and a schema or a field value of the audit trail
    // says as much as a row does.
    if !crate::infra::query::grants::may_read(name) {
        return Err(Error::forbidden(format!(
            "tools cannot read `{name}`: open it in Logs, which needs the audit.read permission"
        )));
    }

    let stream = runtime
        .observability
        .streams
        .get(org_id, name, stream_type)
        .await?;

    let settings = runtime
        .observability
        .streams
        .get_settings(&stream.id)
        .await?;

    if !settings.queryable {
        return Err(Error::forbidden("stream is not queryable"));
    }

    Ok(stream)
}

pub(super) fn require_field<'a>(schema: &'a Schema, field: &str) -> Result<&'a str> {
    schema
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .map(|field| field.name.as_str())
        .ok_or_else(|| Error::invalid(format!("field `{field}` is not present in stream schema")))
}

/// A stream with summaries first, as the trace list has it, then `default`, then by name.
fn traces_stream_order(stream: &StreamDefinition) -> (bool, bool, String) {
    (
        !carries_summaries(&stream.schema),
        stream.name != "default",
        stream.name.clone(),
    )
}

pub(super) async fn resolve_traces_stream(
    runtime: &ToolRuntime,
    org_id: &Id,
) -> Result<Option<StreamDefinition>> {
    let mut candidates = runtime
        .observability
        .streams
        .list(org_id)
        .await?
        .into_iter()
        .filter(|stream| {
            stream.stream_type == StreamType::TRACES && schema_has(&stream.schema, "trace_id")
        })
        .collect::<Vec<_>>();

    candidates.sort_by_key(traces_stream_order);

    for stream in candidates {
        if runtime
            .observability
            .streams
            .get_settings(&stream.id)
            .await?
            .queryable
        {
            return Ok(Some(stream));
        }
    }

    Ok(None)
}

pub(super) fn validate_trace_id(trace_id: &str) -> Result<()> {
    if trace_id.is_empty()
        || trace_id.len() > 128
        || !trace_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(Error::invalid("traceId must be 1..=128 safe characters"));
    }

    Ok(())
}

pub(super) fn is_internal_stream(name: &str) -> bool {
    name.starts_with('_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType},
        shared::{
            time::TimestampMicros,
            trace::summary::{
                TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
                TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD,
                TRACE_SUMMARY_START_NS_FIELD,
            },
        },
    };

    fn stream(name: &str, fields: &[&str]) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(name),
            org_id: Id::from_string("org-1"),
            name: name.to_string(),
            stream_type: StreamType::TRACES,
            schema: Schema {
                fields: fields
                    .iter()
                    .map(|name| FieldDef {
                        name: (*name).to_string(),
                        data_type: FieldType::Utf8,
                        nullable: true,
                        index_type: None,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    })
                    .collect(),
            },
            retention: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn a_traces_stream_with_summaries_comes_before_default() {
        let summarized = [
            TRACE_SUMMARY_MARKER_FIELD,
            TRACE_SUMMARY_START_NS_FIELD,
            TRACE_SUMMARY_DURATION_NS_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD,
            TRACE_SUMMARY_ERROR_COUNT_FIELD,
        ];

        let mut streams = [
            stream("checkout", &["trace_id"]),
            stream("default", &["trace_id"]),
            stream("prod", &summarized),
        ];

        streams.sort_by_key(traces_stream_order);

        let names: Vec<&str> = streams.iter().map(|stream| stream.name.as_str()).collect();

        assert_eq!(names, ["prod", "default", "checkout"]);
    }
}
