// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metric discovery tools. A metrics stream holds the metrics of one source, told apart by
//! `metric_name`; the metric directory names the streams holding each metric, and every query
//! here reads those streams with the metric's name as a condition.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::super::{ToolRuntime, common::*};
use crate::{
    app::iam::IamContext,
    domain::{
        metrics::METRIC_NAME_FIELD,
        query::QueryLanguage,
        stream::{StreamDefinition, StreamType},
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListMetricNames => metric_names(runtime, auth, arguments).await,
        BuiltinToolKind::ListMetricLabelValues => {
            metric_label_values(runtime, auth, arguments).await
        }
        BuiltinToolKind::ListMetricLabels => metric_labels(runtime, auth, arguments).await,
        BuiltinToolKind::ListMetricSeries => metric_series(runtime, auth, arguments).await,
        _ => unreachable!("metric tool executor received unrelated tool"),
    }
}

/// A queryable stream holding the metric, with the labels the metric carried there.
struct MetricStream {
    stream: StreamDefinition,
    labels: Vec<String>,
}

/// The queryable streams holding `metric`, in an hour that `range` touches when one is given.
/// A metric no stream holds is not found; one only unqueryable streams hold is refused.
async fn metric_streams(
    runtime: &ToolRuntime,
    org_id: &Id,
    metric: &str,
    range: Option<TimeRange>,
) -> Result<Vec<MetricStream>> {
    let entries = runtime
        .observability
        .metric_directory
        .find(org_id, metric)
        .await?;

    if entries.is_empty() {
        return Err(Error::not_found(format!("metric `{metric}` not found")));
    }

    let mut streams = Vec::new();
    let mut refused = false;

    for entry in entries
        .into_iter()
        .filter(|entry| range.is_none_or(|range| entry.overlaps(range)))
    {
        let stream = match runtime
            .observability
            .streams
            .get(org_id, &entry.stream_name, StreamType::METRICS)
            .await
        {
            Ok(stream) => stream,
            Err(Error::NotFound(_)) => continue,
            Err(error) => return Err(error),
        };

        if !runtime
            .observability
            .streams
            .get_settings(&stream.id)
            .await?
            .queryable
        {
            refused = true;
            continue;
        }

        streams.push(MetricStream {
            stream,
            labels: entry.labels,
        });
    }

    if streams.is_empty() && refused {
        return Err(Error::forbidden("stream is not queryable"));
    }

    Ok(streams)
}

fn labels_of(streams: &[MetricStream]) -> Vec<String> {
    streams
        .iter()
        .flat_map(|stream| stream.labels.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn metric_condition(metric: &str) -> String {
    format!(
        "{} = '{}'",
        quote_ident(METRIC_NAME_FIELD),
        sql_literal(metric)
    )
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricNamesArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn metric_names(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: MetricNamesArgs = parse_args(arguments)?;
    let query = args.query.as_deref().map(str::to_ascii_lowercase);
    let limit = args.limit.unwrap_or(200).clamp(1, 1_000);

    let mut metrics = runtime
        .observability
        .metric_directory
        .list(&auth.org_id)
        .await?
        .into_iter()
        .filter(|entry| {
            !is_internal_stream(&entry.stream_name)
                && query
                    .as_ref()
                    .is_none_or(|query| entry.metric_name.to_ascii_lowercase().contains(query))
        })
        .map(|entry| entry.metric_name)
        .collect::<Vec<_>>();

    metrics.sort();
    metrics.dedup();
    metrics.truncate(limit);
    Ok(ToolResult::json(json!({"metricNames": metrics})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricLabelValuesArgs {
    metric: String,
    label: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn metric_label_values(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: MetricLabelValuesArgs = parse_args(arguments)?;
    let range = time_range(args.time_range)?;
    let streams = metric_streams(runtime, &auth.org_id, &args.metric, Some(range)).await?;
    let limit = args.limit.unwrap_or(200).clamp(1, 1_000);

    let query = args
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // By the value's JSON text: the value and how many samples carry it in every stream.
    let mut counts = BTreeMap::<String, (Value, i64)>::new();
    let mut labelled = false;
    let mut scanned_rows = 0;
    let mut took_ms = 0;

    for MetricStream { stream, .. } in &streams {
        let Ok(field) = require_field(&stream.schema, &args.label) else {
            continue;
        };

        labelled = true;

        let mut filters = vec![
            metric_condition(&args.metric),
            format!("{} IS NOT NULL", quote_ident(field)),
        ];

        if let Some(query) = query {
            filters.push(format!(
                "CAST({} AS VARCHAR) LIKE '%{}%'",
                quote_ident(field),
                sql_literal(query)
            ));
        }

        let statement = format!(
            "SELECT {field} AS value, COUNT(*) AS count FROM {stream}{where_clause} GROUP BY {field} ORDER BY count DESC LIMIT {limit}",
            field = quote_ident(field),
            stream = quote_ident(&stream.name),
            where_clause = where_clause(&filters),
        );

        let result = run_query(
            runtime,
            &auth.org_id,
            QueryLanguage::Sql,
            statement,
            range,
            Some((&stream.name, StreamType::METRICS)),
            limit,
        )
        .await?;

        scanned_rows += result.scanned_rows;
        took_ms += result.took_ms;

        for row in rows_as_objects(&result) {
            let value = row.get("value").cloned().unwrap_or(Value::Null);
            let count = row.get("count").and_then(Value::as_i64).unwrap_or_default();
            let entry = counts.entry(value.to_string()).or_insert((value, 0));
            entry.1 += count;
        }
    }

    if !labelled {
        return Err(Error::invalid(format!(
            "field `{}` is not present in stream schema",
            args.label
        )));
    }

    let mut values = counts.into_values().collect::<Vec<_>>();
    values.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    values.truncate(limit);

    Ok(ToolResult::json(json!({
        "metric": args.metric,
        "label": args.label,
        "values": values
            .into_iter()
            .map(|(value, count)| json!({"value": value, "count": count}))
            .collect::<Vec<_>>(),
        "scannedRows": scanned_rows,
        "tookMs": took_ms,
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
    let streams = metric_streams(runtime, &auth.org_id, &args.metric, None).await?;

    Ok(ToolResult::json(json!({
        "metric": args.metric,
        "labels": labels_of(&streams),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    let range = time_range(args.time_range)?;
    let streams = metric_streams(runtime, &auth.org_id, &args.metric, Some(range)).await?;
    let labels = labels_of(&streams);
    let limit = args.limit.unwrap_or(200).clamp(1, 1_000);

    if labels.is_empty() {
        return Ok(ToolResult::json(json!({
            "metric": args.metric,
            "series": [{"__name__": args.metric}],
        })));
    }

    if let Some(label) = args
        .match_labels
        .keys()
        .find(|label| !labels.contains(label))
    {
        return Err(Error::invalid(format!(
            "field `{label}` is not a metric label"
        )));
    }

    let mut series = Vec::new();
    let mut scanned_rows = 0;
    let mut took_ms = 0;

    for MetricStream { stream, .. } in &streams {
        let present = labels
            .iter()
            .filter(|label| schema_has(&stream.schema, label))
            .map(|label| quote_ident(label))
            .collect::<Vec<_>>();

        // A stream without one of the matched labels holds no series that match.
        if series.len() >= limit
            || present.is_empty()
            || args
                .match_labels
                .keys()
                .any(|label| !schema_has(&stream.schema, label))
        {
            continue;
        }

        let mut filters = vec![metric_condition(&args.metric)];

        for (label, value) in &args.match_labels {
            filters.push(format!("{} = '{}'", quote_ident(label), sql_literal(value)));
        }

        let statement = format!(
            "SELECT DISTINCT {} FROM {}{} LIMIT {}",
            present.join(", "),
            quote_ident(&stream.name),
            where_clause(&filters),
            limit - series.len(),
        );

        let result = run_query(
            runtime,
            &auth.org_id,
            QueryLanguage::Sql,
            statement,
            range,
            Some((&stream.name, StreamType::METRICS)),
            limit,
        )
        .await?;

        scanned_rows += result.scanned_rows;
        took_ms += result.took_ms;

        series.extend(rows_as_objects(&result).into_iter().map(|mut row| {
            if let Value::Object(fields) = &mut row {
                fields.insert("__name__".into(), Value::String(args.metric.clone()));
            }

            row
        }));
    }

    Ok(ToolResult::json(json!({
        "metric": args.metric,
        "labels": labels,
        "series": series,
        "scannedRows": scanned_rows,
        "tookMs": took_ms,
    })))
}
