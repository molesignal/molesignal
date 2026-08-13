// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, JsonObject, ListToolsResult,
        PaginatedRequestParams, Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use serde_json::{Value, json};
use tool_runtime::{ToolInvocationContext, ToolSpec, catalog::BuiltinToolKind};

use super::handler::{InboundMcpHandler, protocol_error, request_data};

pub(super) mod catalog;
pub(super) mod execution;
mod idempotency;

pub(super) struct ResolvedTool {
    pub kind: BuiltinToolKind,
    pub spec: ToolSpec,
    pub invocation: ToolInvocationContext,
    pub timeout_ms: Option<i64>,
    pub max_response_bytes: Option<i64>,
}

pub(super) fn protocol_tool(name: &str) -> Option<Tool> {
    catalog::protocol_tool(name)
}

pub(super) async fn list(
    handler: &InboundMcpHandler,
    _request: Option<PaginatedRequestParams>,
    context: &RequestContext<RoleServer>,
) -> Result<ListToolsResult, ErrorData> {
    let request = request_data(handler, context)?;
    let tools = catalog::advertised(handler, &request.iam)
        .await
        .map_err(protocol_error)?;
    let response_too_large = match serde_json::to_vec(&tools) {
        Ok(encoded) => encoded.len() > request.settings.max_response_bytes.max(1) as usize,
        Err(_) => true,
    };
    if response_too_large {
        return Err(ErrorData::invalid_request(
            "tool list exceeds the configured response limit",
            None,
        ));
    }
    Ok(ListToolsResult {
        tools,
        ..Default::default()
    }
    .with_ttl_ms(0)
    .with_cache_scope(CacheScope::Private))
}

pub(super) async fn call(
    handler: &InboundMcpHandler,
    request: CallToolRequestParams,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResponse, ErrorData> {
    let data = request_data(handler, context)?;
    let name = request.name.as_ref();
    let arguments = Value::Object(request.arguments.clone().unwrap_or_default());
    execution::notify_progress(context, 0.0, "MoleSignal tool call started").await;
    let response = match name {
        "tool_search" => {
            reject_round_trip_fields(&request)?;
            let value = catalog::search(handler, &data.iam, arguments)
                .await
                .map_err(protocol_error)?;
            if serde_json::to_vec(&value).is_ok_and(|encoded| {
                encoded.len() <= data.settings.max_response_bytes.max(1) as usize
            }) {
                Ok(rmcp::model::CallToolResult::structured(value).into())
            } else {
                Ok(visible_error(
                    "tool_search response exceeds the configured response limit",
                ))
            }
        }
        "call_read_tool" => {
            reject_round_trip_fields(&request)?;
            execution::call_read(handler, context, &data, arguments).await
        }
        "call_managed_tool" => {
            reject_round_trip_fields(&request)?;
            idempotency::call_managed(handler, context, &data, arguments).await
        }
        "execute_agent_approval" => {
            idempotency::execute_approval(handler, context, &data, request).await
        }
        _ if catalog::is_direct(name) => {
            reject_round_trip_fields(&request)?;
            execution::call_direct(handler, context, &data, name, arguments).await
        }
        _ => Err(ErrorData::invalid_params(
            format!(
                "tool `{name}` is not directly exposed; discover it with tool_search and use the matching call adapter"
            ),
            None,
        )),
    };
    execution::notify_progress(context, 1.0, "MoleSignal tool call finished").await;
    response
}

fn reject_round_trip_fields(request: &CallToolRequestParams) -> Result<(), ErrorData> {
    if request.request_state.is_some() || request.input_responses.is_some() {
        Err(ErrorData::invalid_params(
            "requestState and inputResponses are only valid for an active execute_agent_approval confirmation",
            None,
        ))
    } else {
        Ok(())
    }
}

fn adapter_tool(
    name: &'static str,
    description: &'static str,
    schema: Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
) -> Tool {
    let input: JsonObject = schema.as_object().cloned().unwrap_or_default();
    Tool::new(name, description, Arc::new(input)).with_annotations(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(destructive)
            .idempotent(idempotent)
            .open_world(false),
    )
}

fn visible_error(error: impl std::fmt::Display) -> CallToolResponse {
    rmcp::model::CallToolResult::structured_error(json!({
        "error": "tool_execution_failed",
        "message": error.to_string(),
    }))
    .into()
}
