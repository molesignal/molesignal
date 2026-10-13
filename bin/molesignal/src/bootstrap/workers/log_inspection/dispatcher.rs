// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检的 Tool Runtime adapter：automation 入口。
//!
//! 没有人在场审批，所以这个入口比 Mole Agent 聊天窄得多：只有 catalog 里显式声明了
//! automation surface 的只读工具，只能读这次巡检的那一个数据流，结果大小有上限。组织的
//! Tool Policy 仍然生效：被停用、或被改成需要确认的工具，巡检同样用不了。权限与执行都交给
//! `app::tools::ToolRuntime`，与另外两个入口走的是同一处。

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::{Value, json};
use tool_runtime::{ToolCallSource, ToolInvocationContext};

use crate::{
    agent::{
        chat::Provider,
        tool_control::{ToolControlRepository, ToolExecutionMode, ToolPolicy, ToolPolicyDefaults},
        tools::{
            BuiltinToolKind, ToolAuthContext, ToolCall, ToolDispatcher, ToolResult, ToolSurface,
            tools_for_surface,
        },
    },
    app::{log_inspection::ToolCallLog, tools::ToolRuntime},
    shared::{Error, Result, ids::Id},
};

/// 只读工具默认的单次超时，与 Mole Agent 聊天一致。
const DEFAULT_TOOL_TIMEOUT_MS: i64 = 30_000;

/// 单个工具结果的上限。一次巡检最多八轮工具调用，每个结果都会再发给模型一遍：这个数限定
/// 的是一次巡检能塞给模型多少日志。
const MAX_RESPONSE_BYTES: i64 = 65_536;

/// 组织对工具的管控，读一次用一整次巡检。
pub(super) struct OrgToolPolicy {
    policies: HashMap<String, ToolPolicy>,
    /// `l0` … `l4` → 执行模式。
    risk_modes: Value,
}

impl OrgToolPolicy {
    pub(super) async fn load(
        tool_control: &dyn ToolControlRepository,
        org_id: &Id,
    ) -> Result<Self> {
        let policies = tool_control
            .list_policies(org_id)
            .await?
            .into_iter()
            .map(|policy| (policy.tool_name.clone(), policy))
            .collect();

        let defaults = tool_control
            .get_policy_defaults(org_id)
            .await?
            .unwrap_or_else(|| {
                ToolPolicyDefaults::system_defaults(org_id.clone(), Id::from_string("system"))
            });

        Ok(Self {
            policies,
            risk_modes: defaults.risk_modes,
        })
    }

    /// 这个工具在组织里按什么模式执行：它自己的策略优先，否则按风险等级的默认。
    fn execution_mode(&self, kind: BuiltinToolKind) -> ToolExecutionMode {
        if let Some(policy) = self.policies.get(kind.name()) {
            return policy.execution_mode;
        }

        let risk = kind.spec().risk;
        let key = format!("{risk:?}").to_ascii_lowercase();

        self.risk_modes
            .get(&key)
            .cloned()
            .and_then(|mode| serde_json::from_value::<ToolExecutionMode>(mode).ok())
            .filter(|mode| mode.allowed_for_risk(risk))
            .unwrap_or_else(|| ToolExecutionMode::default_for_risk(risk))
    }

    /// 没人审批的巡检只能用「自动执行」的工具。
    fn allows(&self, kind: BuiltinToolKind) -> bool {
        self.policies
            .get(kind.name())
            .is_none_or(|policy| policy.enabled)
            && self.execution_mode(kind) == ToolExecutionMode::Automatic
    }

    fn timeout(&self, kind: BuiltinToolKind) -> Duration {
        let millis = self
            .policies
            .get(kind.name())
            .map_or(DEFAULT_TOOL_TIMEOUT_MS, |policy| policy.timeout_ms);

        Duration::from_millis(u64::try_from(millis.max(1)).unwrap_or(u64::MAX))
    }

    fn max_response_bytes(&self, kind: BuiltinToolKind) -> i64 {
        self.policies
            .get(kind.name())
            .map_or(MAX_RESPONSE_BYTES, |policy| {
                policy.max_response_bytes.min(MAX_RESPONSE_BYTES)
            })
    }

    /// 给模型的工具清单，按 provider 期望的格式：automation 入口上、组织没有停用的那些。
    pub(super) fn tools_schema(&self, provider: Provider) -> Value {
        let mut tools = tools_for_surface(ToolSurface::Automation)
            .into_iter()
            .filter(|tool| {
                BuiltinToolKind::from_name(&tool.name).is_some_and(|kind| self.allows(kind))
            })
            .collect::<Vec<_>>();

        tools.sort_by(|left, right| left.name.cmp(&right.name));

        tools
            .into_iter()
            .map(|tool| match provider {
                Provider::Anthropic => json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                }),
                Provider::OpenAi | Provider::OpenAiCompatible => json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                }),
            })
            .collect()
    }
}

pub(super) struct AutomationToolDispatcher {
    tools: Arc<ToolRuntime>,
    policy: OrgToolPolicy,
    /// 这次巡检只读这一个数据流。
    stream: String,
    calls: Arc<ToolCallLog>,
}

impl AutomationToolDispatcher {
    pub(super) fn new(
        tools: Arc<ToolRuntime>,
        policy: OrgToolPolicy,
        stream: String,
        calls: Arc<ToolCallLog>,
    ) -> Self {
        Self {
            tools,
            policy,
            stream,
            calls,
        }
    }

    async fn dispatch_inner(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        let Some(kind) = BuiltinToolKind::from_name(&call.name) else {
            return Ok(ToolResult::error(format!("unknown tool: {}", call.name)));
        };

        if !kind.spec().exposure.available_on(ToolSurface::Automation) {
            return Ok(ToolResult::error(format!(
                "tool `{}` is not available to a scheduled inspection",
                call.name
            )));
        }

        if !self.policy.allows(kind) {
            return Ok(ToolResult::error(format!(
                "tool `{}` is switched off or needs approval in this organization",
                call.name
            )));
        }

        // 模型拿到的任务只说了一个数据流；它要是去读别的，这里拦下。
        if call.arguments.get("stream").and_then(Value::as_str) != Some(self.stream.as_str()) {
            return Ok(ToolResult::error(format!(
                "this inspection reads only the stream `{}`",
                self.stream
            )));
        }

        let invocation = ToolInvocationContext::from_iam(ctx.iam_context())
            .with_source(ToolCallSource::Automation)
            .with_execution_policy(ctx.execution_policy())
            .with_execution_mode(self.policy.execution_mode(kind));

        let result = tokio::time::timeout(
            self.policy.timeout(kind),
            self.tools.execute(&invocation, kind, call.arguments),
        )
        .await
        .map_err(|_| Error::unavailable(format!("tool `{}` timed out", kind.name())))??;

        let limit = self.policy.max_response_bytes(kind);
        let size = i64::try_from(result.serialized_len()).unwrap_or(i64::MAX);

        if size > limit {
            return Ok(ToolResult::error(format!(
                "the result is {size} bytes and a scheduled inspection reads at most {limit} per \
                 call: count and group instead of listing, select fewer fields, or lower the \
                 limit"
            )));
        }

        Ok(result)
    }
}

#[async_trait]
impl ToolDispatcher for AutomationToolDispatcher {
    #[tracing::instrument(
        name = "gen_ai.tool",
        skip_all,
        fields(otel.kind = "internal", gen_ai.tool.name = %call.name)
    )]
    async fn dispatch(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        let name = call.name.clone();
        let outcome = self.dispatch_inner(ctx, call).await;

        // 报告页只列出调用过哪些工具、成没成，不留查询结果。
        self.calls
            .record(&name, matches!(&outcome, Ok(result) if !result.is_error));

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn policy_for(tool: &str, enabled: bool, mode: ToolExecutionMode) -> ToolPolicy {
        ToolPolicy {
            org_id: Id::from_string("org-1"),
            tool_name: tool.into(),
            enabled,
            execution_mode: mode,
            environment_overrides: json!({}),
            timeout_ms: 5_000,
            max_calls_per_run: 8,
            max_response_bytes: 1_048_576,
            updated_by: Id::from_string("admin"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn org_policy(policies: Vec<ToolPolicy>, risk_modes: Value) -> OrgToolPolicy {
        OrgToolPolicy {
            policies: policies
                .into_iter()
                .map(|policy| (policy.tool_name.clone(), policy))
                .collect(),
            risk_modes,
        }
    }

    fn names(schema: &Value) -> Vec<&str> {
        schema
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| {
                tool.get("name")
                    .or_else(|| tool.pointer("/function/name"))
                    .and_then(Value::as_str)
            })
            .collect()
    }

    #[test]
    fn an_inspection_is_offered_the_read_only_log_tools_in_the_shape_of_its_provider() {
        let policy = org_policy(Vec::new(), json!({ "l0": "automatic" }));

        let openai = policy.tools_schema(Provider::OpenAi);
        assert_eq!(
            names(&openai),
            ["get_stream_schema", "query_logs", "search_field_values"]
        );
        assert_eq!(openai[0]["type"], "function");
        assert!(openai[0]["function"]["parameters"].is_object());

        let anthropic = policy.tools_schema(Provider::Anthropic);
        assert_eq!(names(&anthropic), names(&openai));
        assert!(anthropic[0]["input_schema"].is_object());
    }

    #[test]
    fn a_tool_the_organization_switched_off_or_put_behind_approval_is_not_offered() {
        let policy = org_policy(
            vec![
                policy_for("query_logs", false, ToolExecutionMode::Automatic),
                policy_for("search_field_values", true, ToolExecutionMode::Confirmation),
            ],
            json!({ "l0": "automatic" }),
        );

        assert_eq!(
            names(&policy.tools_schema(Provider::OpenAi)),
            ["get_stream_schema"]
        );
        assert!(!policy.allows(BuiltinToolKind::QueryLogs));
        assert!(!policy.allows(BuiltinToolKind::SearchFieldValues));
        assert!(policy.allows(BuiltinToolKind::GetStreamSchema));
    }

    #[test]
    fn an_organization_that_makes_reads_wait_for_confirmation_leaves_nothing_to_inspect_with() {
        let policy = org_policy(Vec::new(), json!({ "l0": "confirmation" }));

        assert!(names(&policy.tools_schema(Provider::OpenAi)).is_empty());
    }

    #[test]
    fn a_tool_policy_can_shorten_the_timeout_and_the_result_but_not_raise_the_cap() {
        let policy = org_policy(
            vec![policy_for("query_logs", true, ToolExecutionMode::Automatic)],
            json!({}),
        );

        assert_eq!(
            policy.timeout(BuiltinToolKind::QueryLogs),
            Duration::from_secs(5)
        );
        assert_eq!(
            policy.timeout(BuiltinToolKind::GetStreamSchema),
            Duration::from_secs(30)
        );

        // 组织策略允许 1 MiB，巡检的上限更小。
        assert_eq!(
            policy.max_response_bytes(BuiltinToolKind::QueryLogs),
            MAX_RESPONSE_BYTES
        );

        // 默认的风险策略里，只读工具自动执行。
        assert!(policy.allows(BuiltinToolKind::GetStreamSchema));
    }
}
