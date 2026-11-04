// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, timestamp_schema};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::ListAlertRules => (
            "List alert rules with state, query, thresholds, and routing metadata.",
            "rules",
            object_schema(
                json!({"enabledOnly": {"type": "boolean", "default": false}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Rules"],
        ),
        BuiltinToolKind::GetAlertRule => (
            "Get one alert rule by id.",
            "rules",
            id_input("ruleId"),
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
            alert_rule_input(Some("ruleId")),
            vec!["Alerts", "Rules", "CRUD", "Update"],
        ),
        BuiltinToolKind::DeleteAlertRule => (
            "Delete one alert rule.",
            "rules",
            proposal_input("ruleId"),
            vec!["Alerts", "Rules", "CRUD", "Delete"],
        ),
        BuiltinToolKind::TestAlertRule => (
            "Execute one alert rule as a side-effect-free dry run and return its match decision and bounded sample.",
            "rules",
            id_input("ruleId"),
            vec!["Alerts", "Rules", "Test"],
        ),
        BuiltinToolKind::TriggerAlertRule => (
            "Manually open the incident represented by one alert rule without evaluating its threshold.",
            "rules",
            proposal_input("ruleId"),
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
            id_input("groupId"),
            vec!["Incidents", "Groups"],
        ),
        BuiltinToolKind::ListMuteRules => (
            "List alert mute rules without exposing secrets.",
            "mutes",
            object_schema(
                json!({"enabledOnly": {"type": "boolean", "default": false}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Mutes"],
        ),
        BuiltinToolKind::GetMuteRule => (
            "Get one alert mute rule by id.",
            "mutes",
            id_input("muteId"),
            vec!["Alerts", "Mutes"],
        ),
        BuiltinToolKind::ListInhibitRules => (
            "List alert inhibit rules: while an incident matching the source side is active, \
             incidents matching the target side and sharing the listed labels notify nobody. \
             Each rule says how many incidents it holds back now.",
            "inhibitions",
            object_schema(
                json!({"enabledOnly": {"type": "boolean", "default": false}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            ),
            vec!["Alerts", "Inhibitions"],
        ),
        BuiltinToolKind::GetInhibitRule => (
            "Get one alert inhibit rule by id.",
            "inhibitions",
            id_input("inhibitRuleId"),
            vec!["Alerts", "Inhibitions"],
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
            id_input("policyId"),
            vec!["Alerts", "Escalations"],
        ),
        BuiltinToolKind::ListDiscoveryFindings => (
            "List what automatic discovery found without a rule: APM services whose errors, \
             latency or traffic moved, new versions doing worse, new or returning RUM error \
             issues, log streams whose errors surged or that show a new error, and hosts or \
             containers whose disk or memory is nearly full, whose CPU jumped or that restart. \
             Open findings by default, the most recently seen first.",
            "discovery",
            object_schema(json!({
                "status": {"type": "string", "enum": ["open", "ended", "all"], "default": "open"},
                "kind": {"type": "string", "enum": [
                    "apm_anomaly", "apm_regression", "rum_new_issue", "rum_regressed_issue",
                    "log_error_surge", "log_new_error", "metric_saturation", "metric_shift",
                    "metric_restarts"
                ]},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Alerts", "Discovery"],
        ),
        BuiltinToolKind::GetDiscoveryFinding => (
            "Read one finding of automatic discovery by id: what moved, its value and baseline, \
             when it started, and its detail (nearby changes, the log stream and error template, \
             the PromQL that found it).",
            "discovery",
            id_input("findingId"),
            vec!["Alerts", "Discovery"],
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
        &["alerts.rules.manage"][..]
    } else if matches!(
        kind,
        BuiltinToolKind::ListInhibitRules | BuiltinToolKind::GetInhibitRule
    ) {
        &["alerts.inhibitions.read"][..]
    } else {
        &["alerts.discovery.read"][..]
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
        "anomalyParams": {"type": ["object", "null"]},
        "escalationPolicyId": {"type": ["string", "null"]},
        "labels": {"type": "object", "additionalProperties": {"type": "string"}},
        "annotations": {"type": "object", "additionalProperties": {"type": "string"}},
        "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
        "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
        "expiresAt": timestamp_schema()
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
            "expiresAt": timestamp_schema()
        },
        "additionalProperties": false
    })
}
