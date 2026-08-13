// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::ListSyntheticMonitors => (
            "List synthetic monitors and current lifecycle state.",
            "monitors",
            object_schema(json!({"lifecycle": {"type": "string"}, "limit": limit(500, 100)})),
            vec!["synthetics.read"],
            vec!["Synthetics", "Monitors"],
        ),
        BuiltinToolKind::GetSyntheticMonitor => (
            "Get one synthetic monitor and its active revision.",
            "monitors",
            id_input("monitor_id"),
            vec!["synthetics.read"],
            vec!["Synthetics", "Monitors"],
        ),
        BuiltinToolKind::ListSyntheticRevisions => (
            "List bounded revisions for one synthetic monitor.",
            "monitors",
            json!({"type": "object", "required": ["monitor_id"], "properties": {
                "monitor_id": {"type": "string"}, "limit": limit(200, 50)
            }, "additionalProperties": false}),
            vec!["synthetics.read"],
            vec!["Synthetics", "Revisions"],
        ),
        BuiltinToolKind::ListSyntheticResults => (
            "List bounded synthetic execution results.",
            "results",
            object_schema(json!({
                "monitor_id": {"type": "string"}, "location_id": {"type": "string"},
                "time_range": time_range_schema(), "limit": limit(500, 100)
            })),
            vec!["synthetics.read"],
            vec!["Synthetics", "Results"],
        ),
        BuiltinToolKind::ListSyntheticLocations => (
            "List synthetic probe locations.",
            "locations",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["synthetics.read"],
            vec!["Synthetics", "Locations"],
        ),
        BuiltinToolKind::ListSyntheticAgents => (
            "List probe agents, optionally for one location.",
            "agents",
            object_schema(json!({"location_id": {"type": "string"}, "limit": limit(500, 100)})),
            vec!["synthetics.read"],
            vec!["Synthetics", "Agents"],
        ),
        BuiltinToolKind::ListSyntheticSecrets => (
            "List synthetic-secret metadata; secret values are never returned.",
            "secrets",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["synthetics.read"],
            vec!["Synthetics", "Secrets"],
        ),
        BuiltinToolKind::RunSyntheticMonitor => operation(
            "Run one synthetic monitor immediately.",
            &["Synthetics", "Monitors", "Run"],
        ),
        BuiltinToolKind::PauseSyntheticMonitor => operation(
            "Pause one synthetic monitor.",
            &["Synthetics", "Monitors", "Pause"],
        ),
        BuiltinToolKind::ResumeSyntheticMonitor => operation(
            "Resume one synthetic monitor.",
            &["Synthetics", "Monitors", "Resume"],
        ),
        BuiltinToolKind::ArchiveSyntheticMonitor => operation(
            "Archive one synthetic monitor.",
            &["Synthetics", "Monitors", "Archive"],
        ),
        _ => unreachable!("synthetics catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "synthetics",
        category,
        input,
        open_output(),
        &permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::RunSyntheticMonitor
        | BuiltinToolKind::PauseSyntheticMonitor
        | BuiltinToolKind::ResumeSyntheticMonitor => {
            tool.managed_mutation(RiskLevel::L2, false, false)
        }
        BuiltinToolKind::ArchiveSyntheticMonitor => {
            tool.managed_mutation(RiskLevel::L3, true, true)
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
    Vec<&'static str>,
) {
    (
        description,
        "monitors",
        proposal_input("monitor_id"),
        vec!["synthetics.manage"],
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

fn limit(maximum: u32, default: u32) -> serde_json::Value {
    json!({"type": "integer", "minimum": 1, "maximum": maximum, "default": default})
}
