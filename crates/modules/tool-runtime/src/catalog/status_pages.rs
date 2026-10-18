// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::ListStatusPages => (
            "List status pages by lifecycle.",
            "pages",
            object_schema(
                json!({"lifecycle": {"type": "string", "enum": ["active", "archived"]}, "limit": limit(500, 100)}),
            ),
            vec!["Status Page", "Pages"],
        ),
        BuiltinToolKind::GetStatusPage => (
            "Get a status-page snapshot including components and current events.",
            "pages",
            id_input("page_id"),
            vec!["Status Page", "Snapshot"],
        ),
        BuiltinToolKind::ListStatusPageIncidents => (
            "List status-page incidents or maintenance events.",
            "incidents",
            json!({"type": "object", "required": ["page_id"], "properties": {
                "page_id": {"type": "string"}, "kind": {"type": "string", "enum": ["incident", "maintenance"], "default": "incident"},
                "view": {"type": "string", "enum": ["current", "draft", "resolved", "upcoming", "in_progress", "completed"], "default": "current"}
            }, "additionalProperties": false}),
            vec!["Status Page", "Incidents"],
        ),
        BuiltinToolKind::GetStatusPageIncident => (
            "Get one status-page event and its updates.",
            "incidents",
            json!({"type": "object", "required": ["page_id", "event_id"], "properties": {
                "page_id": {"type": "string"}, "event_id": {"type": "string"}
            }, "additionalProperties": false}),
            vec!["Status Page", "Incidents"],
        ),
        BuiltinToolKind::ListStatusPageSubscribers => (
            "List a bounded page of status-page subscribers.",
            "subscriptions",
            page_input(),
            vec!["Status Page", "Subscribers"],
        ),
        BuiltinToolKind::ListStatusPageDeliveries => (
            "List a bounded page of status-page notification deliveries.",
            "subscriptions",
            page_input(),
            vec!["Status Page", "Deliveries"],
        ),
        BuiltinToolKind::ListStatusPageAutomationRules => (
            "List active status-page automation rules.",
            "automation",
            id_input("page_id"),
            vec!["Status Page", "Automation"],
        ),
        BuiltinToolKind::ListStatusPageAutomationCandidates => (
            "List bounded status-page automation candidates.",
            "automation",
            json!({"type": "object", "required": ["page_id"], "properties": {
                "page_id": {"type": "string"}, "state": {"type": "string"}, "limit": limit(100, 50)
            }, "additionalProperties": false}),
            vec!["Status Page", "Automation"],
        ),
        BuiltinToolKind::GetStatusPageAutomationSettings => (
            "Get automation settings for one status page.",
            "automation",
            id_input("page_id"),
            vec!["Status Page", "Automation"],
        ),
        BuiltinToolKind::ArchiveStatusPage => operation(
            "Archive one status page.",
            &["Status Page", "Pages", "Archive"],
        ),
        BuiltinToolKind::RestoreStatusPage => operation(
            "Restore one archived status page.",
            &["Status Page", "Pages", "Restore"],
        ),
        BuiltinToolKind::PauseStatusPageAutomation => operation(
            "Pause automation for one status page.",
            &["Status Page", "Automation", "Pause"],
        ),
        BuiltinToolKind::ResumeStatusPageAutomation => operation(
            "Resume automation for one status page.",
            &["Status Page", "Automation", "Resume"],
        ),
        BuiltinToolKind::DraftStatusPagePostmortem => (
            "Write or replace the draft postmortem of one published status-page incident, in Markdown. The draft is not published: people publish it from the status page once the incident is resolved. A postmortem that is already published is revised there, not here.",
            "incidents",
            json!({
                "type": "object", "required": ["page_id", "event_id", "body", "reason", "impact"],
                "properties": {
                    "page_id": {"type": "string", "minLength": 1},
                    "event_id": {"type": "string", "minLength": 1, "description": "The incident's id, as list_status_page_incidents gives it"},
                    "body": {"type": "string", "minLength": 1, "maxLength": 20000},
                    "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "expires_at_micros": {"type": "integer"}
                }, "additionalProperties": false
            }),
            vec!["Status Page", "Incidents", "Postmortems"],
        ),
        _ => unreachable!("status-page catalog received unrelated kind"),
    };

    let permissions = if matches!(
        kind,
        BuiltinToolKind::ArchiveStatusPage
            | BuiltinToolKind::RestoreStatusPage
            | BuiltinToolKind::PauseStatusPageAutomation
            | BuiltinToolKind::ResumeStatusPageAutomation
            | BuiltinToolKind::DraftStatusPagePostmortem
    ) {
        &["status_pages.manage"][..]
    } else {
        &["status_pages.read"][..]
    };

    let tool = ToolSpec::read(
        kind.name(),
        description,
        "status_pages",
        category,
        input,
        open_output(),
        permissions,
        &tags,
    );

    match kind {
        BuiltinToolKind::RestoreStatusPage
        | BuiltinToolKind::PauseStatusPageAutomation
        | BuiltinToolKind::ResumeStatusPageAutomation => {
            tool.managed_mutation(RiskLevel::L2, false, true)
        }
        BuiltinToolKind::ArchiveStatusPage => tool.managed_mutation(RiskLevel::L3, true, true),
        // A draft is the organization's own text; nothing public changes until a person publishes it.
        BuiltinToolKind::DraftStatusPagePostmortem => {
            tool.managed_mutation(RiskLevel::L1, false, true)
        }
        _ => tool,
    }
}

fn operation(
    description: &'static str,
    tags: &[&'static str],
) -> (
    &'static str,
    &'static str,
    serde_json::Value,
    Vec<&'static str>,
) {
    (
        description,
        "pages",
        proposal_input("page_id"),
        tags.to_vec(),
    )
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

fn id_input(field: &str) -> serde_json::Value {
    json!({"type": "object", "required": [field], "properties": {(field): {"type": "string"}}, "additionalProperties": false})
}

fn page_input() -> serde_json::Value {
    json!({"type": "object", "required": ["page_id"], "properties": {
        "page_id": {"type": "string"}, "page": {"type": "integer", "minimum": 1, "maximum": 10000, "default": 1}
    }, "additionalProperties": false})
}

fn limit(maximum: u32, default: u32) -> serde_json::Value {
    json!({"type": "integer", "minimum": 1, "maximum": maximum, "default": default})
}
