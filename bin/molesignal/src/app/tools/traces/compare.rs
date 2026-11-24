// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `compare_attribute_distributions`: how the slow or failed requests of a service differ from
//! its other requests, field by field, read as the comparison on the Traces page reads it.

use domain::{iam::IamContext, query::QueryLanguage, stream::StreamType};
use kernel::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::super::{ToolRuntime, common::*};
use crate::app::{
    attribute_diff::{Selection, SpanComparison, compare_spans},
    trace::reading::entry_clause,
};

/// The most rows read of each set.
const SAMPLE_ROWS: usize = 1_000;
const NANOS_PER_MILLI: f64 = 1_000_000.0;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompareArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    errors: bool,
    #[serde(default)]
    min_duration_ms: Option<f64>,
    #[serde(default)]
    max_duration_ms: Option<f64>,
    /// Every Span, not only those where a service took a request in.
    #[serde(default)]
    all_spans: bool,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn compare_attribute_distributions(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: CompareArgs = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(10).clamp(1, 20);
    let nanos = |millis: f64| (millis.max(0.0) * NANOS_PER_MILLI).round() as i64;

    let selection = Selection {
        min_duration: args.min_duration_ms.map(nanos),
        max_duration: args.max_duration_ms.map(nanos),
        errors: args.errors,
    };

    selection.check()?;

    let Some(stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(ToolResult::json(json!({"streamAvailable": false})));
    };

    let mut clauses = Vec::new();

    if !args.all_spans {
        clauses.extend(entry_clause(&stream.schema));
    }

    push_string_filter(&mut clauses, "service.name", args.service.as_deref());
    push_string_filter(&mut clauses, "name", args.operation.as_deref());

    let asked = SpanComparison {
        definition: &stream,
        clauses,
        selection: &selection,
        limit: SAMPLE_ROWS,
    };

    let run = |statement: String| {
        run_query(
            runtime,
            &auth.org_id,
            QueryLanguage::Sql,
            statement,
            range,
            Some((stream.name.as_str(), StreamType::TRACES)),
            SAMPLE_ROWS,
        )
    };

    let mut comparison = compare_spans(asked, run).await?;
    comparison.dimensions.truncate(limit);

    Ok(ToolResult::json(json!({
        "streamAvailable": true,
        "stream": stream.name,
        "timeRange": {"startTime": range.start.0, "endTime": range.end.0},
        "comparison": comparison,
        "next": "A score near 1 means the chosen requests and the others hardly share values of \
                 that field. Narrow list_traces or query_logs to the value the chosen requests \
                 crowd into to read examples.",
    })))
}
