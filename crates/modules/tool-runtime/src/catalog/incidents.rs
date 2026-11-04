// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema, timestamp_schema};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::ListRecentAlerts => (
            "List active alert incidents. Compatibility alias for list_incidents with status=active.",
            "incidents",
            object_schema(
                json!({"limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}}),
            ),
            vec!["alerts.incidents.read"],
            vec!["Alerts", "Compatibility"],
        ),
        BuiltinToolKind::ListIncidents => (
            "List incidents by status and optional time range, priority and urgency, with bounded results.",
            "incidents",
            object_schema(json!({
                "status": {"type": "string", "enum": ["active", "resolved", "all"], "default": "active"},
                "timeRange": time_range_schema(),
                "priorities": {"type": "array", "items": {"type": "string", "pattern": "^P[1-9]$"}, "maxItems": 9},
                "urgency": {"type": "string", "pattern": "^[a-z][a-z0-9_]{0,31}$", "description": "An urgency key of the organization: high, low, or one it added."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
            })),
            vec!["alerts.incidents.read"],
            vec!["Incidents", "Alerts"],
        ),
        BuiltinToolKind::GetIncident => (
            "Get one incident and its alert-rule context by incident id.",
            "incidents",
            json!({"type": "object", "required": ["incidentId"], "properties": {"incidentId": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.incidents.read"],
            vec!["Incidents"],
        ),
        BuiltinToolKind::GetIncidentRca => (
            "Get the latest persisted root-cause analysis for an incident.",
            "incidents",
            json!({"type": "object", "required": ["incidentId"], "properties": {"incidentId": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.incidents.read"],
            vec!["Incidents", "RCA"],
        ),
        BuiltinToolKind::GetIncidentInsights => (
            "Return bounded incident counts grouped by status, severity, service, and rule.",
            "incidents",
            object_schema(json!({
                "timeRange": time_range_schema(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
            })),
            vec!["alerts.incidents.read"],
            vec!["Incidents", "Insights"],
        ),
        BuiltinToolKind::AcknowledgeIncident => (
            "Acknowledge one open incident.",
            "incidents",
            proposal_input("incidentId"),
            vec!["alerts.incidents.acknowledge"],
            vec!["Incidents", "Acknowledge"],
        ),
        BuiltinToolKind::ResolveIncident => (
            "Resolve one open or acknowledged incident.",
            "incidents",
            proposal_input("incidentId"),
            vec!["alerts.incidents.acknowledge"],
            vec!["Incidents", "Resolve"],
        ),
        BuiltinToolKind::ReassignIncident => (
            "Reassign one active incident to people, a team (its on-call policy) or an escalation policy.",
            "incidents",
            proposal_with(
                json!({
                    "to": {
                        "type": "object",
                        "required": ["kind"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["users", "team", "policy"]},
                            "userIds": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 10},
                            "teamId": {"type": "string"},
                            "escalationPolicyId": {"type": "string"}
                        },
                        "additionalProperties": false
                    }
                }),
                &["to"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Reassign"],
        ),
        BuiltinToolKind::AddIncidentResponders => (
            "Add people to the assignees of one active incident and notify them.",
            "incidents",
            proposal_with(
                json!({
                    "userIds": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 10}
                }),
                &["userIds"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Responders"],
        ),
        BuiltinToolKind::EscalateIncident => (
            "Escalate one active incident now to a step of its escalation policy (the next step by default).",
            "incidents",
            proposal_with(
                json!({
                    "step": {"type": "integer", "minimum": 0, "description": "Step to escalate to, counted from 0."}
                }),
                &[],
            ),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Escalate"],
        ),
        BuiltinToolKind::SnoozeIncident => (
            "Snooze one active incident: it is acknowledged and comes back unacknowledged when the time is up.",
            "incidents",
            proposal_with(
                json!({
                    "durationSecs": {"type": "integer", "minimum": 60, "maximum": 604800}
                }),
                &["durationSecs"],
            ),
            vec!["alerts.incidents.acknowledge"],
            vec!["Incidents", "Snooze"],
        ),
        BuiltinToolKind::LiftIncidentInhibition => (
            "Stop an inhibit rule from holding back one active incident: its held notifications go out and the incident is not inhibited again.",
            "incidents",
            proposal_with(json!({}), &[]),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Inhibitions"],
        ),
        BuiltinToolKind::SetIncidentSeverity => (
            "Set the severity of one active incident; the rule no longer changes it afterwards.",
            "incidents",
            proposal_with(
                json!({
                    "severity": {"type": "string", "enum": ["info", "warning", "error", "critical"]}
                }),
                &["severity"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Severity"],
        ),
        BuiltinToolKind::SetIncidentPriority => (
            "Set the priority (P1 to Pn of the organization's levels, or none) of one active incident; it \
             changes no notification.",
            "incidents",
            proposal_with(
                json!({
                    "priority": {
                        "type": ["string", "null"],
                        "pattern": "^P[1-9]$"
                    }
                }),
                &["priority"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Priority"],
        ),
        BuiltinToolKind::SetIncidentUrgency => (
            "Set the urgency of one active incident to a level of the organization (high, low, or one \
             it added); raised to a more urgent level, an unacknowledged one is paged again from the \
             first level.",
            "incidents",
            proposal_with(
                json!({
                    "urgency": {"type": "string", "pattern": "^[a-z][a-z0-9_]{0,31}$"}
                }),
                &["urgency"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Urgency"],
        ),
        BuiltinToolKind::CreateIncident => (
            "Open an incident by hand for a problem no rule caught, such as a customer report; it \
             pages and escalates like any other. Routed like a fired alert unless a team, an \
             escalation policy or people are named.",
            "incidents",
            create_incident_input(),
            vec!["alerts.incidents.create"],
            vec!["Incidents", "Create"],
        ),
        BuiltinToolKind::MergeIncidents => (
            "Merge other active incidents into one: they stop notifying and are resolved with it.",
            "incidents",
            proposal_with(
                json!({
                    "sourceIds": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 50}
                }),
                &["sourceIds"],
            ),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Merge"],
        ),
        BuiltinToolKind::AddIncidentNote => (
            "Write a note on the timeline of one incident: what was found, what was done.",
            "incidents",
            proposal_with(
                json!({
                    "text": {"type": "string", "minLength": 1, "maxLength": 4000}
                }),
                &["text"],
            ),
            vec!["alerts.incidents.update"],
            vec!["Incidents", "Notes"],
        ),
        BuiltinToolKind::GetIncidentTickets => (
            "List the tickets made for one incident in Jira, ServiceNow or Meegle, with their status.",
            "incidents",
            json!({"type": "object", "required": ["incidentId"], "properties": {"incidentId": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.incidents.read"],
            vec!["Incidents", "Tickets"],
        ),
        BuiltinToolKind::CreateTicket => (
            "Make a ticket for one incident with a Jira, ServiceNow or Meegle connector (one per connector).",
            "incidents",
            proposal_with(json!({"connectorId": {"type": "string"}}), &["connectorId"]),
            vec!["alerts.incidents.tickets"],
            vec!["Incidents", "Tickets"],
        ),
        BuiltinToolKind::ListRemediationActions => (
            "List the remediation actions one incident can run, with the parameters each asks for.",
            "incidents",
            json!({"type": "object", "required": ["incidentId"], "properties": {"incidentId": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.remediation.read"],
            vec!["Incidents", "Remediation"],
        ),
        BuiltinToolKind::RunRemediationAction => (
            "Propose running a remediation action on one incident; it always waits for people to approve it.",
            "incidents",
            proposal_with(
                json!({
                    "actionId": {"type": "string"},
                    "inputs": {"type": "object", "additionalProperties": {"type": "string"}}
                }),
                &["actionId"],
            ),
            vec!["alerts.incidents.remediate"],
            vec!["Incidents", "Remediation"],
        ),
        BuiltinToolKind::DeclareMajorIncident => (
            "Declare one incident a major incident: roles, a war room, the checklist and stakeholder updates.",
            "incidents",
            proposal_with(json!({"commanderId": {"type": "string"}}), &[]),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Major incident"],
        ),
        BuiltinToolKind::AssignIncidentRole => (
            "Assign the commander, communications lead or scribe of a major incident, or clear the role.",
            "incidents",
            proposal_with(
                json!({
                    "role": {"type": "string", "enum": ["commander", "communications", "scribe"]},
                    "assigneeId": {"type": ["string", "null"]}
                }),
                &["role"],
            ),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Major incident"],
        ),
        BuiltinToolKind::PostIncidentUpdate => (
            "Post a status update on a major incident (impact, where things are, next steps) to its war room and stakeholders.",
            "incidents",
            proposal_with(
                json!({
                    "impact": {"type": "string", "maxLength": 2000},
                    "current": {"type": "string", "maxLength": 2000},
                    "next": {"type": "string", "maxLength": 2000}
                }),
                &[],
            ),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Major incident"],
        ),
        BuiltinToolKind::AddIncidentTask => (
            "Add a task to a major incident's task list.",
            "incidents",
            proposal_with(
                json!({"title": {"type": "string", "minLength": 1, "maxLength": 200}}),
                &["title"],
            ),
            vec!["alerts.incidents.escalate"],
            vec!["Incidents", "Major incident"],
        ),
        BuiltinToolKind::DraftIncidentPostmortem => (
            "Save a Markdown draft of an incident's internal postmortem (summary, impact, timeline, root cause, what went well and badly, action items); a published one is left as it is.",
            "incidents",
            proposal_with(
                json!({
                    "title": {"type": "string", "maxLength": 200},
                    "body": {"type": "string", "minLength": 1, "maxLength": 100000}
                }),
                &["body"],
            ),
            vec!["postmortems.manage"],
            vec!["Incidents", "Postmortem"],
        ),
        BuiltinToolKind::GetIncidentTimeline => (
            "Get the timeline of one incident: who was notified, what people did, notes, and the incidents merged into it.",
            "incidents",
            json!({"type": "object", "required": ["incidentId"], "properties": {"incidentId": {"type": "string"}}, "additionalProperties": false}),
            vec!["alerts.incidents.read"],
            vec!["Incidents", "Timeline"],
        ),
        BuiltinToolKind::ListOnCallSchedules => (
            "List on-call schedules and current assignees.",
            "on_call",
            object_schema(
                json!({"enabledOnly": {"type": "boolean", "default": true}, "atTime": timestamp_schema()}),
            ),
            vec!["schedules.read"],
            vec!["On-call", "Schedules"],
        ),
        BuiltinToolKind::GetOnCallSchedule => (
            "Get one on-call schedule by id.",
            "on_call",
            json!({"type": "object", "required": ["scheduleId"], "properties": {"scheduleId": {"type": "string"}}, "additionalProperties": false}),
            vec!["schedules.read"],
            vec!["On-call", "Schedules"],
        ),
        BuiltinToolKind::GetCurrentOnCall => (
            "Resolve current on-call owners for one schedule or all enabled schedules.",
            "on_call",
            object_schema(json!({"scheduleId": {"type": "string"}, "atTime": timestamp_schema()})),
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
        BuiltinToolKind::AcknowledgeIncident
        | BuiltinToolKind::ResolveIncident
        | BuiltinToolKind::ReassignIncident
        | BuiltinToolKind::AddIncidentResponders
        | BuiltinToolKind::EscalateIncident
        | BuiltinToolKind::SnoozeIncident
        | BuiltinToolKind::LiftIncidentInhibition
        | BuiltinToolKind::SetIncidentSeverity
        | BuiltinToolKind::SetIncidentPriority
        | BuiltinToolKind::SetIncidentUrgency
        | BuiltinToolKind::CreateIncident => tool.managed_mutation(RiskLevel::L2, false, true),
        // A merge cannot be undone.
        BuiltinToolKind::MergeIncidents => tool.managed_mutation(RiskLevel::L2, true, true),
        BuiltinToolKind::AddIncidentNote
        | BuiltinToolKind::CreateTicket
        | BuiltinToolKind::AssignIncidentRole
        | BuiltinToolKind::AddIncidentTask
        | BuiltinToolKind::DraftIncidentPostmortem => {
            tool.managed_mutation(RiskLevel::L1, false, false)
        }
        BuiltinToolKind::DeclareMajorIncident | BuiltinToolKind::PostIncidentUpdate => {
            tool.managed_mutation(RiskLevel::L2, false, true)
        }
        // A remediation action changes production systems.
        BuiltinToolKind::RunRemediationAction => tool.managed_mutation(RiskLevel::L3, true, true),
        _ => tool,
    }
}

/// 一个事件的提议操作：事件 ID、理由与影响，再加上这个操作自己的参数。
fn proposal_with(extra: serde_json::Value, required: &[&str]) -> serde_json::Value {
    let mut schema = proposal_input("incidentId");

    if let (Some(properties), Some(extra)) =
        (schema["properties"].as_object_mut(), extra.as_object())
    {
        properties.extend(extra.clone());
    }

    if let Some(list) = schema["required"].as_array_mut() {
        list.extend(required.iter().map(|field| json!(field)));
    }

    schema
}

/// 手动建事件：没有目标事件，参数同 `POST /api/v1/alerts/incidents`。
fn create_incident_input() -> serde_json::Value {
    json!({
        "type": "object", "required": ["title", "reason", "impact"],
        "properties": {
            "title": {"type": "string", "minLength": 1, "maxLength": 200},
            "description": {"type": "string", "maxLength": 20000, "description": "Markdown."},
            "severity": {"type": "string", "enum": ["critical", "error", "warning", "info"], "default": "error"},
            "urgency": {"type": "string", "pattern": "^[a-z][a-z0-9_]{0,31}$", "description": "An urgency key of the organization (high, low, or one it added). Left out, the escalation policy decides."},
            "priority": {"type": "string", "pattern": "^P[1-9]$", "description": "P1 to Pn of the organization's priority levels."},
            "assign": {
                "type": "object",
                "required": ["kind"],
                "properties": {
                    "kind": {"type": "string", "enum": ["auto", "team", "policy", "users"]},
                    "teamId": {"type": "string"},
                    "escalationPolicyId": {"type": "string"},
                    "userIds": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 10}
                },
                "additionalProperties": false
            },
            "services": {"type": "array", "items": {"type": "string"}, "maxItems": 20},
            "labels": {"type": "object", "additionalProperties": {"type": "string"}},
            "declareMajor": {"type": "boolean", "default": false},
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expiresAt": timestamp_schema()
        },
        "additionalProperties": false
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
