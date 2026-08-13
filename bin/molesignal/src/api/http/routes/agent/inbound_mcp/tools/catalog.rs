// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{cmp::Reverse, sync::Arc};

use rmcp::model::{JsonObject, Tool, ToolAnnotations as McpAnnotations};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{
    ExecutionPolicy, ToolAccess, ToolCallSource, ToolExecutionMode, ToolInvocationContext,
    ToolSpec, ToolSurface,
    catalog::{BuiltinToolKind, tools_for_surface},
};

use super::{ResolvedTool, adapter_tool};
use crate::{
    api::http::routes::agent::{inbound_mcp::handler::InboundMcpHandler, toolsets},
    shared::{Error, Result},
};

pub(super) const ADAPTER_NAMES: [&str; 3] = ["tool_search", "call_read_tool", "call_managed_tool"];

const DIRECT_CONTROL_NAMES: [&str; 5] = [
    "list_agent_approvals",
    "get_agent_approval",
    "execute_agent_approval",
    "list_agent_executions",
    "get_agent_execution",
];

pub(super) fn is_direct(name: &str) -> bool {
    BuiltinToolKind::from_name(name).is_some_and(|kind| {
        let spec = kind.spec();
        spec.exposure.available_on(ToolSurface::InboundMcp)
            && (spec.exposure.pinned || DIRECT_CONTROL_NAMES.contains(&name))
    })
}

pub(super) fn protocol_tool(name: &str) -> Option<Tool> {
    match name {
        "tool_search" => Some(adapter_tool(
            "tool_search",
            "Search all Inbound MCP tools allowed by IAM and the organization Tool Policy.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "domain": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 10},
                    "include_schema": {"type": "boolean", "default": true}
                },
                "additionalProperties": false
            }),
            true,
            false,
            true,
        )),
        "call_read_tool" => Some(adapter_tool(
            "call_read_tool",
            "Call one read-only or preflight tool discovered with tool_search.",
            json!({
                "type": "object",
                "required": ["name", "arguments"],
                "properties": {
                    "name": {"type": "string"},
                    "arguments": {"type": "object"},
                    "as_task": {"type": "boolean", "default": false}
                },
                "additionalProperties": false
            }),
            true,
            false,
            true,
        )),
        "call_managed_tool" => Some(adapter_tool(
            "call_managed_tool",
            "Call one managed change tool. idempotency_key is required and is reserved before any approval is created.",
            json!({
                "type": "object",
                "required": ["name", "arguments", "idempotency_key"],
                "properties": {
                    "name": {"type": "string"},
                    "arguments": {"type": "object"},
                    "idempotency_key": {"type": "string", "minLength": 1, "maxLength": 128}
                },
                "additionalProperties": false
            }),
            false,
            true,
            true,
        )),
        _ if is_direct(name) => BuiltinToolKind::from_name(name).map(|kind| spec_tool(kind.spec())),
        _ => None,
    }
}

pub(super) async fn advertised(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
) -> Result<Vec<Tool>> {
    let resolution = toolsets::resolve_inbound_tool_policies(&handler.state, &iam.org_id).await?;
    let mut tools = ADAPTER_NAMES
        .into_iter()
        .filter_map(protocol_tool)
        .collect::<Vec<_>>();
    for spec in tools_for_surface(ToolSurface::InboundMcp)
        .into_iter()
        .filter(|spec| spec.exposure.pinned || DIRECT_CONTROL_NAMES.contains(&spec.name.as_str()))
    {
        if !resolution.builtin_enabled(&spec.name) {
            continue;
        }
        let Some(kind) = BuiltinToolKind::from_name(&spec.name) else {
            continue;
        };
        let mode = effective_execution_mode(&resolution, &spec);
        let invocation = ToolInvocationContext::from_iam(iam)
            .with_source(ToolCallSource::InboundMcp)
            .with_execution_policy(ExecutionPolicy::Policy)
            .with_execution_mode(mode);
        if handler.state.tools.authorize(&invocation, kind).is_ok() {
            tools.push(spec_tool(spec));
        }
    }
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(tools)
}

pub(in crate::api::http::routes::agent::inbound_mcp) async fn resolve(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
    name: &str,
) -> Result<ResolvedTool> {
    let kind = BuiltinToolKind::from_name(name)
        .ok_or_else(|| Error::not_found(format!("tool `{name}` is not registered")))?;
    let spec = kind.spec();
    if !spec.exposure.available_on(ToolSurface::InboundMcp) {
        return Err(Error::forbidden(format!(
            "tool `{name}` is not exposed on Inbound MCP"
        )));
    }
    let resolution = toolsets::resolve_inbound_tool_policies(&handler.state, &iam.org_id).await?;
    if !resolution.builtin_enabled(name) {
        return Err(Error::forbidden(format!(
            "tool `{name}` is disabled by the organization Tool Policy"
        )));
    }
    let mode = effective_execution_mode(&resolution, &spec);
    let invocation = ToolInvocationContext::from_iam(iam)
        .with_source(ToolCallSource::InboundMcp)
        .with_execution_policy(ExecutionPolicy::Policy)
        .with_execution_mode(mode);
    handler.state.tools.authorize(&invocation, kind)?;
    let policy = resolution.tool_policy(name);
    Ok(ResolvedTool {
        kind,
        spec,
        invocation,
        timeout_ms: policy.map(|value| value.timeout_ms),
        max_response_bytes: policy.map(|value| value.max_response_bytes),
    })
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_schema: Option<bool>,
}

pub(super) async fn search(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
    arguments: Value,
) -> Result<Value> {
    let args: SearchArgs = serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool_search arguments: {error}")))?;
    let query = args.query.unwrap_or_default();
    let domain = args.domain;
    if query.len() > 256 || domain.as_ref().is_some_and(|value| value.len() > 128) {
        return Err(Error::invalid(
            "tool_search query must not exceed 256 bytes and domain must not exceed 128 bytes",
        ));
    }
    let query = query.trim().to_ascii_lowercase();
    let domain = domain.map(|value| value.trim().to_ascii_lowercase());
    let resolution = toolsets::resolve_inbound_tool_policies(&handler.state, &iam.org_id).await?;
    let mut matches = Vec::new();
    for spec in tools_for_surface(ToolSurface::InboundMcp) {
        if ADAPTER_NAMES.contains(&spec.name.as_str()) {
            continue;
        }
        if domain
            .as_ref()
            .is_some_and(|domain| !spec.domain.eq_ignore_ascii_case(domain))
        {
            continue;
        }
        if !resolution.builtin_enabled(&spec.name) {
            continue;
        }
        let Some(kind) = BuiltinToolKind::from_name(&spec.name) else {
            continue;
        };
        let mode = effective_execution_mode(&resolution, &spec);
        let invocation = ToolInvocationContext::from_iam(iam)
            .with_source(ToolCallSource::InboundMcp)
            .with_execution_policy(ExecutionPolicy::Policy)
            .with_execution_mode(mode);
        if handler.state.tools.authorize(&invocation, kind).is_err() {
            continue;
        }
        let Some(score) = search_score(&query, &spec) else {
            continue;
        };
        matches.push((
            score,
            json!({
                "name": spec.name,
                "title": spec.display_name,
                "description": spec.description,
                "domain": spec.domain,
                "category": spec.category,
                "tags": spec.tags,
                "risk": spec.risk,
                "access": spec.access,
                "execution_mode": mode,
                "required_permissions": spec.required_permissions,
                "input_schema": args.include_schema.unwrap_or(true).then_some(spec.input_schema),
                "output_schema": args.include_schema.unwrap_or(true).then_some(spec.output_schema),
            }),
        ));
    }
    matches.sort_by_key(|(score, value)| {
        (
            Reverse(*score),
            value["name"].as_str().unwrap_or_default().to_string(),
        )
    });
    matches.truncate(args.limit.unwrap_or(10).clamp(1, 50));
    Ok(json!({
        "query": query,
        "tools": matches.into_iter().map(|(_, value)| value).collect::<Vec<_>>()
    }))
}

pub(in crate::api::http::routes::agent::inbound_mcp) async fn resource_catalog(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
) -> Result<Value> {
    let resolution = toolsets::resolve_inbound_tool_policies(&handler.state, &iam.org_id).await?;
    let mut entries = Vec::new();
    for spec in tools_for_surface(ToolSurface::InboundMcp) {
        if !resolution.builtin_enabled(&spec.name) {
            continue;
        }
        let Some(kind) = BuiltinToolKind::from_name(&spec.name) else {
            continue;
        };
        let mode = effective_execution_mode(&resolution, &spec);
        let invocation = ToolInvocationContext::from_iam(iam)
            .with_source(ToolCallSource::InboundMcp)
            .with_execution_policy(ExecutionPolicy::Policy)
            .with_execution_mode(mode);
        if handler.state.tools.authorize(&invocation, kind).is_ok() {
            entries.push(json!({
                "name": spec.name,
                "title": spec.display_name,
                "description": spec.description,
                "domain": spec.domain,
                "category": spec.category,
                "tags": spec.tags,
                "risk": spec.risk,
                "access": spec.access,
                "execution_mode": mode,
                "required_permissions": spec.required_permissions,
                "input_schema": spec.input_schema,
                "output_schema": spec.output_schema,
            }));
        }
    }
    entries.sort_by_key(|value| value["name"].as_str().unwrap_or_default().to_string());
    Ok(json!({
        "tools": entries,
        "direct_tools": advertised(handler, iam).await?.into_iter().map(|tool| tool.name).collect::<Vec<_>>(),
        "agent_profile_applied": false,
        "outbound_mcp_proxied": false,
    }))
}

fn effective_execution_mode(
    resolution: &toolsets::ToolsetResolution,
    spec: &ToolSpec,
) -> ToolExecutionMode {
    if spec.access == ToolAccess::ExecutesApprovedOperation {
        // The adapter has already enforced either MCP-host confirmation or the
        // required MoleSignal reviews. Applying another approval mode here
        // would make the approval lifecycle impossible to close.
        ToolExecutionMode::Automatic
    } else {
        resolution.execution_mode_for_builtin(&spec.name)
    }
}

fn search_score(query: &str, spec: &ToolSpec) -> Option<u16> {
    if query.is_empty() {
        return Some(1);
    }
    let name = spec.name.to_ascii_lowercase();
    if name == query {
        return Some(1_000);
    }
    let title = spec.display_name.to_ascii_lowercase();
    let description = spec.description.to_ascii_lowercase();
    let domain = spec.domain.to_ascii_lowercase();
    query.split_whitespace().try_fold(0u16, |score, token| {
        let token_score = u16::from(name.contains(token)) * 300
            + u16::from(title.contains(token)) * 160
            + u16::from(domain.contains(token)) * 120
            + u16::from(
                spec.tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(token)),
            ) * 100
            + u16::from(description.contains(token)) * 40;
        (token_score > 0).then_some(score.saturating_add(token_score))
    })
}

pub(super) fn spec_tool(spec: ToolSpec) -> Tool {
    let input = object(spec.input_schema);
    let output = object(spec.output_schema);
    Tool::new(spec.name, spec.description, Arc::new(input))
        .with_title(spec.display_name.clone())
        .with_raw_output_schema(Arc::new(output))
        .with_annotations(
            McpAnnotations::with_title(spec.display_name)
                .read_only(spec.annotations.read_only)
                .destructive(spec.annotations.destructive)
                .idempotent(spec.annotations.idempotent)
                .open_world(spec.annotations.open_world),
        )
}

fn object(value: Value) -> JsonObject {
    value.as_object().cloned().unwrap_or_else(|| {
        json!({"type": "object", "additionalProperties": true})
            .as_object()
            .cloned()
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_lookup_never_reveals_deferred_tools_as_direct_tools() {
        assert!(protocol_tool("tool_search").is_some());
        assert!(protocol_tool("query_logs").is_some());
        assert!(protocol_tool("create_alert_rule").is_none());
        assert!(protocol_tool("create_api_token").is_none());
    }

    #[test]
    fn approved_execution_is_not_wrapped_in_a_second_approval_policy() {
        let resolution = toolsets::ToolsetResolution {
            default_risk_modes: json!({"l1": "dual_approval"}),
            ..Default::default()
        };
        let spec = BuiltinToolKind::ExecuteAgentApproval.spec();
        assert_eq!(
            effective_execution_mode(&resolution, &spec),
            ToolExecutionMode::Automatic
        );
    }
}
