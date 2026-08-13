// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::super::{ToolRuntime, common::*};
use crate::{
    app::iam::IamContext,
    domain::{
        metrics::{is_metric_identity_storage_field, is_prometheus_exemplar_storage_field},
        query::QueryLanguage,
        stream::{FieldType, StreamType},
    },
    shared::Result,
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetStreamSettings => stream_settings(runtime, auth, arguments).await,
        BuiltinToolKind::ListMetricLabels => metric_labels(runtime, auth, arguments).await,
        BuiltinToolKind::ListMetricSeries => metric_series(runtime, auth, arguments).await,
        _ => unreachable!("observability metadata executor received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamArgs {
    stream: String,
    stream_type: StreamType,
}

async fn stream_settings(
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
        "stream": stream.name,
        "stream_type": stream.stream_type,
        "retention": stream.retention,
        "settings": settings,
        "created_at": stream.created_at,
        "updated_at": stream.updated_at,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricArgs {
    metric: String,
}

async fn metric_labels(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: MetricArgs = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.metric, StreamType::Metrics).await?;
    let labels = label_names(&stream.schema);
    Ok(ToolResult::json(json!({
        "metric": args.metric,
        "labels": labels,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricSeriesArgs {
    metric: String,
    time_range: TimeRangeArg,
    #[serde(default, rename = "match")]
    match_labels: BTreeMap<String, String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn metric_series(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: MetricSeriesArgs = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.metric, StreamType::Metrics).await?;
    let labels = label_names(&stream.schema);
    let limit = args.limit.unwrap_or(200).clamp(1, 1_000);
    if labels.is_empty() {
        return Ok(ToolResult::json(json!({
            "metric": args.metric,
            "series": [{"__name__": args.metric}],
        })));
    }
    let mut filters = Vec::new();
    for (label, value) in &args.match_labels {
        let field = require_field(&stream.schema, label)?;
        if !labels.iter().any(|candidate| candidate == field) {
            return Err(crate::shared::Error::invalid(format!(
                "field `{label}` is not a metric label"
            )));
        }
        filters.push(format!("{} = '{}'", quote_ident(field), sql_literal(value)));
    }
    let selected = labels
        .iter()
        .map(|label| quote_ident(label))
        .collect::<Vec<_>>();
    let statement = format!(
        "SELECT DISTINCT {} FROM {}{} LIMIT {limit}",
        selected.join(", "),
        quote_ident(&args.metric),
        where_clause(&filters),
    );
    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        time_range(args.time_range)?,
        Some((&args.metric, StreamType::Metrics)),
        limit,
    )
    .await?;
    let series = rows_as_objects(&result)
        .into_iter()
        .map(|mut row| {
            if let Value::Object(fields) = &mut row {
                fields.insert("__name__".into(), Value::String(args.metric.clone()));
            }
            row
        })
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({
        "metric": args.metric,
        "labels": labels,
        "series": series,
        "scanned_rows": result.scanned_rows,
        "took_ms": result.took_ms,
    })))
}

fn label_names(schema: &crate::domain::stream::Schema) -> Vec<String> {
    let mut labels = schema
        .fields
        .iter()
        .filter(|field| field.data_type == FieldType::Utf8)
        .map(|field| field.name.clone())
        .filter(|name| {
            name != "value"
                && name != "_timestamp"
                && !is_metric_identity_storage_field(name)
                && !is_prometheus_exemplar_storage_field(name)
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}
