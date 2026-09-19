// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的薄 Tool Runtime adapter。
//!
//! 本层只负责 Agent Profile/Toolset、延迟工具发现、出站 MCP 和审计。所有内置工具
//! 的权限与执行均委托给 `app::tools::ToolRuntime`，未来入站 MCP adapter 可直接复用后者。

use std::{
    cmp::Reverse,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolCallSource, ToolContent, ToolInvocationContext};

use crate::{
    agent::{
        model::{RiskLevel, ToolCallRecord},
        tool_control::ToolExecutionMode,
        tools::{
            BuiltinToolKind, ToolAuthContext, ToolCall, ToolDispatcher, ToolResult, builtin_tools,
            is_builtin_tool, is_meta_tool, risk_for_tool,
        },
    },
    api::AppState,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;

pub struct RealToolDispatcher {
    state: AppState,
    resolution: super::toolsets::ToolsetResolution,
}

impl RealToolDispatcher {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            resolution: super::toolsets::ToolsetResolution::default(),
        }
    }

    pub fn with_toolsets(mut self, resolution: super::toolsets::ToolsetResolution) -> Self {
        self.resolution = resolution;
        self
    }
}

#[async_trait]
impl ToolDispatcher for RealToolDispatcher {
    #[tracing::instrument(
        name = "gen_ai.tool",
        skip_all,
        fields(otel.kind = "internal", gen_ai.tool.name = %call.name)
    )]
    async fn dispatch(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        let tool_name = call.name.clone();
        let input = redact_sensitive_value(&call.arguments);
        let started = Instant::now();
        let outcome = self.dispatch_inner(ctx, call).await;
        self.audit(ctx, &tool_name, input, started, &outcome)
            .await?;
        outcome
    }
}

impl RealToolDispatcher {
    async fn dispatch_inner(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        if !self
            .state
            .platform
            .license
            .has_feature(crate::agent::FEATURE)
        {
            return Err(Error::forbidden(
                "Mole Agent tools require the agent feature",
            ));
        }
        if ctx.is_query_generation_only() {
            return Ok(ToolResult::error(
                "tool calls are disabled while query-generation-only mode is active",
            ));
        }
        if is_meta_tool(&call.name) {
            if !self.resolution.builtin_enabled(&call.name) {
                return Ok(ToolResult::error(format!(
                    "meta tool `{}` is disabled by policy",
                    call.name
                )));
            }
            return match BuiltinToolKind::from_name(&call.name) {
                Some(BuiltinToolKind::ToolSearch) => self.tool_search(call.arguments),
                Some(BuiltinToolKind::ToolsCall) => self.tools_call(ctx, call.arguments).await,
                _ => unreachable!(),
            };
        }
        let org_id = Id(ctx.org_id().to_string());
        let Some(kind) = BuiltinToolKind::from_name(&call.name) else {
            return self.execute_mcp(ctx, &org_id, call).await;
        };
        if !kind.spec().exposure.mole_agent {
            return Ok(ToolResult::error(format!(
                "tool `{}` is not exposed to Mole Agent",
                call.name
            )));
        }
        if !self.resolution.builtin_enabled(&call.name) {
            return Ok(ToolResult::error(format!(
                "tool `{}` is not enabled for this organization",
                call.name
            )));
        }
        let execution_mode = self.resolution.execution_mode_for_builtin(&call.name);
        let invocation = ToolInvocationContext::from_iam(ctx.iam_context())
            .with_source(ToolCallSource::MoleAgent)
            .with_chat(ctx.chat_id().map(str::to_string))
            .with_investigation(ctx.investigation_id().map(str::to_string))
            .with_execution_policy(ctx.execution_policy())
            .with_execution_mode(execution_mode)
            .query_generation_only(ctx.is_query_generation_only());
        let execution =
            super::builtin_execution::execute(&self.state, &invocation, kind, call.arguments);
        let result = if kind.spec().annotations.read_only {
            let timeout_ms = self
                .resolution
                .tool_policy(kind.name())
                .map(|policy| policy.timeout_ms)
                .unwrap_or(30_000);
            let timeout_ms = u64::try_from(timeout_ms.max(1)).unwrap_or(u64::MAX);
            tokio::time::timeout(Duration::from_millis(timeout_ms), execution)
                .await
                .map_err(|_| Error::unavailable(format!("tool `{}` timed out", kind.name())))??
        } else {
            execution.await?
        };
        Ok(self.enforce_response_limit(kind.name(), result))
    }

    async fn execute_mcp(
        &self,
        ctx: &ToolAuthContext,
        org_id: &Id,
        call: ToolCall,
    ) -> Result<ToolResult> {
        let Some(tool) = self.resolution.mcp_tools.get(&call.name) else {
            return Ok(ToolResult::error(format!("unknown tool: {}", call.name)));
        };
        if self.resolution.mcp_tool(&call.name).is_none() {
            return Ok(ToolResult::error(format!(
                "MCP tool `{}` is disabled, unavailable, or blocked by the active Agent Profile",
                call.name
            )));
        }
        if !ctx.execution_policy().allows_approval_request() && tool.risk != RiskLevel::L0 {
            return Ok(ToolResult::error(format!(
                "MCP tool `{}` is blocked by the current execution policy",
                call.name
            )));
        }
        let execution_mode = self.resolution.execution_mode_for_mcp(tool);
        if execution_mode != ToolExecutionMode::Automatic {
            return Ok(ToolResult::error(format!(
                "MCP tool `{}` requires `{}` and cannot execute directly",
                call.name,
                execution_mode_label(execution_mode)
            )));
        }
        let mut server = self
            .resolution
            .mcp_servers
            .get(&tool.server_id.0)
            .cloned()
            .ok_or_else(|| Error::internal("MCP tool references a missing server"))?;
        if let Some(policy) = self.resolution.tool_policy(&call.name) {
            server.timeout_ms = server.timeout_ms.min(policy.timeout_ms);
            server.max_response_bytes = server.max_response_bytes.min(policy.max_response_bytes);
        }
        super::mcp::execute_tool(&self.state, org_id, &server, tool, call.arguments).await
    }

    fn tool_search(&self, arguments: Value) -> Result<ToolResult> {
        let args: ToolSearchArgs = serde_json::from_value(arguments)
            .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;
        let query = args.query.unwrap_or_default().trim().to_ascii_lowercase();
        let domain = args.domain.as_deref().map(str::to_ascii_lowercase);
        let mut entries = builtin_tools()
            .into_iter()
            .filter(|tool| {
                tool.exposure.mole_agent
                    && !is_meta_tool(&tool.name)
                    && self.resolution.builtin_enabled(&tool.name)
            })
            .filter(|tool| domain.as_ref().is_none_or(|domain| tool.domain.eq_ignore_ascii_case(domain)))
            .filter_map(|tool| {
                let score = search_score(
                    &query,
                    &tool.name,
                    &tool.display_name,
                    &tool.description,
                    &tool.domain,
                    &tool.tags,
                )?;
                Some((score, json!({
                    "name": tool.name, "display_name": tool.display_name,
                    "description": tool.description,
                    "domain": tool.domain, "category": tool.category, "tags": tool.tags,
                    "required_permissions": tool.required_permissions,
                    "permission_mode": tool.permission_mode,
                    "execution_mode": self.resolution.execution_mode_for_builtin(&tool.name),
                    "input_schema": args.include_schema.unwrap_or(true).then_some(tool.input_schema),
                    "output_schema": args.include_schema.unwrap_or(true).then_some(tool.output_schema),
                })))
            })
            .collect::<Vec<_>>();
        entries.extend(
            self.resolution
                .mcp_tools
                .values()
                .filter(|tool| self.resolution.mcp_tool(&tool.name).is_some())
                .filter(|_| domain.as_ref().is_none_or(|domain| domain == "mcp"))
                .filter_map(|tool| {
                    let score = search_score(
                        &query,
                        &tool.name,
                        &tool.display_name,
                        &tool.description,
                        "mcp",
                        &tool.tags,
                    )?;
                    Some((score, json!({
                        "name": tool.name, "display_name": tool.display_name,
                        "description": tool.description, "tags": tool.tags,
                        "risk": tool.risk, "source": "mcp",
                        "execution_mode": self.resolution.execution_mode_for_mcp(tool),
                        "input_schema": args.include_schema.unwrap_or(true).then_some(tool.input_schema.clone()),
                        "output_schema": args.include_schema.unwrap_or(true).then_some(tool.output_schema.clone()),
                    })))
                }),
        );
        entries.sort_by_key(|(score, value)| {
            (
                Reverse(*score),
                value["name"].as_str().unwrap_or_default().to_string(),
            )
        });
        let limit = args.limit.unwrap_or(10).clamp(1, 50);
        entries.truncate(limit);
        Ok(ToolResult::json(json!({
            "query": query, "tools": entries.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
        })))
    }

    async fn tools_call(&self, ctx: &ToolAuthContext, arguments: Value) -> Result<ToolResult> {
        let args: ToolsCallArgs = serde_json::from_value(arguments)
            .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;
        if is_meta_tool(&args.name) {
            return Ok(ToolResult::error("meta tools cannot call themselves"));
        }
        Box::pin(self.dispatch(
            ctx,
            ToolCall {
                name: args.name,
                arguments: args.arguments,
            },
        ))
        .await
    }

    fn enforce_response_limit(&self, name: &str, result: ToolResult) -> ToolResult {
        let max_response_bytes = self
            .resolution
            .tool_policy(name)
            .map(|policy| policy.max_response_bytes)
            .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
        if i64::try_from(result.serialized_len()).unwrap_or(i64::MAX) > max_response_bytes {
            ToolResult::error(format!(
                "tool `{name}` response exceeded the configured {} byte limit",
                max_response_bytes
            ))
        } else {
            result
        }
    }

    async fn audit(
        &self,
        ctx: &ToolAuthContext,
        tool_name: &str,
        input: Value,
        started: Instant,
        outcome: &Result<ToolResult>,
    ) -> Result<()> {
        let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let (status, error) = match outcome {
            Ok(result) if result.is_error => ("error", result.first_text()),
            Ok(_) => ("success", None),
            Err(error) => ("error", Some(error.to_string())),
        };
        let output_summary = outcome.as_ref().ok().and_then(redacted_result_summary);
        let risk = risk_for_tool(tool_name)
            .or_else(|| {
                self.resolution
                    .mcp_tools
                    .get(tool_name)
                    .map(|tool| tool.risk)
            })
            .unwrap_or(RiskLevel::L4);
        let execution_mode = if is_builtin_tool(tool_name) {
            self.resolution.execution_mode_for_builtin(tool_name)
        } else {
            self.resolution
                .mcp_tools
                .get(tool_name)
                .map(|tool| self.resolution.execution_mode_for_mcp(tool))
                .unwrap_or(ToolExecutionMode::Disabled)
        };
        self.state
            .agent
            .repository
            .record_tool_call(ToolCallRecord {
                id: Id::new(),
                org_id: Id(ctx.org_id().to_string()),
                chat_id: ctx.chat_id().map(|value| Id(value.to_string())),
                investigation_id: ctx.investigation_id().map(|value| Id(value.to_string())),
                step_id: None,
                tool_name: tool_name.to_string(),
                risk,
                input,
                output_summary,
                status: status.into(),
                error,
                duration_ms,
                called_by: Id(ctx.principal_id().to_string()),
                call_source: if ctx.investigation_id().is_some() {
                    "investigation"
                } else if ctx.chat_id().is_some() {
                    "chat"
                } else {
                    "manual_test"
                }
                .into(),
                profile_id: self.resolution.active_profile_id.clone(),
                approval_id: None,
                policy_decision: json!({
                    "enabled": true, "execution_mode": execution_mode,
                    "network_access": self.resolution.network_access,
                    "chat_execution_policy": ctx.execution_policy(),
                }),
                audit_id: None,
                created_at: TimestampMicros::now(),
            })
            .await
            .map(|_| ())
    }
}

fn redacted_result_summary(result: &ToolResult) -> Option<String> {
    result.content.iter().find_map(|content| {
        let value = match content {
            ToolContent::Text { text } => Value::String(text.clone()),
            ToolContent::Json { json } => json.clone(),
        };
        serde_json::to_string(&redact_sensitive_value(&value))
            .ok()
            .map(|text| text.chars().take(1_000).collect())
    })
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_schema: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsCallArgs {
    name: String,
    arguments: Value,
}

fn search_score(
    query: &str,
    name: &str,
    display_name: &str,
    description: &str,
    domain: &str,
    tags: &[String],
) -> Option<u16> {
    if query.is_empty() {
        return Some(1);
    }
    let name = name.to_ascii_lowercase();
    if name == query {
        return Some(1_000);
    }
    let display_name = display_name.to_ascii_lowercase();
    let description = description.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    query.split_whitespace().try_fold(0u16, |score, token| {
        let token_score = u16::from(name.contains(token)) * 300
            + u16::from(display_name.contains(token)) * 160
            + u16::from(domain.contains(token)) * 120
            + u16::from(
                tags.iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(token)),
            ) * 100
            + u16::from(description.contains(token)) * 40;
        (token_score > 0).then_some(score.saturating_add(token_score))
    })
}

fn execution_mode_label(mode: ToolExecutionMode) -> &'static str {
    match mode {
        ToolExecutionMode::Automatic => "automatic",
        ToolExecutionMode::Confirmation => "confirmation",
        ToolExecutionMode::SingleApproval => "single_approval",
        ToolExecutionMode::DualApproval => "dual_approval",
        ToolExecutionMode::Disabled => "disabled",
    }
}

pub(crate) fn redact_sensitive_value(value: &Value) -> Value {
    const SENSITIVE_PARTS: [&str; 7] = [
        "token",
        "password",
        "secret",
        "authorization",
        "cookie",
        "api_key",
        "credential",
    ];
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if SENSITIVE_PARTS
                        .iter()
                        .any(|part| key.to_ascii_lowercase().contains(part))
                    {
                        Value::String("<redacted>".into())
                    } else {
                        redact_sensitive_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_sensitive_value).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_prefers_exact_name_and_matches_tags() {
        assert_eq!(
            search_score("get_trace", "get_trace", "", "", "", &[]),
            Some(1_000)
        );
        assert!(search_score("traces", "x", "", "", "", &["Traces".into()]).is_some());
        assert_eq!(search_score("missing", "x", "", "", "", &[]), None);
    }

    #[test]
    fn audit_redaction_is_recursive() {
        let value = json!({"nested": {"api_key": "secret"}, "safe": [1, 2]});
        assert_eq!(
            redact_sensitive_value(&value)["nested"]["api_key"],
            "<redacted>"
        );
        assert_eq!(redact_sensitive_value(&value)["safe"], json!([1, 2]));
    }

    #[test]
    fn tool_result_summary_never_persists_one_time_credentials() {
        let result = ToolResult::json(json!({
            "one_time_result": {"api_token": {"token": "ms_prefix_secret"}},
            "execution": {"status": "succeeded"}
        }));
        let summary = redacted_result_summary(&result).expect("summary");
        assert!(!summary.contains("ms_prefix_secret"));
        assert!(summary.contains("<redacted>"));
    }
}
