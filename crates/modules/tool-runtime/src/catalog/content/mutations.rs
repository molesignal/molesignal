// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::super::{BuiltinToolKind, open_output, timestamp_schema};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permission, tags, risk, destructive, idempotent) = match kind
    {
        BuiltinToolKind::CreateDashboard => (
            "Create a Dashboard from a complete validated model.",
            "dashboards",
            proposal_schema(
                &["model"],
                json!({
                    "model": {"type": "object"},
                    "folderId": {"type": ["string", "null"]}
                }),
            ),
            "dashboards.create",
            &["Dashboard", "CRUD", "Create"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::UpdateDashboard => (
            "Replace one Dashboard model and optionally move it to another folder.",
            "dashboards",
            proposal_schema(
                &["dashboardId", "model"],
                json!({
                    "dashboardId": {"type": "string", "minLength": 1},
                    "model": {"type": "object"},
                    "folderId": {"type": ["string", "null"]}
                }),
            ),
            "dashboards.edit",
            &["Dashboard", "CRUD", "Update"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::DeleteDashboard => (
            "Delete one Dashboard.",
            "dashboards",
            proposal_id_schema("dashboardId"),
            "dashboards.delete",
            &["Dashboard", "CRUD", "Delete"][..],
            RiskLevel::L3,
            true,
            true,
        ),
        BuiltinToolKind::CreateFolder => (
            "Create a Dashboard folder.",
            "folders",
            proposal_schema(
                &["name"],
                json!({
                    "name": {"type": "string", "minLength": 1, "maxLength": 255},
                    "parentId": {"type": ["string", "null"]}
                }),
            ),
            "dashboards.create",
            &["Dashboard", "Folders", "CRUD", "Create"][..],
            RiskLevel::L1,
            false,
            false,
        ),
        BuiltinToolKind::UpdateFolder => (
            "Rename or move one Dashboard folder.",
            "folders",
            proposal_schema(
                &["folderId", "name"],
                json!({
                    "folderId": {"type": "string", "minLength": 1},
                    "name": {"type": "string", "minLength": 1, "maxLength": 255},
                    "parentId": {"type": ["string", "null"]}
                }),
            ),
            "dashboards.edit",
            &["Dashboard", "Folders", "CRUD", "Update"][..],
            RiskLevel::L2,
            false,
            false,
        ),
        BuiltinToolKind::DeleteFolder => (
            "Delete one empty Dashboard folder.",
            "folders",
            proposal_id_schema("folderId"),
            "dashboards.delete",
            &["Dashboard", "Folders", "CRUD", "Delete"][..],
            RiskLevel::L3,
            true,
            true,
        ),
        _ => unreachable!("content mutation catalog received unrelated kind"),
    };

    ToolSpec::read(
        kind.name(),
        description,
        "dashboard_reports",
        category,
        input,
        open_output(),
        &[permission],
        tags,
    )
    .managed_mutation(risk, destructive, idempotent)
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
            "expiresAt": timestamp_schema()
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
