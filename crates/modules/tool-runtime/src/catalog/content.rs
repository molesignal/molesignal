// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::{RiskLevel, ToolSpec};

mod mutations;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    if matches!(
        kind,
        BuiltinToolKind::CreateDashboard
            | BuiltinToolKind::UpdateDashboard
            | BuiltinToolKind::DeleteDashboard
            | BuiltinToolKind::CreateFolder
            | BuiltinToolKind::UpdateFolder
            | BuiltinToolKind::DeleteFolder
    ) {
        return mutations::spec(kind);
    }
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::ListDashboards => entry(
            "List dashboards, optionally within one folder.",
            "dashboards",
            object_schema(json!({
                "folder_id": {"type": "string"}, "limit": limit()
            })),
            &["dashboards.read", "sys.dashboards.read"],
            &["Dashboard"],
        ),
        BuiltinToolKind::GetDashboard => id_entry(
            "Get a Dashboard and its complete model by id.",
            "dashboards",
            "dashboard_id",
            &["dashboards.read", "sys.dashboards.read"],
            &["Dashboard"],
        ),
        BuiltinToolKind::ListFolders => entry(
            "List Dashboard folders.",
            "folders",
            object_schema(json!({"limit": limit()})),
            &["dashboards.read", "sys.dashboards.read"],
            &["Dashboard", "Folders"],
        ),
        BuiltinToolKind::GetFolder => id_entry(
            "Get one Dashboard folder by id.",
            "folders",
            "folder_id",
            &["dashboards.read", "sys.dashboards.read"],
            &["Dashboard", "Folders"],
        ),
        BuiltinToolKind::ListAnnotations => entry(
            "List bounded time annotations for a range and optional Dashboard or panel.",
            "annotations",
            object_schema(json!({
                "time_range": time_range_schema(), "dashboard_id": {"type": "string"},
                "limit": limit()
            })),
            &["dashboards.read"],
            &["Dashboard", "Annotations"],
        ),
        BuiltinToolKind::GetAnnotation => id_entry(
            "Get one time annotation by id.",
            "annotations",
            "annotation_id",
            &["dashboards.read"],
            &["Dashboard", "Annotations"],
        ),
        BuiltinToolKind::CreateAnnotation => entry(
            "Create a time annotation through the configured execution policy.",
            "annotations",
            annotation_create_input(),
            &["dashboards.edit"],
            &["Dashboard", "Annotations", "Write"],
        ),
        BuiltinToolKind::UpdateAnnotation => entry(
            "Update selected fields of one time annotation through the configured execution policy.",
            "annotations",
            annotation_update_input(),
            &["dashboards.edit"],
            &["Dashboard", "Annotations", "Write"],
        ),
        BuiltinToolKind::DeleteAnnotation => entry(
            "Delete one time annotation through the configured execution policy.",
            "annotations",
            proposal_id_input("annotation_id"),
            &["dashboards.edit"],
            &["Dashboard", "Annotations", "Delete"],
        ),
        BuiltinToolKind::AddDashboardPanel => entry(
            "Add one panel to a Dashboard root or nested container atomically.",
            "dashboards",
            panel_add_input(),
            &["dashboards.edit"],
            &["Dashboard", "Panels", "Write"],
        ),
        BuiltinToolKind::UpdateDashboardPanel => entry(
            "Replace one Dashboard panel atomically.",
            "dashboards",
            panel_update_input(),
            &["dashboards.edit"],
            &["Dashboard", "Panels", "Write"],
        ),
        BuiltinToolKind::MoveDashboardPanel => entry(
            "Move one Dashboard panel to a root or nested container position atomically.",
            "dashboards",
            panel_move_input(),
            &["dashboards.edit"],
            &["Dashboard", "Panels", "Write"],
        ),
        BuiltinToolKind::DeleteDashboardPanel => entry(
            "Delete one Dashboard panel atomically.",
            "dashboards",
            panel_delete_input(),
            &["dashboards.edit"],
            &["Dashboard", "Panels", "Delete"],
        ),
        _ => unreachable!("content catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "dashboard_reports",
        category,
        input,
        open_output(),
        &permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::ListDashboards
        | BuiltinToolKind::GetDashboard
        | BuiltinToolKind::ListFolders
        | BuiltinToolKind::GetFolder => tool.any_permission(),
        BuiltinToolKind::CreateAnnotation | BuiltinToolKind::MoveDashboardPanel => {
            tool.managed_mutation(RiskLevel::L1, false, false)
        }
        BuiltinToolKind::UpdateAnnotation
        | BuiltinToolKind::AddDashboardPanel
        | BuiltinToolKind::UpdateDashboardPanel => {
            tool.managed_mutation(RiskLevel::L2, false, false)
        }
        BuiltinToolKind::DeleteAnnotation | BuiltinToolKind::DeleteDashboardPanel => {
            tool.managed_mutation(RiskLevel::L3, true, true)
        }
        _ => tool,
    }
}

type Entry = (
    &'static str,
    &'static str,
    Value,
    Vec<&'static str>,
    Vec<&'static str>,
);

fn entry(
    description: &'static str,
    category: &'static str,
    input: Value,
    permissions: &[&'static str],
    tags: &[&'static str],
) -> Entry {
    (
        description,
        category,
        input,
        permissions.to_vec(),
        tags.to_vec(),
    )
}

fn id_entry(
    description: &'static str,
    category: &'static str,
    field: &'static str,
    permissions: &[&'static str],
    tags: &[&'static str],
) -> Entry {
    entry(description, category, id_input(field), permissions, tags)
}

fn limit() -> Value {
    json!({"type": "integer", "minimum": 1, "maximum": 500, "default": 100})
}

fn id_input(field: &str) -> Value {
    json!({"type": "object", "required": [field], "properties": {(field): {"type": "string"}}, "additionalProperties": false})
}

fn proposal_schema(required: Vec<&str>, properties: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.extend(
        json!({
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        })
        .as_object()
        .cloned()
        .expect("static approval schema"),
    );
    json!({
        "type": "object",
        "required": required.into_iter().chain(["reason", "impact"]).collect::<Vec<_>>(),
        "properties": properties,
        "additionalProperties": false
    })
}

fn proposal_id_input(field: &str) -> Value {
    proposal_schema(
        vec![field],
        json!({(field): {"type": "string", "minLength": 1}}),
    )
}

fn annotation_create_input() -> Value {
    proposal_schema(
        vec!["title", "time_start_micros", "time_end_micros"],
        json!({
            "title": {"type": "string", "minLength": 1, "maxLength": 500},
            "description": {"type": ["string", "null"]},
            "tags": {"type": "array", "maxItems": 64, "items": {"type": "string", "maxLength": 128}},
            "time_start_micros": {"type": "integer"}, "time_end_micros": {"type": "integer"},
            "dashboard_id": {"type": ["string", "null"]}, "stream_name": {"type": ["string", "null"]}
        }),
    )
}

fn annotation_update_input() -> Value {
    proposal_schema(
        vec!["annotation_id"],
        json!({
            "annotation_id": {"type": "string", "minLength": 1},
            "title": {"type": "string", "minLength": 1, "maxLength": 500},
            "description": {"type": ["string", "null"]},
            "tags": {"type": "array", "maxItems": 64, "items": {"type": "string", "maxLength": 128}},
            "time_start_micros": {"type": "integer"}, "time_end_micros": {"type": "integer"},
            "dashboard_id": {"type": ["string", "null"]}, "stream_name": {"type": ["string", "null"]}
        }),
    )
}

fn panel_add_input() -> Value {
    proposal_schema(
        vec!["dashboard_id", "panel"],
        json!({
            "dashboard_id": {"type": "string", "minLength": 1}, "panel": {"type": "object"},
            "container_id": {"type": ["string", "null"]}, "position": {"type": "integer", "minimum": 0},
            "expected_version": {"type": "integer", "minimum": 0}
        }),
    )
}

fn panel_update_input() -> Value {
    proposal_schema(
        vec!["dashboard_id", "panel_id", "panel"],
        json!({
            "dashboard_id": {"type": "string", "minLength": 1}, "panel_id": {"type": "string", "minLength": 1},
            "panel": {"type": "object"}, "expected_version": {"type": "integer", "minimum": 0}
        }),
    )
}

fn panel_move_input() -> Value {
    proposal_schema(
        vec!["dashboard_id", "panel_id", "position"],
        json!({
            "dashboard_id": {"type": "string", "minLength": 1}, "panel_id": {"type": "string", "minLength": 1},
            "container_id": {"type": ["string", "null"]}, "position": {"type": "integer", "minimum": 0},
            "expected_version": {"type": "integer", "minimum": 0}
        }),
    )
}

fn panel_delete_input() -> Value {
    proposal_schema(
        vec!["dashboard_id", "panel_id"],
        json!({
            "dashboard_id": {"type": "string", "minLength": 1}, "panel_id": {"type": "string", "minLength": 1},
            "expected_version": {"type": "integer", "minimum": 0}
        }),
    )
}
