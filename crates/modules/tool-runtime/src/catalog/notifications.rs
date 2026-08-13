// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::ListNotificationConnectors => (
            "List notification connectors with credential fields omitted.",
            "connectors",
            list_input(),
            vec!["Notify", "Connectors"],
        ),
        BuiltinToolKind::GetNotificationConnector => (
            "Get notification connector metadata with credentials omitted.",
            "connectors",
            id_input("connector_id"),
            vec!["Notify", "Connectors"],
        ),
        BuiltinToolKind::ListNotificationPolicies => (
            "List notification routing policies.",
            "policies",
            list_input(),
            vec!["Notify", "Policies"],
        ),
        BuiltinToolKind::GetNotificationPolicy => (
            "Get one notification routing policy.",
            "policies",
            id_input("policy_id"),
            vec!["Notify", "Policies"],
        ),
        BuiltinToolKind::ListNotificationTemplates => (
            "List organization and system notification templates.",
            "templates",
            list_input(),
            vec!["Notify", "Templates"],
        ),
        BuiltinToolKind::GetNotificationTemplate => (
            "Get one notification template.",
            "templates",
            id_input("template_id"),
            vec!["Notify", "Templates"],
        ),
        BuiltinToolKind::ListNotificationDeliveries => (
            "List bounded notification delivery attempts.",
            "deliveries",
            object_schema(json!({
                "status": {"type": "string"}, "event_id": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Notify", "Deliveries"],
        ),
        BuiltinToolKind::GetNotificationDelivery => (
            "Get one notification delivery attempt.",
            "deliveries",
            id_input("delivery_id"),
            vec!["Notify", "Deliveries"],
        ),
        BuiltinToolKind::RetryNotificationDelivery => (
            "Retry one failed notification delivery.",
            "deliveries",
            proposal_input("delivery_id"),
            vec!["Notify", "Deliveries", "Retry"],
        ),
        BuiltinToolKind::AcknowledgeNotificationDelivery => (
            "Acknowledge one notification delivery.",
            "deliveries",
            proposal_input("delivery_id"),
            vec!["Notify", "Deliveries", "Acknowledge"],
        ),
        _ => unreachable!("notification catalog received unrelated kind"),
    };
    let permissions = if matches!(kind, BuiltinToolKind::AcknowledgeNotificationDelivery) {
        &["alerts.acknowledge"][..]
    } else if matches!(kind, BuiltinToolKind::RetryNotificationDelivery) {
        &["alerts.manage"][..]
    } else {
        &["alerts.read"][..]
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "notifications",
        category,
        input,
        open_output(),
        permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::RetryNotificationDelivery
        | BuiltinToolKind::AcknowledgeNotificationDelivery => {
            tool.managed_mutation(RiskLevel::L2, false, true)
        }
        _ => tool,
    }
}

fn list_input() -> serde_json::Value {
    object_schema(
        json!({"limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
    )
}

fn id_input(field: &str) -> serde_json::Value {
    json!({"type": "object", "required": [field], "properties": {(field): {"type": "string"}}, "additionalProperties": false})
}

fn proposal_input(field: &str) -> serde_json::Value {
    json!({
        "type": "object", "required": [field, "reason", "impact"],
        "properties": {
            (field): {"type": "string", "minLength": 1},
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        }, "additionalProperties": false
    })
}
