// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::ListRecentAlerts => (
            "List active alert incidents. Compatibility alias for list_incidents with status=active.",
            "incidents",
            object_schema(
                json!({"limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}}),
            ),
            vec!["alerts.read"],
            vec!["Alerts", "Compatibility"],
        ),
        BuiltinToolKind::ListIncidents => (
            "List incidents by status and optional time range, with bounded results.",
            "incidents",
            object_schema(json!({
                "status": {"type": "string", "enum": ["active", "resolved", "all"], "default": "active"},
                "time_range": time_range_schema(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
            })),
            vec!["alerts.read"],
            vec!["Incidents", "Alerts"],
        ),
        BuiltinToolKind::GetIncident => (
            "Get one incident and its alert-rule context by incident id.",
            "incidents",
            json!({"type": "object", "required": ["incident_id"], "properties": {"incident_id": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.read"],
            vec!["Incidents"],
        ),
        BuiltinToolKind::GetIncidentRca => (
            "Get the latest persisted root-cause analysis for an incident.",
            "incidents",
            json!({"type": "object", "required": ["incident_id"], "properties": {"incident_id": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.read"],
            vec!["Incidents", "RCA"],
        ),
        BuiltinToolKind::GetIncidentInsights => (
            "Return bounded incident counts grouped by status, severity, service, and rule.",
            "incidents",
            object_schema(json!({
                "time_range": time_range_schema(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
            })),
            vec!["alerts.read"],
            vec!["Incidents", "Insights"],
        ),
        BuiltinToolKind::AcknowledgeIncident => (
            "Acknowledge one open incident.",
            "incidents",
            proposal_input("incident_id"),
            vec!["alerts.acknowledge"],
            vec!["Incidents", "Acknowledge"],
        ),
        BuiltinToolKind::ResolveIncident => (
            "Resolve one open or acknowledged incident.",
            "incidents",
            proposal_input("incident_id"),
            vec!["alerts.acknowledge"],
            vec!["Incidents", "Resolve"],
        ),
        BuiltinToolKind::ListOnCallSchedules => (
            "List on-call schedules and current assignees.",
            "on_call",
            object_schema(
                json!({"enabled_only": {"type": "boolean", "default": true}, "at_micros": {"type": "integer"}}),
            ),
            vec!["schedules.read"],
            vec!["On-call", "Schedules"],
        ),
        BuiltinToolKind::GetOnCallSchedule => (
            "Get one on-call schedule by id.",
            "on_call",
            json!({"type": "object", "required": ["schedule_id"], "properties": {"schedule_id": {"type": "string"}}, "additionalProperties": false}),
            vec!["schedules.read"],
            vec!["On-call", "Schedules"],
        ),
        BuiltinToolKind::GetCurrentOnCall => (
            "Resolve current on-call owners for one schedule or all enabled schedules.",
            "on_call",
            object_schema(
                json!({"schedule_id": {"type": "string"}, "at_micros": {"type": "integer"}}),
            ),
            vec!["schedules.read"],
            vec!["On-call"],
        ),
        _ => unreachable!("incident catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "alerts_on_call",
        category,
        input,
        open_output(),
        &permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::GetIncident => tool.pinned(),
        BuiltinToolKind::AcknowledgeIncident | BuiltinToolKind::ResolveIncident => {
            tool.managed_mutation(RiskLevel::L2, false, true)
        }
        _ => tool,
    }
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
