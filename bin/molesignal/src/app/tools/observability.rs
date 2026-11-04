// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*};
use crate::{
    app::iam::IamContext,
    domain::{query::QueryLanguage, stream::StreamType},
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod metadata;
mod metrics;

const DEFAULT_LIMIT: usize = 500;
const MAX_LIMIT: usize = 5_000;
/// Steps a metric query is read at when the caller names no step.
const DEFAULT_METRIC_STEPS: i64 = 100;
const SEARCH_AROUND_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::QueryLogs => query_logs(runtime, auth, arguments).await,
        BuiltinToolKind::QueryMetrics => query_metrics(runtime, auth, arguments).await,
        BuiltinToolKind::ListStreams => list_streams(runtime, auth, arguments).await,
        BuiltinToolKind::GetStreamSchema => get_stream_schema(runtime, auth, arguments).await,
        BuiltinToolKind::GetStreamSettings => {
            metadata::execute(runtime, auth, kind, arguments).await
        }
        BuiltinToolKind::ListMetricNames
        | BuiltinToolKind::ListMetricLabelValues
        | BuiltinToolKind::ListMetricLabels
        | BuiltinToolKind::ListMetricSeries => {
            metrics::execute(runtime, auth, kind, arguments).await
        }
        BuiltinToolKind::SearchAround => search_around(runtime, auth, arguments).await,
        BuiltinToolKind::SearchFieldValues => search_field_values(runtime, auth, arguments).await,
        _ => unreachable!("observability handler received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryLogsArgs {
    sql: String,
    stream: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    limit: Option<usize>,
}

async fn query_logs(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: QueryLogsArgs = parse_args(arguments)?;
    require_stream(runtime, &auth.org_id, &args.stream, StreamType::LOGS).await?;

    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        args.sql,
        time_range(args.time_range)?,
        Some((&args.stream, StreamType::LOGS)),
        args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    )
    .await?;

    Ok(query_result(result))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryMetricsArgs {
    promql: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    step_seconds: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn query_metrics(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: QueryMetricsArgs = parse_args(arguments)?;

    if args.promql.to_ascii_lowercase().contains("label_values(") {
        return Ok(ToolResult::error(
            "label_values() is not PromQL; use list_metric_label_values",
        ));
    }

    let range = time_range(args.time_range)?;
    let step = match args.step_seconds {
        Some(seconds) => i64::try_from(seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000_000),
        None => (range.end.0.saturating_sub(range.start.0) / DEFAULT_METRIC_STEPS).max(1_000_000),
    };

    let result = run_metric_range(
        runtime,
        &auth.org_id,
        args.promql,
        range,
        step,
        args.limit.unwrap_or(1_000).clamp(1, MAX_LIMIT),
    )
    .await?;

    Ok(query_result(result))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListStreamsArgs {
    #[serde(default)]
    stream_type: Option<StreamType>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_streams(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListStreamsArgs = parse_args(arguments)?;
    let limit = args.limit.unwrap_or(200).clamp(1, 1_000);

    let mut streams = runtime
        .observability
        .streams
        .list(&auth.org_id)
        .await?
        .into_iter()
        .filter(|stream| {
            !is_internal_stream(&stream.name)
                && args
                    .stream_type
                    .is_none_or(|want| stream.stream_type == want)
        })
        .collect::<Vec<_>>();

    streams.sort_by(|left, right| {
        left.stream_type
            .as_str()
            .cmp(right.stream_type.as_str())
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut rows = Vec::new();

    for stream in streams {
        let settings = runtime
            .observability
            .streams
            .get_settings(&stream.id)
            .await?;

        if settings.queryable {
            rows.push(json!({
                "name": stream.name,
                "streamType": stream.stream_type,
                "fieldCount": stream.schema.fields.len(),
                "retention": stream.retention,
                "description": settings.description,
            }));
        }

        if rows.len() == limit {
            break;
        }
    }

    Ok(ToolResult::json(json!({"streams": rows, "limit": limit})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StreamArgs {
    stream: String,
    stream_type: StreamType,
}

async fn get_stream_schema(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: StreamArgs = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.stream, args.stream_type).await?;

    let settings = runtime
        .observability
        .streams
        .get_settings(&stream.id)
        .await?;

    Ok(ToolResult::json(json!({
        "name": stream.name,
        "streamType": stream.stream_type,
        "schema": stream.schema,
        "retention": stream.retention,
        "settings": settings,
        "createdAt": stream.created_at,
        "updatedAt": stream.updated_at,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchAroundArgs {
    stream: String,
    anchor_time: i64,
    #[serde(default)]
    before: Option<usize>,
    #[serde(default)]
    after: Option<usize>,
    #[serde(default)]
    filter_sql: Option<String>,
}

async fn search_around(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SearchAroundArgs = parse_args(arguments)?;
    require_stream(runtime, &auth.org_id, &args.stream, StreamType::LOGS).await?;
    let before = args.before.unwrap_or(25).clamp(1, 250);
    let after = args.after.unwrap_or(25).clamp(1, 250);
    let extra = validated_filter(args.filter_sql.as_deref())?;
    let stream = quote_ident(&args.stream);

    let before_sql = format!(
        "SELECT * FROM {stream} WHERE _timestamp <= {}{extra} ORDER BY _timestamp DESC LIMIT {before}",
        args.anchor_time,
    );

    let after_sql = format!(
        "SELECT * FROM {stream} WHERE _timestamp > {}{extra} ORDER BY _timestamp ASC LIMIT {after}",
        args.anchor_time,
    );

    let range = TimeRange::new(
        TimestampMicros(args.anchor_time.saturating_sub(SEARCH_AROUND_WINDOW_MICROS)),
        TimestampMicros(args.anchor_time.saturating_add(SEARCH_AROUND_WINDOW_MICROS)),
    );

    let mut preceding = rows_as_objects(
        &run_query(
            runtime,
            &auth.org_id,
            QueryLanguage::Sql,
            before_sql,
            range,
            Some((&args.stream, StreamType::LOGS)),
            before.max(1),
        )
        .await?,
    );

    preceding.reverse();

    let following = rows_as_objects(
        &run_query(
            runtime,
            &auth.org_id,
            QueryLanguage::Sql,
            after_sql,
            range,
            Some((&args.stream, StreamType::LOGS)),
            after.max(1),
        )
        .await?,
    );

    Ok(ToolResult::json(json!({
        "stream": args.stream, "anchorTime": args.anchor_time,
        "before": preceding, "after": following,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchFieldValuesArgs {
    stream: String,
    field: String,
    #[serde(default = "logs_stream_type")]
    stream_type: StreamType,
    time_range: TimeRangeArg,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn logs_stream_type() -> StreamType {
    StreamType::LOGS
}

async fn search_field_values(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SearchFieldValuesArgs = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.stream, args.stream_type).await?;
    let field = require_field(&stream.schema, &args.field)?;
    let mut filters = vec![format!("{} IS NOT NULL", quote_ident(field))];

    if let Some(query) = args
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        filters.push(format!(
            "CAST({} AS VARCHAR) LIKE '%{}%'",
            quote_ident(field),
            sql_literal(query)
        ));
    }

    let limit = args.limit.unwrap_or(50).clamp(1, 500);
    let field = quote_ident(field);

    let statement = format!(
        "SELECT {field} AS value, COUNT(*) AS count FROM {stream}{} GROUP BY {field} ORDER BY count DESC LIMIT {limit}",
        where_clause(&filters),
        stream = quote_ident(&args.stream),
    );

    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        time_range(args.time_range)?,
        Some((&args.stream, args.stream_type)),
        limit,
    )
    .await?;

    Ok(ToolResult::json(json!({
        "stream": args.stream, "field": args.field,
        "values": rows_as_objects(&result),
        "scannedRows": result.scanned_rows, "tookMs": result.took_ms,
    })))
}

fn validated_filter(value: Option<&str>) -> Result<String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };

    let normalized = value.to_ascii_lowercase();

    if value.len() > 2_000
        || [
            ";", "--", "/*", "select ", " from ", " union ", " order ", " limit ",
        ]
        .iter()
        .any(|blocked| normalized.contains(blocked))
    {
        return Err(Error::invalid(
            "filterSql must be a single boolean expression",
        ));
    }

    Ok(format!(" AND ({value})"))
}
