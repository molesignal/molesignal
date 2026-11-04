// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::super::{ToolRuntime, common::*};
use crate::{app::iam::IamContext, domain::stream::StreamType, shared::Result};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetStreamSettings => stream_settings(runtime, auth, arguments).await,
        _ => unreachable!("observability metadata executor received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
        "streamType": stream.stream_type,
        "retention": stream.retention,
        "settings": settings,
        "createdAt": stream.created_at,
        "updatedAt": stream.updated_at,
    })))
}
