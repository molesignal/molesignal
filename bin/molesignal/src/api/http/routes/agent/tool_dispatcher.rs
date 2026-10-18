// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的薄 Tool Runtime adapter。
//!
//! 本层只负责 Agent Profile/Toolset、延迟工具发现、出站 MCP 和审计。所有内置工具
//! 的权限与执行均委托给 `app::tools::ToolRuntime`，未来入站 MCP adapter 可直接复用后者。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::{Value, json};
pub(crate) use session::{Reply, RunInteraction};
use tool_runtime::{ToolCallSource, ToolContent, ToolInvocationContext};
pub(crate) use workspace::ChatWorkspace;

use crate::{
    agent::{
        model::{RiskLevel, ToolCallRecord},
        tool_control::ToolExecutionMode,
        tools::{
            BuiltinToolKind, ToolAuthContext, ToolCall, ToolDispatcher, ToolResult,
            is_builtin_tool, is_meta_tool, is_session_tool, is_skill_tool, is_workspace_tool,
            risk_for_tool,
        },
    },
    api::AppState,
    app::skills::SessionSkills,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod meta;
mod session;
mod skills;
mod workspace;

const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;

pub struct RealToolDispatcher {
    state: AppState,
    resolution: super::toolsets::ToolsetResolution,
    /// 这个会话能用的 Skill：Skill 的三个工具只在这里面找。
    skills: Arc<SessionSkills>,
    /// 和发起这一轮的人说话：任务清单、提问与「确认」模式的确认。
    interaction: Option<Arc<dyn RunInteraction>>,
    /// 会话在知识、记忆与调查上的位置。
    workspace: Option<Arc<ChatWorkspace>>,
}

impl RealToolDispatcher {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            resolution: super::toolsets::ToolsetResolution::default(),
            skills: Arc::new(SessionSkills::default()),
            interaction: None,
            workspace: None,
        }
    }

    pub(crate) fn with_workspace(mut self, workspace: ChatWorkspace) -> Self {
        self.workspace = Some(Arc::new(workspace));
        self
    }

    pub(crate) fn with_interaction(mut self, interaction: Arc<dyn RunInteraction>) -> Self {
        self.interaction = Some(interaction);
        self
    }

    pub fn with_toolsets(mut self, resolution: super::toolsets::ToolsetResolution) -> Self {
        self.resolution = resolution;
        self
    }

    pub fn with_skills(mut self, skills: Arc<SessionSkills>) -> Self {
        self.skills = skills;
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

        if is_skill_tool(&call.name) {
            if !self.resolution.builtin_enabled(&call.name) {
                return Ok(ToolResult::error(format!(
                    "tool `{}` is not enabled for this organization",
                    call.name
                )));
            }

            let Some(kind) = BuiltinToolKind::from_name(&call.name) else {
                return Ok(ToolResult::error(format!("unknown tool: {}", call.name)));
            };

            let result = skills::execute(&self.state, &self.skills, kind, call.arguments).await?;
            return Ok(self.enforce_response_limit(kind.name(), result));
        }

        let Some(kind) = BuiltinToolKind::from_name(&call.name) else {
            return self.execute_mcp(ctx, call).await;
        };

        if is_session_tool(&call.name) && self.resolution.builtin_enabled(&call.name) {
            return session::execute(self.interaction.as_deref(), kind, call.arguments).await;
        }

        if is_workspace_tool(&call.name) && self.resolution.builtin_enabled(&call.name) {
            let interaction = self.interaction.as_deref();
            let workspace = self.workspace.as_ref();

            return workspace::execute(
                &self.state,
                ctx,
                workspace,
                interaction,
                kind,
                call.arguments,
            )
            .await;
        }

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

        // 「确认」模式：提问人在对话里同意后照自动执行。「自动」不问可以撤回的改动，bypass
        // permissions 什么都不问。
        let mut execution_mode = self.resolution.execution_mode_for_builtin(&call.name);

        if execution_mode == ToolExecutionMode::Confirmation {
            let spec = kind.spec();

            if !ctx
                .execution_policy()
                .skips_confirmation(spec.annotations.destructive)
                && let Err(result) = session::confirmation(
                    self.interaction.as_deref(),
                    &call.name,
                    &call.arguments,
                    spec.risk,
                )
                .await
            {
                return Ok(result);
            }

            execution_mode = ToolExecutionMode::Automatic;
        }

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

    async fn execute_mcp(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
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

        // 标了 destructiveHint 的工具导入时风险至少是 L4，到不了「确认」：能确认的都不是破坏性的。
        if execution_mode == ToolExecutionMode::Confirmation {
            if !ctx.execution_policy().skips_confirmation(false)
                && let Err(result) = session::confirmation(
                    self.interaction.as_deref(),
                    &call.name,
                    &call.arguments,
                    tool.risk,
                )
                .await
            {
                return Ok(result);
            }
        } else if execution_mode != ToolExecutionMode::Automatic {
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

        // 成员凭据与数据权限都按发这条消息的人。
        self.state
            .agent
            .mcp
            .execute_tool(
                &server,
                tool,
                &Id(ctx.principal_id().to_string()),
                call.arguments,
            )
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
