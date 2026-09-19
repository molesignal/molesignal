// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::ListAlertRules => (
            "List alert rules with state, query, thresholds, and routing metadata.",
            "rules",
            object_schema(
                json!({"enabled_only": {"type": "boolean", "default": false}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Rules"],
        ),
        BuiltinToolKind::GetAlertRule => (
            "Get one alert rule by id.",
            "rules",
            id_input("rule_id"),
            vec!["Alerts", "Rules"],
        ),
        BuiltinToolKind::CreateAlertRule => (
            "Create an alert rule after validation.",
            "rules",
            alert_rule_input(None),
            vec!["Alerts", "Rules", "CRUD", "Create"],
        ),
        BuiltinToolKind::UpdateAlertRule => (
            "Replace one alert rule after validation.",
            "rules",
            alert_rule_input(Some("rule_id")),
            vec!["Alerts", "Rules", "CRUD", "Update"],
        ),
        BuiltinToolKind::DeleteAlertRule => (
            "Delete one alert rule.",
            "rules",
            proposal_input("rule_id"),
            vec!["Alerts", "Rules", "CRUD", "Delete"],
        ),
        BuiltinToolKind::TestAlertRule => (
            "Execute one alert rule as a side-effect-free dry run and return its match decision and bounded sample.",
            "rules",
            id_input("rule_id"),
            vec!["Alerts", "Rules", "Test"],
        ),
        BuiltinToolKind::TriggerAlertRule => (
            "Manually open the incident represented by one alert rule without evaluating its threshold.",
            "rules",
            proposal_input("rule_id"),
            vec!["Alerts", "Rules", "Trigger"],
        ),
        BuiltinToolKind::ListIncidentGroups => (
            "List correlated incident groups with bounded status filtering.",
            "incident_groups",
            object_schema(
                json!({"status": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Incidents", "Groups"],
        ),
        BuiltinToolKind::GetIncidentGroup => (
            "Get one correlated incident group by id.",
            "incident_groups",
            id_input("group_id"),
            vec!["Incidents", "Groups"],
        ),
        BuiltinToolKind::ListMuteRules => (
            "List alert mute rules without exposing secrets.",
            "mutes",
            object_schema(
                json!({"enabled_only": {"type": "boolean", "default": false}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Mutes"],
        ),
        BuiltinToolKind::GetMuteRule => (
            "Get one alert mute rule by id.",
            "mutes",
            id_input("mute_id"),
            vec!["Alerts", "Mutes"],
        ),
        BuiltinToolKind::ListEscalationPolicies => (
            "List incident escalation policies.",
            "escalations",
            object_schema(
                json!({"limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Escalations"],
        ),
        BuiltinToolKind::GetEscalationPolicy => (
            "Get one incident escalation policy by id.",
            "escalations",
            id_input("policy_id"),
            vec!["Alerts", "Escalations"],
        ),
        _ => unreachable!("alerting catalog received unrelated kind"),
    };
    let permissions = if matches!(
        kind,
        BuiltinToolKind::CreateAlertRule
            | BuiltinToolKind::UpdateAlertRule
            | BuiltinToolKind::DeleteAlertRule
            | BuiltinToolKind::TestAlertRule
            | BuiltinToolKind::TriggerAlertRule
    ) {
        &["alerts.manage"][..]
    } else {
        &["alerts.read"][..]
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "alerts_on_call",
        category,
        input,
        open_output(),
        permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::CreateAlertRule | BuiltinToolKind::UpdateAlertRule => {
            tool.managed_mutation(RiskLevel::L2, false, false)
        }
        BuiltinToolKind::DeleteAlertRule => tool.managed_mutation(RiskLevel::L3, true, true),
        BuiltinToolKind::TriggerAlertRule => tool.managed_mutation(RiskLevel::L3, false, false),
        _ => tool,
    }
}

fn alert_rule_input(id_field: Option<&str>) -> serde_json::Value {
    let mut required = vec!["name", "query", "trigger", "reason", "impact"];
    let mut properties = json!({
        "name": {"type": "string", "minLength": 1},
        "description": {"type": "string"},
        "enabled": {"type": "boolean", "default": true},
        "kind": {"type": "string", "enum": ["scheduled", "real_time", "anomaly"]},
        "query": {"type": "object"},
        "trigger": {"type": "object"},
        "thresholds": {"type": "array", "items": {"type": "object"}},
        "severity": {"type": ["string", "null"], "enum": ["info", "warning", "error", "critical", null]},
        "anomaly_params": {"type": ["object", "null"]},
        "escalation_policy_id": {"type": ["string", "null"]},
        "labels": {"type": "object", "additionalProperties": {"type": "string"}},
        "annotations": {"type": "object", "additionalProperties": {"type": "string"}},
        "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
        "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
        "expires_at_micros": {"type": "integer"}
    });
    if let Some(field) = id_field {
        required.insert(0, field);
        properties
            .as_object_mut()
            .expect("alert rule properties")
            .insert(field.into(), json!({"type": "string", "minLength": 1}));
    }
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
        "additionalProperties": false
    })
}

fn id_input(field: &str) -> serde_json::Value {
    json!({
        "type": "object", "required": [field],
        "properties": {(field): {"type": "string"}}, "additionalProperties": false
    })
}

fn proposal_input(field: &str) -> serde_json::Value {
    json!({
        "type": "object", "required": [field, "reason", "impact"],
        "properties": {
            (field): {"type": "string", "minLength": 1},
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        },
        "additionalProperties": false
    })
}
