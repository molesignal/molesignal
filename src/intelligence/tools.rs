// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的服务端工具注册表。
//!
//! 工具只能在代码中注册；不存在动态 Shell、Browser、任意 HTTP 或开放式 MCP 入口。
//! 调度时组织与用户身份只取可信认证上下文，模型参数中的身份字段一律忽略。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{intelligence::model::RiskLevel, shared::Result};

mod dashboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccess {
    ReadOnly,
    CreatesApprovalRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub risk: RiskLevel,
    pub access: ToolAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text { text: String },
    Json { json: serde_json::Value },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionPolicy {
    AdviceOnly,
    ReadOnly,
    #[default]
    Policy,
}

impl AgentExecutionPolicy {
    pub const fn allows_approval_request(self) -> bool {
        matches!(self, Self::Policy)
    }
}

#[derive(Debug, Clone)]
pub struct ToolAuthContext {
    pub user_id: String,
    pub org_id: String,
    pub chat_id: Option<String>,
    pub investigation_id: Option<String>,
    pub execution_policy: AgentExecutionPolicy,
    pub query_generation_only: bool,
}

#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    async fn dispatch(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    QueryLogs,
    QueryMetrics,
    ListStreams,
    GetTrace,
    ListRecentAlerts,
    ListOnCallSchedules,
    GetCurrentOnCall,
    ListRumSessions,
    ListRumActions,
    ListRumErrors,
    ListContinuousProfiles,
    ListReportTemplates,
    ListScheduledReports,
    GetDashboardCapabilities,
    PrepareDashboard,
    ProposeDashboardCreation,
    ProposeOperation,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinToolPresentation {
    pub display_name: &'static str,
    pub description_zh: &'static str,
    pub domain: &'static str,
    pub category: &'static str,
    pub output_schema: serde_json::Value,
    pub capabilities: serde_json::Value,
    pub tags: Vec<&'static str>,
}

impl BuiltinToolKind {
    pub const ALL: [Self; 17] = [
        Self::QueryLogs,
        Self::QueryMetrics,
        Self::ListStreams,
        Self::GetTrace,
        Self::ListRecentAlerts,
        Self::ListOnCallSchedules,
        Self::GetCurrentOnCall,
        Self::ListRumSessions,
        Self::ListRumActions,
        Self::ListRumErrors,
        Self::ListContinuousProfiles,
        Self::ListReportTemplates,
        Self::ListScheduledReports,
        Self::GetDashboardCapabilities,
        Self::PrepareDashboard,
        Self::ProposeDashboardCreation,
        Self::ProposeOperation,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::QueryLogs => "query_logs",
            Self::QueryMetrics => "query_metrics",
            Self::ListStreams => "list_streams",
            Self::GetTrace => "get_trace",
            Self::ListRecentAlerts => "list_recent_alerts",
            Self::ListOnCallSchedules => "list_on_call_schedules",
            Self::GetCurrentOnCall => "get_current_on_call",
            Self::ListRumSessions => "list_rum_sessions",
            Self::ListRumActions => "list_rum_actions",
            Self::ListRumErrors => "list_rum_errors",
            Self::ListContinuousProfiles => "list_continuous_profiles",
            Self::ListReportTemplates => "list_report_templates",
            Self::ListScheduledReports => "list_scheduled_reports",
            Self::GetDashboardCapabilities => "get_dashboard_capabilities",
            Self::PrepareDashboard => "prepare_dashboard",
            Self::ProposeDashboardCreation => "propose_dashboard_creation",
            Self::ProposeOperation => "propose_operation",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    pub fn presentation(self) -> BuiltinToolPresentation {
        let read_capabilities = serde_json::json!({
            "read_only": true,
            "supports_dry_run": true,
            "idempotent": true,
            "streaming": false
        });
        let read_output = serde_json::json!({
            "type": "object",
            "additionalProperties": true
        });
        match self {
            Self::QueryLogs => BuiltinToolPresentation {
                display_name: "查询日志",
                description_zh: "对已授权的日志数据流执行只读 SQL 查询。",
                domain: "observability",
                category: "logs",
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "columns": {"type": "array"},
                        "rows": {"type": "array"},
                        "scanned_rows": {"type": "integer"},
                        "took_ms": {"type": "integer"}
                    }
                }),
                capabilities: read_capabilities,
                tags: vec!["Logs", "SQL"],
            },
            Self::QueryMetrics => BuiltinToolPresentation {
                display_name: "查询指标",
                description_zh: "执行只读 PromQL 查询并返回时间序列结果。",
                domain: "observability",
                category: "metrics",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Metrics", "PromQL"],
            },
            Self::ListStreams => BuiltinToolPresentation {
                display_name: "列出数据流",
                description_zh: "列出当前组织中可查询的可观测数据流。",
                domain: "observability",
                category: "streams",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Streams"],
            },
            Self::GetTrace => BuiltinToolPresentation {
                display_name: "查询链路",
                description_zh: "按 Trace ID 获取完整链路及其 Span。",
                domain: "observability",
                category: "traces",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Trace"],
            },
            Self::ListRecentAlerts => BuiltinToolPresentation {
                display_name: "查看近期告警",
                description_zh: "列出当前组织内仍处于活跃状态的告警事件。",
                domain: "alerts_on_call",
                category: "alerts",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Alert"],
            },
            Self::ListOnCallSchedules => BuiltinToolPresentation {
                display_name: "列出值班排班",
                description_zh: "列出可用排班表和当前值班人员，无需预先提供 Schedule ID。",
                domain: "alerts_on_call",
                category: "on_call",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["On-call", "Schedule"],
            },
            Self::GetCurrentOnCall => BuiltinToolPresentation {
                display_name: "查询当前值班",
                description_zh: "查询指定排班或全部已启用排班的当前值班人员。",
                domain: "alerts_on_call",
                category: "on_call",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["On-call"],
            },
            Self::ListRumSessions => BuiltinToolPresentation {
                display_name: "查询 RUM 会话",
                description_zh: "列出真实用户监控会话并支持应用、环境和用户筛选。",
                domain: "observability",
                category: "rum",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["RUM"],
            },
            Self::ListRumActions => BuiltinToolPresentation {
                display_name: "查询 RUM 行为",
                description_zh: "列出用户行为、页面浏览、资源与 Web Vitals 事件。",
                domain: "observability",
                category: "rum",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["RUM", "Web Vitals"],
            },
            Self::ListRumErrors => BuiltinToolPresentation {
                display_name: "查询 RUM 错误",
                description_zh: "列出真实用户监控错误并支持指纹与会话筛选。",
                domain: "observability",
                category: "rum",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["RUM", "Errors"],
            },
            Self::ListContinuousProfiles => BuiltinToolPresentation {
                display_name: "查询持续剖析",
                description_zh: "查询持续性能剖析元数据及关联服务、类型和链路。",
                domain: "observability",
                category: "profiles",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Profiles"],
            },
            Self::ListReportTemplates => BuiltinToolPresentation {
                display_name: "列出报告模板",
                description_zh: "列出当前用户可用的内置和组织级报告模板。",
                domain: "dashboard_reports",
                category: "reports",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Reports"],
            },
            Self::ListScheduledReports => BuiltinToolPresentation {
                display_name: "列出定时报告",
                description_zh: "列出组织内定时报告及其最近运行配置。",
                domain: "dashboard_reports",
                category: "reports",
                output_schema: read_output,
                capabilities: read_capabilities,
                tags: vec!["Reports", "Schedule"],
            },
            Self::GetDashboardCapabilities
            | Self::PrepareDashboard
            | Self::ProposeDashboardCreation => dashboard::presentation(self),
            Self::ProposeOperation => BuiltinToolPresentation {
                display_name: "提出处置操作",
                description_zh: "为受控运维操作创建审批请求，本工具不会直接执行操作。",
                domain: "automation",
                category: "operations",
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "approval": {"type": "object"},
                        "message": {"type": "string"}
                    }
                }),
                capabilities: serde_json::json!({
                    "read_only": false,
                    "supports_dry_run": true,
                    "idempotent": false,
                    "streaming": false
                }),
                tags: vec!["Approval", "Operations"],
            },
        }
    }
}

pub fn builtin_tools() -> Vec<Tool> {
    let mut tools = vec![
        Tool {
            name: "query_logs".into(),
            description:
                "Run a read-only SQL query against one logs stream. Before the first query, use list_streams with stream_type=logs and reference only columns present in that stream's returned schema. The time_range argument already constrains event time; never invent a timestamp/time column or add time ordering unless that exact column exists in the schema."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["sql", "time_range", "stream"],
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "Read-only SQL using only exact column names returned by list_streams for the selected stream. Do not assume a timestamp column."
                    },
                    "stream": {
                        "type": "string",
                        "description": "Exact logs stream name returned by list_streams."
                    },
                    "time_range": { "$ref": "#/$defs/time_range" }
                },
                "$defs": {
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "query_metrics".into(),
            description:
                "Run one read-only PromQL query. Use list_streams with stream_type=metrics to discover available metric streams and schema first. Only PromQL expressions are accepted: label_values() and other Grafana template helpers are not PromQL and must never be used. Do not repeat an equivalent query after it returns no rows."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["promql", "time_range"],
                "properties": {
                    "promql": {
                        "type": "string",
                        "description": "A valid PromQL expression only. Grafana helpers such as label_values() are invalid."
                    },
                    "step_secs": { "type": "integer", "default": 60 },
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_streams".into(),
            description:
                "List queryable observability streams and their exact schemas in the current organization. Use this before query_logs or query_metrics so subsequent queries only reference real stream names and fields."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "stream_type": {
                        "type": "string",
                        "enum": ["logs", "metrics", "traces", "enrichment"]
                    }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "get_trace".into(),
            description: "Fetch all spans of a trace by id.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["trace_id"],
                "properties": { "trace_id": { "type": "string" } }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_recent_alerts".into(),
            description: "List active alert incidents in the current organization.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "default": 50, "minimum": 1, "maximum": 500 }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_on_call_schedules".into(),
            description:
                "List available on-call schedules and their current assignees. No schedule id is required."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled_only": { "type": "boolean", "default": true },
                    "at_micros": { "type": "integer" }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "get_current_on_call".into(),
            description:
                "Resolve current on-call owners. schedule_id is optional; omit it to check every enabled schedule."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "schedule_id": { "type": "string" },
                    "at_micros": { "type": "integer" }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_rum_sessions".into(),
            description:
                "List recent real-user monitoring sessions from the current organization.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    },
                    "application": { "type": "string" },
                    "environment": { "type": "string" },
                    "user_id": { "type": "string" },
                    "limit": { "type": "integer", "default": 100, "minimum": 1, "maximum": 500 }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_rum_actions".into(),
            description:
                "List recent real-user monitoring actions, page views, resources, and web-vital events."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    },
                    "session_id": { "type": "string" },
                    "action_type": { "type": "string" },
                    "application": { "type": "string" },
                    "environment": { "type": "string" },
                    "limit": { "type": "integer", "default": 100, "minimum": 1, "maximum": 500 }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_rum_errors".into(),
            description:
                "List recent real-user monitoring errors from the current organization.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    },
                    "fingerprint": { "type": "string" },
                    "session_id": { "type": "string" },
                    "application": { "type": "string" },
                    "environment": { "type": "string" },
                    "limit": { "type": "integer", "default": 100, "minimum": 1, "maximum": 500 }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_continuous_profiles".into(),
            description:
                "List recent continuous-profiling metadata with optional service, type, and trace filters."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "time_range": {
                        "type": "object",
                        "required": ["start_micros", "end_micros"],
                        "properties": {
                            "start_micros": { "type": "integer" },
                            "end_micros": { "type": "integer" }
                        }
                    },
                    "service": { "type": "string" },
                    "profile_type": { "type": "string" },
                    "trace_id": { "type": "string" },
                    "limit": { "type": "integer", "default": 100, "minimum": 1, "maximum": 1000 }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_report_templates".into(),
            description:
                "List built-in and organization report templates available to the current user."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "list_scheduled_reports".into(),
            description:
                "List scheduled reports and their latest run configuration in the current organization."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled_only": { "type": "boolean", "default": false }
                }
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "propose_operation".into(),
            description:
                "Create an approval request for a registered operation; never executes it.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["action", "target", "parameters", "reason", "impact"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["acknowledge_alert", "resolve_alert"]
                    },
                    "target": { "type": "string" },
                    "parameters": { "type": "object" },
                    "reason": { "type": "string" },
                    "impact": { "type": "string" }
                }
            }),
            risk: RiskLevel::L1,
            access: ToolAccess::CreatesApprovalRequest,
        },
    ];
    tools.extend(dashboard::definitions());
    tools
}

pub fn is_builtin_tool(name: &str) -> bool {
    BuiltinToolKind::from_name(name).is_some()
}

pub fn risk_for_tool(name: &str) -> Option<RiskLevel> {
    builtin_tools()
        .into_iter()
        .find(|tool| tool.name == name)
        .map(|tool| tool.risk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_only_explicit_safe_tools() {
        let tools = builtin_tools();
        assert!(tools.iter().any(|tool| tool.name == "query_logs"));
        assert!(tools.iter().any(|tool| tool.name == "get_current_on_call"));
        assert!(tools.iter().any(|tool| tool.name == "propose_operation"));
        assert!(!tools.iter().any(|tool| tool.name == "shell"));
        assert!(!tools.iter().any(|tool| tool.name == "http"));
        assert!(!tools.iter().any(|tool| tool.name == "browser"));
    }

    #[test]
    fn registry_and_dispatch_kinds_are_complete_and_unique() {
        let tools = builtin_tools();
        let registered: std::collections::HashSet<_> =
            tools.iter().map(|tool| tool.name.as_str()).collect();
        let implemented: std::collections::HashSet<_> = BuiltinToolKind::ALL
            .iter()
            .map(|kind| kind.name())
            .collect();
        assert_eq!(
            registered.len(),
            tools.len(),
            "duplicate registered tool name"
        );
        assert_eq!(registered, implemented);
    }

    #[test]
    fn observability_tools_document_query_language_boundaries() {
        let tools = builtin_tools();
        let logs = tools
            .iter()
            .find(|tool| tool.name == "query_logs")
            .expect("query_logs");
        assert!(logs.description.contains("list_streams"));
        assert!(logs.description.contains("never invent"));

        let metrics = tools
            .iter()
            .find(|tool| tool.name == "query_metrics")
            .expect("query_metrics");
        assert!(metrics.description.contains("label_values()"));
        assert!(metrics.description.contains("not PromQL"));
    }

    #[test]
    fn every_registered_tool_has_an_object_schema() {
        for tool in builtin_tools() {
            assert_eq!(
                tool.input_schema
                    .get("type")
                    .and_then(|value| value.as_str()),
                Some("object"),
                "{} must expose an object input schema",
                tool.name
            );
        }
    }

    #[test]
    fn on_call_schedule_id_is_optional() {
        let tool = builtin_tools()
            .into_iter()
            .find(|tool| tool.name == "get_current_on_call")
            .expect("on-call tool");
        assert!(
            tool.input_schema
                .get("required")
                .and_then(serde_json::Value::as_array)
                .is_none_or(|required| !required.iter().any(|item| item == "schedule_id"))
        );
    }

    #[test]
    fn proposal_tool_cannot_execute() {
        let tool = builtin_tools()
            .into_iter()
            .find(|tool| tool.name == "propose_operation")
            .expect("proposal tool");
        assert_eq!(tool.access, ToolAccess::CreatesApprovalRequest);
        assert_eq!(tool.risk, RiskLevel::L1);
    }

    #[test]
    fn only_policy_mode_can_create_approval_requests() {
        assert!(!AgentExecutionPolicy::AdviceOnly.allows_approval_request());
        assert!(!AgentExecutionPolicy::ReadOnly.allows_approval_request());
        assert!(AgentExecutionPolicy::Policy.allows_approval_request());
    }
}
