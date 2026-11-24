// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `analyze_log_patterns`: the logs of one stream in a window, grouped into message patterns and
//! compared with a baseline stretch, read with the same stream checks as `query_logs`
//! ([`crate::app::log_patterns`] does the reading and grouping).

use domain::{iam::IamContext, stream::StreamType};
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{
    super::{ToolRuntime, common::*},
    validated_filter,
};
use crate::app::{
    log_patterns::{PatternQuery, analyze as analyze_patterns, level_field, message_field},
    query::baseline::Baseline,
};

const DEFAULT_LIMIT: usize = 30;
const MAX_LIMIT: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    stream: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    filter_sql: Option<String>,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    baseline: Baseline,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn analyze(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.stream, StreamType::LOGS).await?;
    let message = message_field(&stream.schema, &args.stream, args.field.as_deref())?;
    let level = level_field(&stream.schema, &message);

    let query = PatternQuery {
        org_id: &auth.org_id,
        stream: &args.stream,
        message: &message,
        level: level.as_deref(),
        filter: validated_filter(args.filter_sql.as_deref())?,
        window: time_range(args.time_range)?,
        baseline: args.baseline,
        limit: args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        tokens: false,
    };

    let report = analyze_patterns(&runtime.observability.query, &query).await?;
    let mut value =
        serde_json::to_value(report).map_err(|error| Error::internal(error.to_string()))?;

    if let Some(object) = value.as_object_mut() {
        object.insert(
            "next".into(),
            json!(format!(
                "Read logs of a pattern with query_logs, filtering {message} with LIKE \
                 '%<fragment>%', or with search_around at its lastSeen time."
            )),
        );
    }

    Ok(ToolResult::json(value))
}
