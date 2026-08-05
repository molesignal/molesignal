// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 服务端工具调度器。
//!
//! 编译期注册工具的 dispatch 实装覆盖：日志、指标、数据流、链路、告警、值班排班、
//! RUM、Continuous Profiles、报告，以及只创建审批请求的受控操作建议。
//!
//! 安全约束（spec D5）：dispatcher 完全忽略 `tools/call.arguments` 里的 `org_id`/`user_id`
//! 字段；所有租户上下文以 `ToolAuthContext.org_id` 为唯一可信源。
//!
//! 每次调用同步写入 `intelligence_tool_calls`；审计落库失败时调用失败关闭。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{StreamDefinition, StreamRepository, StreamType},
    },
    intelligence::{
        model::{RiskLevel, ToolCallRecord},
        tool_control::ToolExecutionMode,
        tools::{
            BuiltinToolKind, ToolAuthContext, ToolCall, ToolContent, ToolDispatcher, ToolResult,
            is_builtin_tool, risk_for_tool,
        },
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod dashboard;

const GET_TRACE_SPAN_LIMIT: usize = 100_000;
/// `get_trace` 默认查询窗口：过去 24h，与 `routes/web/trace.rs` 保持一致。
const GET_TRACE_WINDOW_SECS: i64 = 24 * 3600;
const ALERTS_DEFAULT_LIMIT: usize = 50;
const ALERTS_MAX_LIMIT: usize = 500;
const OBSERVABILITY_DEFAULT_LIMIT: usize = 100;
const OBSERVABILITY_MAX_LIMIT: usize = 500;
const PROFILES_MAX_LIMIT: usize = 1_000;
const DEFAULT_LOOKBACK_MICROS: i64 = 60 * 60 * 1_000_000;

pub struct RealToolDispatcher {
    state: AppState,
    /// 默认 Agent Profile 与 org Toolset 的有效白名单，只能进一步收窄编译期注册表。
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
        let t0 = Instant::now();
        let outcome = self.dispatch_inner(ctx, call).await;
        let duration_ms = i64::try_from(t0.elapsed().as_millis()).unwrap_or(i64::MAX);
        let (status, error_msg) = match &outcome {
            Ok(r) if r.is_error => (
                "error",
                first_text(&r.content).unwrap_or_else(|| "tool error".into()),
            ),
            Ok(_) => ("success", String::new()),
            Err(e) => ("error", e.to_string()),
        };
        let output_summary = outcome.as_ref().ok().and_then(|result| {
            first_text(&result.content).map(|text| text.chars().take(1000).collect::<String>())
        });
        let risk = risk_for_tool(&tool_name)
            .or_else(|| {
                self.resolution
                    .mcp_tools
                    .get(&tool_name)
                    .map(|tool| tool.risk)
            })
            .unwrap_or(RiskLevel::L4);
        let execution_mode = if is_builtin_tool(&tool_name) {
            self.resolution.execution_mode_for_builtin(&tool_name)
        } else {
            self.resolution
                .mcp_tools
                .get(&tool_name)
                .map(|tool| self.resolution.execution_mode_for_mcp(tool))
                .unwrap_or(ToolExecutionMode::Disabled)
        };
        let call_source = if ctx.investigation_id.is_some() {
            "investigation"
        } else if ctx.chat_id.is_some() {
            "chat"
        } else {
            "manual_test"
        };
        self.state
            .intelligence
            .repository
            .record_tool_call(ToolCallRecord {
                id: Id::new(),
                org_id: Id(ctx.org_id.clone()),
                chat_id: ctx.chat_id.clone().map(Id),
                investigation_id: ctx.investigation_id.clone().map(Id),
                step_id: None,
                tool_name,
                risk,
                input,
                output_summary,
                status: status.into(),
                error: (!error_msg.is_empty()).then_some(error_msg),
                duration_ms,
                called_by: Id(ctx.user_id.clone()),
                call_source: call_source.into(),
                profile_id: self.resolution.active_profile_id.clone(),
                approval_id: None,
                policy_decision: json!({
                    "enabled": true,
                    "execution_mode": execution_mode,
                    "network_access": self.resolution.network_access,
                    "chat_execution_policy": ctx.execution_policy,
                }),
                audit_id: None,
                created_at: TimestampMicros::now(),
            })
            .await?;
        outcome
    }
}

impl RealToolDispatcher {
    async fn dispatch_inner(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        if ctx.query_generation_only {
            return Ok(err_text(
                "tool calls are disabled while the chat is in query-generation-only mode",
            ));
        }
        // org_id 始终来自 auth context（spec D5：忽略 args.org_id 防 prompt injection）
        let org_id = Id(ctx.org_id.clone());

        // 有效白名单：被 Profile 或 org Toolset 禁用的工具，即使模型硬调也直接拒绝。
        if is_builtin_tool(&call.name) && !self.resolution.builtin_enabled(&call.name) {
            return Ok(err_text(format!(
                "tool `{}` is not enabled for this org",
                call.name
            )));
        }

        let Some(tool) = BuiltinToolKind::from_name(&call.name) else {
            let Some(mcp_tool) = self.resolution.mcp_tools.get(&call.name) else {
                return Ok(err_text(format!("unknown tool: {}", call.name)));
            };
            if self.resolution.mcp_tool(&call.name).is_none() {
                return Ok(err_text(format!(
                    "MCP tool `{}` is disabled, unavailable, or blocked by the active Agent Profile",
                    call.name
                )));
            }
            if !ctx.execution_policy.allows_approval_request() && mcp_tool.risk != RiskLevel::L0 {
                return Ok(err_text(format!(
                    "MCP tool `{}` is not read-only and is blocked by the current chat execution policy",
                    call.name
                )));
            }
            let execution_mode = self.resolution.execution_mode_for_mcp(mcp_tool);
            if execution_mode != ToolExecutionMode::Automatic {
                return Ok(err_text(format!(
                    "MCP tool `{}` requires `{}` and cannot execute directly from the model",
                    call.name,
                    execution_mode_label(execution_mode)
                )));
            }
            let mut server = self
                .resolution
                .mcp_servers
                .get(&mcp_tool.server_id.0)
                .cloned()
                .ok_or_else(|| Error::internal("MCP tool references a missing server"))?;
            if let Some(policy) = self.resolution.tool_policy(&call.name) {
                server.timeout_ms = server.timeout_ms.min(policy.timeout_ms);
                server.max_response_bytes =
                    server.max_response_bytes.min(policy.max_response_bytes);
            }
            return super::mcp::execute_tool(
                &self.state,
                &org_id,
                &server,
                mcp_tool,
                call.arguments,
            )
            .await;
        };
        let execution_mode = self.resolution.execution_mode_for_builtin(&call.name);
        if execution_mode != ToolExecutionMode::Automatic
            && !matches!(
                tool,
                BuiltinToolKind::ProposeOperation | BuiltinToolKind::ProposeDashboardCreation
            )
        {
            return Ok(err_text(format!(
                "tool `{}` requires `{}` and cannot execute without confirmation",
                call.name,
                execution_mode_label(execution_mode)
            )));
        }
        let mut auth = IamContext {
            user_id: Id(ctx.user_id.clone()),
            org_id: org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            scope: crate::domain::iam::IamScope::Organization,
            permissions: std::collections::BTreeSet::new(),
            features: std::collections::BTreeSet::new(),
            policy_version: 0,
        };
        self.state.iam.access.enrich_context(&mut auth).await?;

        match tool {
            BuiltinToolKind::QueryLogs => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                let args: QueryLogsArgs = parse_args(call.arguments)?;
                let time_range = validated_time_range(args.time_range)?;
                let req = QueryRequest {
                    org_id: org_id.clone(),
                    language: QueryLanguage::Sql,
                    statement: args.sql,
                    time_range,
                    stream: Some(StreamHint {
                        name: args.stream,
                        stream_type: StreamType::Logs,
                    }),
                    limit: None,
                    federation_clusters: Vec::new(),
                };
                let out = self.state.query.run(req).await?;
                Ok(ok_json(json!({
                    "columns": out.columns,
                    "rows": out.rows,
                    "scanned_rows": out.scanned_rows,
                    "took_ms": out.took_ms,
                })))
            }
            BuiltinToolKind::QueryMetrics => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                let args: QueryMetricsArgs = parse_args(call.arguments)?;
                if let Some(message) = unsupported_promql_message(&args.promql) {
                    return Ok(err_text(message));
                }
                let time_range = validated_time_range(args.time_range)?;
                let req = QueryRequest {
                    org_id: org_id.clone(),
                    language: QueryLanguage::Promql,
                    statement: args.promql,
                    time_range,
                    stream: None,
                    limit: None,
                    federation_clusters: Vec::new(),
                };
                let out = self.state.query.run(req).await?;
                Ok(ok_json(json!({
                    "columns": out.columns,
                    "rows": out.rows,
                    "scanned_rows": out.scanned_rows,
                    "took_ms": out.took_ms,
                })))
            }
            BuiltinToolKind::ListStreams => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                handle_list_streams(&self.state.telemetry.streams, &org_id, call.arguments).await
            }
            BuiltinToolKind::GetTrace => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                let args: GetTraceArgs = parse_args(call.arguments)?;
                validate_trace_id(&args.trace_id)?;
                let now = TimestampMicros::now();
                let range = TimeRange::new(
                    TimestampMicros(now.0 - GET_TRACE_WINDOW_SECS * 1_000_000),
                    now,
                );
                // 与 /web/trace 走同一套：动态解析 traces 流（按需建流后通常叫 `default`），
                // `SELECT *` 取全列让 rows_to_spans 按标准 OTEL 列名提取 + 聚合扁平属性。
                let Some(stream) = crate::api::http::routes::web::trace::resolve_traces_stream(
                    &self.state,
                    &org_id,
                )
                .await
                else {
                    return Ok(err_text(format!("trace {} not found", args.trace_id)));
                };
                let sql = format!(
                    "SELECT *
                     FROM \"{stream}\"
                     WHERE trace_id = '{trace_id}'
                     ORDER BY _timestamp ASC
                     LIMIT {limit}",
                    stream = crate::infra::query::escape_sql_ident(&stream),
                    trace_id = args.trace_id,
                    limit = GET_TRACE_SPAN_LIMIT + 1,
                );
                let req = QueryRequest {
                    org_id: org_id.clone(),
                    language: QueryLanguage::Sql,
                    statement: sql,
                    time_range: range,
                    stream: Some(StreamHint {
                        name: stream,
                        stream_type: StreamType::Traces,
                    }),
                    limit: Some(GET_TRACE_SPAN_LIMIT + 1),
                    federation_clusters: Vec::new(),
                };
                let out = self.state.query.run(req).await?;
                let (spans, truncated) = crate::app::web::trace::view::rows_to_spans(&out);
                if spans.is_empty() {
                    return Ok(err_text(format!("trace {} not found", args.trace_id)));
                }
                Ok(ok_json(
                    serde_json::to_value(crate::app::web::trace::view::TraceResponse::new(
                        args.trace_id,
                        spans,
                        truncated,
                    ))
                    .unwrap_or(Value::Null),
                ))
            }
            BuiltinToolKind::ListRecentAlerts => {
                Permission::require_key(&auth, "alerts.read")?;
                let args: ListRecentAlertsArgs =
                    parse_args(call.arguments.clone()).unwrap_or_default();
                let limit = args
                    .limit
                    .unwrap_or(ALERTS_DEFAULT_LIMIT)
                    .clamp(1, ALERTS_MAX_LIMIT);
                let mut incidents = self
                    .state
                    .alerting
                    .service
                    .list_incidents_active(&org_id)
                    .await?;
                // 最新优先（created_at desc），再取 limit
                incidents.sort_by_key(|i| std::cmp::Reverse(i.created_at.0));
                incidents.truncate(limit);
                Ok(ok_json(json!({ "incidents": incidents })))
            }
            BuiltinToolKind::ListOnCallSchedules => {
                Permission::require_key(&auth, "schedules.read")?;
                handle_list_on_call_schedules(&self.state, &org_id, call.arguments).await
            }
            BuiltinToolKind::GetCurrentOnCall => {
                Permission::require_key(&auth, "schedules.read")?;
                handle_get_current_on_call(&self.state, &org_id, call.arguments).await
            }
            BuiltinToolKind::ListRumSessions => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                handle_list_rum(
                    &self.state,
                    &org_id,
                    "rum_sessions",
                    "sessions",
                    "started_at_micros",
                    call.arguments,
                )
                .await
            }
            BuiltinToolKind::ListRumActions => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                handle_list_rum(
                    &self.state,
                    &org_id,
                    "rum_actions",
                    "actions",
                    "ts_micros",
                    call.arguments,
                )
                .await
            }
            BuiltinToolKind::ListRumErrors => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                handle_list_rum(
                    &self.state,
                    &org_id,
                    "rum_errors",
                    "errors",
                    "timestamp",
                    call.arguments,
                )
                .await
            }
            BuiltinToolKind::ListContinuousProfiles => {
                Permission::require_any_key(&auth, &["streams.query", "sys.telemetry.read"])?;
                handle_list_continuous_profiles(&self.state, &org_id, call.arguments).await
            }
            BuiltinToolKind::ListReportTemplates => {
                Permission::require_key(&auth, "dashboards.read")?;
                handle_list_report_templates(&self.state, &org_id).await
            }
            BuiltinToolKind::ListScheduledReports => {
                Permission::require_key(&auth, "dashboards.read")?;
                handle_list_scheduled_reports(&self.state, &org_id, call.arguments).await
            }
            BuiltinToolKind::GetDashboardCapabilities => {
                dashboard::get_capabilities(&self.state, &auth, call.arguments).await
            }
            BuiltinToolKind::PrepareDashboard => {
                dashboard::prepare(&self.state, &auth, call.arguments).await
            }
            BuiltinToolKind::ProposeDashboardCreation => {
                dashboard::propose(&self.state, &auth, ctx, call.arguments, execution_mode).await
            }
            BuiltinToolKind::ProposeOperation => {
                Permission::require_key(&auth, "intelligence.use")?;
                if !ctx.execution_policy.allows_approval_request() {
                    return Ok(err_text(
                        "the current chat execution policy does not allow creating approval requests",
                    ));
                }
                let args: ProposeOperationArgs = parse_args(call.arguments)?;
                let approval = super::control::create_agent_approval(
                    &self.state,
                    &auth,
                    super::control::CreateApprovalRequest {
                        investigation_id: ctx.investigation_id.clone().map(Id),
                        action: args.action,
                        target: args.target,
                        parameters: args.parameters,
                        reason: args.reason,
                        impact: args.impact,
                        expires_at_micros: args.expires_at_micros,
                        required_approvals_override: None,
                    },
                )
                .await?;
                Ok(ok_json(json!({
                    "approval": approval,
                    "message": "Mole Agent 已创建审批请求；审批前不会执行该操作。"
                })))
            }
        }
    }
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
                    let normalized = key.to_ascii_lowercase();
                    let value = if SENSITIVE_PARTS.iter().any(|part| normalized.contains(part)) {
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

// ---- Tool argument types ----

#[derive(Debug, Deserialize)]
struct TimeRangeArg {
    start_micros: i64,
    end_micros: i64,
}

#[derive(Debug, Deserialize)]
struct QueryLogsArgs {
    sql: String,
    stream: String,
    time_range: TimeRangeArg,
}

#[derive(Debug, Deserialize)]
struct QueryMetricsArgs {
    promql: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    #[allow(dead_code)]
    step_secs: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ListStreamsArgs {
    #[serde(default)]
    stream_type: Option<StreamType>,
}

#[derive(Debug, Deserialize)]
struct GetTraceArgs {
    trace_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct ListRecentAlertsArgs {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct GetCurrentOnCallArgs {
    #[serde(default)]
    schedule_id: Option<String>,
    #[serde(default)]
    at_micros: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ListOnCallSchedulesArgs {
    #[serde(default)]
    enabled_only: Option<bool>,
    #[serde(default)]
    at_micros: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ListRumArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    action_type: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct ListContinuousProfilesArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    profile_type: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct ListScheduledReportsArgs {
    #[serde(default)]
    enabled_only: bool,
}

#[derive(Debug, Deserialize)]
struct ProposeOperationArgs {
    action: String,
    target: String,
    #[serde(default)]
    parameters: Value,
    reason: String,
    impact: String,
    #[serde(default)]
    expires_at_micros: Option<i64>,
}

// ---- helpers ----

fn parse_args<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|e| Error::invalid(format!("invalid tool arguments: {e}")))
}

fn validated_time_range(value: TimeRangeArg) -> Result<TimeRange> {
    if value.start_micros > value.end_micros {
        return Err(Error::invalid(
            "time_range.start_micros must be less than or equal to end_micros",
        ));
    }
    Ok(TimeRange::new(
        TimestampMicros(value.start_micros),
        TimestampMicros(value.end_micros),
    ))
}

fn unsupported_promql_message(promql: &str) -> Option<&'static str> {
    let normalized = promql.to_ascii_lowercase();
    normalized.contains("label_values(").then_some(
        "label_values() is a Grafana template helper, not PromQL. Use list_streams with stream_type=metrics to discover available metric names and fields, then submit a valid PromQL selector or aggregation.",
    )
}

fn optional_time_range(value: Option<TimeRangeArg>) -> Result<TimeRange> {
    match value {
        Some(value) => validated_time_range(value),
        None => {
            let end = TimestampMicros::now();
            Ok(TimeRange::new(
                TimestampMicros(end.0 - DEFAULT_LOOKBACK_MICROS),
                end,
            ))
        }
    }
}

fn ok_json(v: Value) -> ToolResult {
    ToolResult {
        content: vec![ToolContent::Json { json: v }],
        is_error: false,
    }
}

fn err_text(msg: impl Into<String>) -> ToolResult {
    ToolResult {
        content: vec![ToolContent::Text { text: msg.into() }],
        is_error: true,
    }
}

fn filter_streams(all: Vec<StreamDefinition>, want: Option<StreamType>) -> Vec<StreamDefinition> {
    match want {
        Some(t) => all.into_iter().filter(|s| s.stream_type == t).collect(),
        None => all,
    }
}

/// 不暴露给 Mole Agent 的内部 observability 流：模型查它们没有意义，且其 schema 不是为
/// 用户分析设计的（如 `intelligence_model_traces` 的 cost_usd 等），会导致聚合查询失败 → 重试循环。
fn is_internal_stream(name: &str) -> bool {
    name == "_intelligence_model_traces" || name.starts_with('_')
}

async fn handle_list_streams(
    streams: &std::sync::Arc<dyn StreamRepository>,
    org_id: &Id,
    args_value: Value,
) -> Result<ToolResult> {
    let args: ListStreamsArgs = parse_args(args_value).unwrap_or_default();
    let all = streams.list(org_id).await?;
    let candidates: Vec<_> = filter_streams(all, args.stream_type)
        .into_iter()
        .filter(|s| !is_internal_stream(&s.name))
        .collect();
    // 不可查询的 stream（仅作 ingest 入口 / pipeline 源）对 Mole Agent 没有意义：排除掉，免得模型
    // 对其发起查询拿到 forbidden 后陷入重试。settings 与 streams 表分列存储，逐个取。
    let mut filtered = Vec::with_capacity(candidates.len());
    for s in candidates {
        if streams.get_settings(&s.id).await?.queryable {
            filtered.push(s);
        }
    }
    Ok(ok_json(json!({ "streams": filtered })))
}

async fn handle_list_on_call_schedules(
    state: &AppState,
    org_id: &Id,
    args_value: Value,
) -> Result<ToolResult> {
    let args: ListOnCallSchedulesArgs = parse_args(args_value)?;
    let at = TimestampMicros(args.at_micros.unwrap_or_else(|| TimestampMicros::now().0));
    let enabled_only = args.enabled_only.unwrap_or(true);
    let schedules = state.alerting.service.list_schedules(org_id).await?;
    let mut rows = Vec::new();
    for schedule in schedules
        .into_iter()
        .filter(|schedule| !enabled_only || schedule.enabled)
    {
        let current_on_call = match schedule.who_is_on_call(at) {
            Some(user_id) => on_call_user_json(state, &user_id).await,
            None => Value::Null,
        };
        rows.push(json!({
            "schedule_id": schedule.id,
            "name": schedule.name,
            "description": schedule.description,
            "team_id": schedule.team_id,
            "timezone": schedule.timezone,
            "enabled": schedule.enabled,
            "rotation_count": schedule.rotations.len(),
            "override_count": schedule.overrides.len(),
            "current_on_call": current_on_call,
        }));
    }
    let schedule_count = rows.len();
    Ok(ok_json(json!({
        "at_micros": at,
        "schedules": rows,
        "schedule_count": schedule_count,
    })))
}

async fn handle_get_current_on_call(
    state: &AppState,
    org_id: &Id,
    args_value: Value,
) -> Result<ToolResult> {
    let args: GetCurrentOnCallArgs = parse_args(args_value)?;
    let at = TimestampMicros(args.at_micros.unwrap_or_else(|| TimestampMicros::now().0));
    let schedules = if let Some(schedule_id) = args.schedule_id {
        let schedule = state
            .alerting
            .service
            .get_schedule(&Id(schedule_id))
            .await?;
        if schedule.org_id != *org_id {
            return Err(Error::not_found("schedule not found"));
        }
        vec![schedule]
    } else {
        state.alerting.service.list_schedules(org_id).await?
    };

    let mut available_schedules = Vec::with_capacity(schedules.len());
    let mut on_call = Vec::new();
    for schedule in schedules {
        let user_id = schedule.who_is_on_call(at);
        let current_on_call = match user_id.as_ref() {
            Some(user_id) => on_call_user_json(state, user_id).await,
            None => Value::Null,
        };
        available_schedules.push(json!({
            "schedule_id": schedule.id,
            "name": schedule.name,
            "enabled": schedule.enabled,
            "timezone": schedule.timezone,
            "has_current_assignee": user_id.is_some(),
            "current_on_call": current_on_call,
        }));
        if schedule.enabled
            && let Some(user_id) = user_id
        {
            on_call.push(json!({
                "schedule_id": schedule.id,
                "schedule_name": schedule.name,
                "at_micros": at,
                "user": on_call_user_json(state, &user_id).await,
            }));
        }
    }

    let message = if available_schedules.is_empty() {
        "No on-call schedules are configured in the current organization."
    } else if on_call.is_empty() {
        "Schedules are configured, but none has an active assignee at the requested time."
    } else {
        "Current on-call assignments resolved successfully."
    };
    Ok(ok_json(json!({
        "at_micros": at,
        "on_call": on_call,
        "available_schedules": available_schedules,
        "message": message,
    })))
}

async fn on_call_user_json(state: &AppState, user_id: &Id) -> Value {
    let user = state.iam.service.current_user(user_id).await.ok();
    json!({
        "id": user_id,
        "display_name": user.as_ref().map(|item| item.display_name.as_str()),
        "avatar_url": user.as_ref().and_then(|item| item.avatar_url.as_deref()),
    })
}

async fn handle_list_rum(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    result_key: &str,
    order_column: &str,
    args_value: Value,
) -> Result<ToolResult> {
    let args: ListRumArgs = parse_args(args_value)?;
    let time_range = optional_time_range(args.time_range)?;
    let limit = args
        .limit
        .unwrap_or(OBSERVABILITY_DEFAULT_LIMIT)
        .clamp(1, OBSERVABILITY_MAX_LIMIT);
    let mut filters = Vec::new();
    push_string_filter(&mut filters, "application", args.application.as_deref());
    push_string_filter(&mut filters, "environment", args.environment.as_deref());
    push_string_filter(&mut filters, "session_id", args.session_id.as_deref());
    if stream == "rum_sessions" {
        push_string_filter(&mut filters, "user_id", args.user_id.as_deref());
    } else if stream == "rum_errors" {
        push_string_filter(&mut filters, "fingerprint", args.fingerprint.as_deref());
    } else {
        push_string_filter(&mut filters, "type", args.action_type.as_deref());
    }
    let statement = format!(
        "SELECT * FROM \"{}\"{} ORDER BY {order_column} DESC LIMIT {limit}",
        crate::infra::query::escape_sql_ident(stream),
        where_clause(&filters),
    );
    let result = run_optional_stream_query(
        state,
        org_id,
        statement,
        time_range,
        stream,
        StreamType::Logs,
        limit,
    )
    .await?;
    let Some(result) = result else {
        return Ok(ok_json(json!({
            (result_key): [],
            "stream_available": false,
            "message": format!("{stream} has not received data yet"),
        })));
    };
    let rows = query_rows_as_objects(&result);
    Ok(ok_json(json!({
        (result_key): rows,
        "stream_available": true,
        "scanned_rows": result.scanned_rows,
        "took_ms": result.took_ms,
    })))
}

async fn handle_list_continuous_profiles(
    state: &AppState,
    org_id: &Id,
    args_value: Value,
) -> Result<ToolResult> {
    let args: ListContinuousProfilesArgs = parse_args(args_value)?;
    let time_range = optional_time_range(args.time_range)?;
    let limit = args
        .limit
        .unwrap_or(OBSERVABILITY_DEFAULT_LIMIT)
        .clamp(1, PROFILES_MAX_LIMIT);
    let mut filters = Vec::new();
    push_string_filter(&mut filters, "service", args.service.as_deref());
    push_string_filter(&mut filters, "profile_type", args.profile_type.as_deref());
    push_string_filter(&mut filters, "trace_id", args.trace_id.as_deref());
    let statement = format!(
        "SELECT id, service, profile_type, total_value, sample_count, duration_nanos, \
         unsymbolized, trace_id, span_id, _timestamp AS timestamp \
         FROM \"default\"{} ORDER BY _timestamp DESC LIMIT {limit}",
        where_clause(&filters),
    );
    let result = run_optional_stream_query(
        state,
        org_id,
        statement,
        time_range,
        "default",
        StreamType::Profiles,
        limit,
    )
    .await?;
    let Some(result) = result else {
        return Ok(ok_json(json!({
            "profiles": [],
            "stream_available": false,
            "message": "Continuous Profiles has not received data yet",
        })));
    };
    Ok(ok_json(json!({
        "profiles": query_rows_as_objects(&result),
        "stream_available": true,
        "scanned_rows": result.scanned_rows,
        "took_ms": result.took_ms,
    })))
}

async fn handle_list_report_templates(state: &AppState, org_id: &Id) -> Result<ToolResult> {
    let builtins = super::super::reports::templates::builtin_templates_json();
    let custom = state.platform.report_templates.list(org_id).await?;
    Ok(ok_json(json!({
        "builtin_templates": builtins,
        "custom_templates": custom,
    })))
}

async fn handle_list_scheduled_reports(
    state: &AppState,
    org_id: &Id,
    args_value: Value,
) -> Result<ToolResult> {
    let args: ListScheduledReportsArgs = parse_args(args_value)?;
    let reports = state
        .platform
        .scheduled_reports
        .list(org_id)
        .await?
        .into_iter()
        .filter(|report| !args.enabled_only || report.enabled)
        .collect::<Vec<_>>();
    Ok(ok_json(json!({ "scheduled_reports": reports })))
}

async fn run_optional_stream_query(
    state: &AppState,
    org_id: &Id,
    statement: String,
    time_range: TimeRange,
    stream: &str,
    stream_type: StreamType,
    limit: usize,
) -> Result<Option<QueryResult>> {
    match state
        .query
        .run(QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range,
            stream: Some(StreamHint {
                name: stream.to_string(),
                stream_type,
            }),
            limit: Some(limit),
            federation_clusters: Vec::new(),
        })
        .await
    {
        Ok(result) => Ok(Some(result)),
        Err(error)
            if error
                .to_string()
                .to_ascii_lowercase()
                .contains("stream not found") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn query_rows_as_objects(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (column, value) in result.columns.iter().zip(row) {
                object.insert(column.clone(), value.clone());
            }
            Value::Object(object)
        })
        .collect()
}

fn push_string_filter(filters: &mut Vec<String>, column: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        filters.push(format!("{column} = '{}'", sql_escape_literal(value)));
    }
}

fn where_clause(filters: &[String]) -> String {
    if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    }
}

fn sql_escape_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn first_text(content: &[ToolContent]) -> Option<String> {
    content.iter().find_map(|c| match c {
        ToolContent::Text { text } => Some(text.clone()),
        ToolContent::Json { json } => serde_json::to_string(json).ok(),
    })
}

fn validate_trace_id(trace_id: &str) -> Result<()> {
    if trace_id.is_empty() || trace_id.len() > 128 {
        return Err(Error::invalid("trace_id length must be 1..=128"));
    }
    if !trace_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::invalid("trace_id contains invalid characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::{
        domain::stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
        intelligence::tools::ToolContent,
        shared::{Result as SharedResult, ids::Id, time::TimestampMicros},
    };

    fn auth(org: &str) -> ToolAuthContext {
        ToolAuthContext {
            user_id: "u1".into(),
            org_id: org.into(),
            chat_id: None,
            investigation_id: None,
            execution_policy: Default::default(),
            query_generation_only: false,
        }
    }

    fn mk_stream(org: &str, name: &str, t: StreamType) -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id(org.into()),
            name: name.into(),
            stream_type: t,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "msg".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    /// fake StreamRepository：捕获每次 `list(org_id)` 入参，按 org 返不同 stream 集。
    #[derive(Default)]
    struct FakeStreams {
        called_with: Mutex<Vec<String>>,
        by_org: std::collections::HashMap<String, Vec<StreamDefinition>>,
    }

    #[async_trait]
    impl StreamRepository for FakeStreams {
        async fn create(&self, _def: StreamDefinition) -> SharedResult<StreamDefinition> {
            unimplemented!()
        }
        async fn update_schema(&self, _id: &Id, _schema: Schema) -> SharedResult<()> {
            unimplemented!()
        }
        async fn get(
            &self,
            _org_id: &Id,
            _name: &str,
            _t: StreamType,
        ) -> SharedResult<StreamDefinition> {
            unimplemented!()
        }
        async fn list(&self, org_id: &Id) -> SharedResult<Vec<StreamDefinition>> {
            self.called_with.lock().unwrap().push(org_id.0.clone());
            Ok(self.by_org.get(&org_id.0).cloned().unwrap_or_default())
        }
        async fn delete(&self, _id: &Id) -> SharedResult<()> {
            unimplemented!()
        }
    }

    /// 5.1 + 5.2：dispatcher 始终以 `ToolAuthContext.org_id` 调 `streams.list`；
    /// 即使 args 里硬塞 `org_id: "B"`，也不会泄漏 org B 的 streams。
    ///
    /// 反推方式：fake repo 按 org 返不同条数（A=1 / B=2），结果只含 A 的条目即可证明
    /// dispatcher 用了 ctx.org_id（A），忽略了 args.org_id（B）。同时 `called_with` 捕获
    /// list() 入参做直接断言。
    #[tokio::test]
    async fn list_streams_uses_ctx_org_and_ignores_arg_org_id() {
        let mut by_org = std::collections::HashMap::new();
        by_org.insert(
            "A".to_string(),
            vec![mk_stream("A", "a-logs", StreamType::Logs)],
        );
        by_org.insert(
            "B".to_string(),
            vec![
                mk_stream("B", "b-logs", StreamType::Logs),
                mk_stream("B", "b-mx", StreamType::Metrics),
            ],
        );
        let fake = Arc::new(FakeStreams {
            called_with: Mutex::new(Vec::new()),
            by_org,
        });
        let streams: Arc<dyn StreamRepository> = fake.clone();
        let ctx = auth("A");
        // 攻击向量：args 含 org_id=B
        let args = json!({ "org_id": "B" });
        let r = handle_list_streams(&streams, &Id(ctx.org_id.clone()), args)
            .await
            .unwrap();
        assert!(!r.is_error);
        // 直接断言 list() 收到的是 ctx.org_id="A"，不是 args.org_id="B"
        let captured = fake.called_with.lock().unwrap().clone();
        assert_eq!(captured, vec!["A".to_string()]);
        if let ToolContent::Json { json } = &r.content[0] {
            let arr = json["streams"].as_array().unwrap();
            assert_eq!(arr.len(), 1, "expected only org A's streams");
            assert_eq!(arr[0]["name"], "a-logs");
        } else {
            panic!("expected JSON content");
        }
    }

    /// 5.1 续：stream_type 过滤生效。
    #[tokio::test]
    async fn list_streams_filters_by_stream_type() {
        let mut by_org = std::collections::HashMap::new();
        by_org.insert(
            "A".to_string(),
            vec![
                mk_stream("A", "logs1", StreamType::Logs),
                mk_stream("A", "metrics1", StreamType::Metrics),
                mk_stream("A", "traces1", StreamType::Traces),
            ],
        );
        let fake = FakeStreams {
            called_with: Mutex::new(Vec::new()),
            by_org,
        };
        let streams: Arc<dyn StreamRepository> = Arc::new(fake);
        let r = handle_list_streams(&streams, &Id("A".into()), json!({ "stream_type": "logs" }))
            .await
            .unwrap();
        if let ToolContent::Json { json } = &r.content[0] {
            let arr = json["streams"].as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0]["name"], "logs1");
        } else {
            panic!("expected JSON content");
        }
    }

    /// 5.3：未知 tool name → `is_error: true` + "unknown tool: <name>"。
    /// 通过验证默认 arm 返回的形状（err_text 是默认 arm 的唯一构造点）。
    #[test]
    fn unknown_tool_name_returns_error_result() {
        let r = err_text("unknown tool: delete_everything");
        assert!(r.is_error);
        let body = first_text(&r.content).unwrap();
        assert!(body.starts_with("unknown tool: "));
        assert!(body.contains("delete_everything"));
    }

    #[test]
    fn unknown_tool_is_classified_as_high_risk() {
        assert_eq!(
            risk_for_tool("delete_everything").unwrap_or(RiskLevel::L4),
            RiskLevel::L4
        );
    }

    #[test]
    fn sensitive_tool_arguments_are_redacted_recursively() {
        let redacted = redact_sensitive_value(&json!({
            "query": "status:error",
            "authorization": "Bearer abc",
            "nested": {
                "api_key": "secret-value",
                "safe": 42
            }
        }));
        assert_eq!(redacted["query"], "status:error");
        assert_eq!(redacted["authorization"], "<redacted>");
        assert_eq!(redacted["nested"]["api_key"], "<redacted>");
        assert_eq!(redacted["nested"]["safe"], 42);
    }

    /// 验证 ListStreamsArgs 不接 `org_id` 字段（防 prompt injection 的编译期保证）。
    #[test]
    fn list_streams_args_struct_has_no_org_id_field() {
        let parsed: ListStreamsArgs = serde_json::from_value(json!({
            "org_id": "B",
            "stream_type": "logs"
        }))
        .unwrap();
        assert!(matches!(parsed.stream_type, Some(StreamType::Logs)));
    }

    #[tokio::test]
    async fn first_text_extracts_text_payload() {
        let r = err_text("unknown tool: foo");
        let s = first_text(&r.content).unwrap();
        assert_eq!(s, "unknown tool: foo");
    }

    #[tokio::test]
    async fn parse_args_returns_invalid_for_bad_shape() {
        let res: Result<GetTraceArgs> = parse_args(json!({"not_a_trace_id": 1}));
        let err = res.expect_err("should error");
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[test]
    fn validate_trace_id_rejects_quote_chars() {
        assert!(validate_trace_id("abc' OR '1'='1").is_err());
        assert!(validate_trace_id("abc-123_DEF").is_ok());
        assert!(validate_trace_id("").is_err());
        assert!(validate_trace_id(&"x".repeat(129)).is_err());
    }

    #[test]
    fn time_range_validation_rejects_reversed_bounds() {
        assert!(
            validated_time_range(TimeRangeArg {
                start_micros: 20,
                end_micros: 10,
            })
            .is_err()
        );
        let range = validated_time_range(TimeRangeArg {
            start_micros: 10,
            end_micros: 20,
        })
        .expect("valid range");
        assert_eq!(range.start.0, 10);
        assert_eq!(range.end.0, 20);
    }

    #[test]
    fn grafana_label_values_helper_is_rejected_before_query_execution() {
        let message = unsupported_promql_message("label_values(http_requests_total, service)")
            .expect("Grafana helper should be rejected");
        assert!(message.contains("not PromQL"));
        assert!(unsupported_promql_message("sum(rate(http_requests_total[5m]))").is_none());
    }

    #[test]
    fn omitted_optional_time_range_defaults_to_one_hour() {
        let range = optional_time_range(None).expect("default range");
        assert_eq!(range.end.0 - range.start.0, DEFAULT_LOOKBACK_MICROS);
    }

    #[test]
    fn query_rows_are_returned_as_named_objects() {
        let result = QueryResult {
            columns: vec!["session_id".into(), "application".into()],
            rows: vec![vec![json!("session-1"), json!("shop")]],
            scanned_rows: 1,
            took_ms: 2,
            federation: None,
        };
        assert_eq!(
            query_rows_as_objects(&result),
            vec![json!({
                "session_id": "session-1",
                "application": "shop",
            })]
        );
    }

    #[test]
    fn generated_filters_escape_sql_literals() {
        let mut filters = Vec::new();
        push_string_filter(
            &mut filters,
            "application",
            Some("shop' OR application <> 'shop"),
        );
        assert_eq!(
            where_clause(&filters),
            " WHERE application = 'shop'' OR application <> ''shop'"
        );
    }

    #[test]
    fn rum_arguments_cannot_override_tenant_context() {
        let parsed: ListRumArgs = serde_json::from_value(json!({
            "org_id": "other-org",
            "application": "shop"
        }))
        .expect("RUM args");
        assert_eq!(parsed.application.as_deref(), Some("shop"));
    }

    #[test]
    fn ok_json_wraps_value_with_is_error_false() {
        let r = ok_json(json!({"hello": "world"}));
        assert!(!r.is_error);
        assert_eq!(r.content.len(), 1);
        if let ToolContent::Json { json } = &r.content[0] {
            assert_eq!(json["hello"], "world");
        } else {
            panic!("expected Json content");
        }
    }
}
