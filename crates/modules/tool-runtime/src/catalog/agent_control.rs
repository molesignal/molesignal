// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::ListAgentInvestigations => (
            "List Mole Agent investigations.",
            "investigations",
            list_input(),
            vec!["Agent", "Investigations"],
        ),
        BuiltinToolKind::GetAgentInvestigation => (
            "Get one Mole Agent investigation with steps, evidence, and hypotheses.",
            "investigations",
            id_input("investigation_id"),
            vec!["Agent", "Investigations"],
        ),
        BuiltinToolKind::ListAgentApprovals => (
            "List Mole Agent approval requests.",
            "approvals",
            list_input(),
            vec!["Agent", "Approvals"],
        ),
        BuiltinToolKind::GetAgentApproval => (
            "Get one Mole Agent approval request.",
            "approvals",
            id_input("approval_id"),
            vec!["Agent", "Approvals"],
        ),
        BuiltinToolKind::ExecuteAgentApproval => {
            return ToolSpec::read(
                kind.name(),
                "Execute an approved Mole Agent operation idempotently.",
                "agent_control",
                "approvals",
                json!({
                    "type": "object",
                    "required": ["approval_id", "idempotency_key"],
                    "properties": {
                        "approval_id": {"type": "string"},
                        "idempotency_key": {"type": "string", "minLength": 1, "maxLength": 128}
                    },
                    "additionalProperties": false
                }),
                open_output(),
                &["agent.use"],
                &["Agent", "Approvals", "Execution"],
            )
            .approved_execution()
            .inbound_mcp_only();
        }
        BuiltinToolKind::ListAgentExecutions => (
            "List Mole Agent approved-operation executions.",
            "executions",
            list_input(),
            vec!["Agent", "Executions"],
        ),
        BuiltinToolKind::GetAgentExecution => (
            "Get one Mole Agent approved-operation execution.",
            "executions",
            id_input("execution_id"),
            vec!["Agent", "Executions"],
        ),
        BuiltinToolKind::ListAgentAutomations => (
            "List configured Mole Agent automations.",
            "automations",
            list_input(),
            vec!["Agent", "Automations"],
        ),
        _ => unreachable!("agent-control catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "agent_control",
        category,
        input,
        open_output(),
        &["agent.use"],
        &tags,
    )
}

fn list_input() -> serde_json::Value {
    object_schema(
        json!({"limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
    )
}

fn id_input(field: &str) -> serde_json::Value {
    json!({"type": "object", "required": [field], "properties": {(field): {"type": "string"}}, "additionalProperties": false})
}
