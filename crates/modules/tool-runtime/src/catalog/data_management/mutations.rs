// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::super::{BuiltinToolKind, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permission, tags, risk, destructive, idempotent) = match kind
    {
        BuiltinToolKind::CreateSavedView => (
            "Create a saved SQL or PromQL view.",
            "saved_views",
            saved_view_schema(None),
            "saved_views.create",
            &["Search", "Saved views", "CRUD", "Create"][..],
            RiskLevel::L1,
            false,
            false,
        ),
        BuiltinToolKind::UpdateSavedView => (
            "Replace one saved SQL or PromQL view.",
            "saved_views",
            saved_view_schema(Some("view_id")),
            "saved_views.edit",
            &["Search", "Saved views", "CRUD", "Update"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::DeleteSavedView => (
            "Delete one saved query view.",
            "saved_views",
            proposal_id_schema("view_id"),
            "saved_views.delete",
            &["Search", "Saved views", "CRUD", "Delete"][..],
            RiskLevel::L3,
            true,
            true,
        ),
        BuiltinToolKind::CreateFunction => (
            "Create a processing function after compilation precheck.",
            "functions",
            function_schema(None),
            "functions.create",
            &["Functions", "CRUD", "Create"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::UpdateFunction => (
            "Replace one organization processing function after compilation precheck.",
            "functions",
            function_schema(Some("function_id")),
            "functions.edit",
            &["Functions", "CRUD", "Update"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::DeleteFunction => (
            "Delete one organization processing function.",
            "functions",
            proposal_id_schema("function_id"),
            "functions.delete",
            &["Functions", "CRUD", "Delete"][..],
            RiskLevel::L3,
            true,
            true,
        ),
        _ => unreachable!("data mutation catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "data_management",
        category,
        input,
        open_output(),
        &[permission],
        tags,
    )
    .managed_mutation(risk, destructive, idempotent)
}

fn saved_view_schema(id_field: Option<&str>) -> Value {
    let mut required = vec!["name", "language", "statement", "time_range_secs"];
    let mut properties = json!({
        "name": {"type": "string", "minLength": 1, "maxLength": 255},
        "language": {"type": "string", "enum": ["sql", "promql"]},
        "statement": {"type": "string", "minLength": 1},
        "time_range_secs": {"type": "integer", "minimum": 1, "maximum": 31622400},
        "stream": {"type": ["string", "null"], "maxLength": 255},
        "tags": {"type": "array", "items": {"type": "string"}},
        "pinned": {"type": "boolean", "default": false}
    });
    if let Some(field) = id_field {
        required.insert(0, field);
        properties
            .as_object_mut()
            .expect("saved view properties")
            .insert(field.into(), json!({"type": "string", "minLength": 1}));
    }
    proposal_schema(&required, properties)
}

fn function_schema(id_field: Option<&str>) -> Value {
    let mut required = vec!["name", "language", "source"];
    let mut properties = json!({
        "name": {"type": "string", "minLength": 1},
        "language": {"type": "string", "enum": ["vrl", "js", "llm"]},
        "source": {"type": "string"},
        "params_schema": {"type": "object"}
    });
    if let Some(field) = id_field {
        required.insert(0, field);
        properties
            .as_object_mut()
            .expect("function properties")
            .insert(field.into(), json!({"type": "string", "minLength": 1}));
    }
    proposal_schema(&required, properties)
}

fn proposal_id_schema(field: &str) -> Value {
    proposal_schema(
        &[field],
        json!({(field): {"type": "string", "minLength": 1}}),
    )
}

fn proposal_schema(required: &[&str], properties: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.extend(
        json!({
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        })
        .as_object()
        .cloned()
        .expect("static approval properties"),
    );
    let mut required = required.to_vec();
    required.extend(["reason", "impact"]);
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
        "additionalProperties": false
    })
}
