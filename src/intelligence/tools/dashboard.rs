// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, BuiltinToolPresentation, Tool, ToolAccess};
use crate::{intelligence::model::RiskLevel, shared::contracts::dashboard_authoring_validator};

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "get_dashboard_capabilities".into(),
            description: "Discover the exact Dashboard authoring contract, supported queries, visualizations, limits, and controlled creation workflow deployed on this server.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "prepare_dashboard".into(),
            description: "Validate and compile a semantic Dashboard authoring specification, then safely dry-run its queries and persist a short-lived preview draft. This never creates a Dashboard.".into(),
            input_schema: dashboard_authoring_validator().schema().clone(),
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
        },
        Tool {
            name: "propose_dashboard_creation".into(),
            description: "Create a controlled confirmation or approval request for a previously prepared Dashboard draft. This never creates a Dashboard directly.".into(),
            input_schema: json!({
                "type": "object",
                "required": ["draft_id", "expected_hash", "reason", "impact"],
                "properties": {
                    "draft_id": {"type": "string", "minLength": 1, "maxLength": 128},
                    "expected_hash": {"type": "string", "pattern": "^[a-f0-9]{64}$"},
                    "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "impact": {"type": "string", "minLength": 1, "maxLength": 2000}
                },
                "additionalProperties": false
            }),
            risk: RiskLevel::L1,
            access: ToolAccess::CreatesApprovalRequest,
        },
    ]
}

pub(super) fn presentation(kind: BuiltinToolKind) -> BuiltinToolPresentation {
    let (display_name, description_zh, category, read_only, tags) = match kind {
        BuiltinToolKind::GetDashboardCapabilities => (
            "获取 Dashboard 能力",
            "读取当前服务支持的 Dashboard 编写契约、查询类型、可视化和限制。",
            "capabilities",
            true,
            vec!["Dashboard", "Discovery"],
        ),
        BuiltinToolKind::PrepareDashboard => (
            "准备 Dashboard 草稿",
            "校验、编译并预检 Dashboard 语义规格，生成短期只读预览草稿。",
            "authoring",
            true,
            vec!["Dashboard", "Preview"],
        ),
        BuiltinToolKind::ProposeDashboardCreation => (
            "提交 Dashboard 创建建议",
            "为已预览草稿创建确认或审批请求，不直接创建 Dashboard。",
            "operations",
            false,
            vec!["Dashboard", "Approval"],
        ),
        _ => unreachable!("dashboard presentation called for a non-dashboard tool"),
    };
    BuiltinToolPresentation {
        display_name,
        description_zh,
        domain: "dashboard_reports",
        category,
        output_schema: json!({"type": "object", "additionalProperties": true}),
        capabilities: json!({
            "read_only": read_only,
            "supports_dry_run": true,
            "idempotent": kind != BuiltinToolKind::ProposeDashboardCreation,
            "streaming": false
        }),
        tags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declares_property(value: &serde_json::Value, property: &str) -> bool {
        match value {
            serde_json::Value::Object(values) => {
                values
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|properties| properties.contains_key(property))
                    || values
                        .values()
                        .any(|value| declares_property(value, property))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| declares_property(value, property)),
            _ => false,
        }
    }

    #[test]
    fn dashboard_tool_schemas_do_not_expose_server_owned_or_secret_fields() {
        for tool in definitions() {
            for field in [
                "org_id",
                "user_id",
                "approval_id",
                "compiled_model",
                "credential",
                "secret",
                "token",
                "gridPos",
                "refId",
                "schemaVersion",
            ] {
                assert!(
                    !declares_property(&tool.input_schema, field),
                    "{} unexpectedly advertises `{field}`",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn proposal_schema_is_a_minimal_closed_reference_contract() {
        let proposal = definitions()
            .into_iter()
            .find(|tool| tool.name == "propose_dashboard_creation")
            .unwrap();
        let properties = proposal.input_schema["properties"].as_object().unwrap();
        let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
        property_names.sort_unstable();
        assert_eq!(
            property_names,
            ["draft_id", "expected_hash", "impact", "reason"]
        );
        assert_eq!(proposal.input_schema["additionalProperties"], false);
    }
}
