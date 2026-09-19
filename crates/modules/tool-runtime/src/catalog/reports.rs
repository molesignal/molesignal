// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, input, tags) = match kind {
        BuiltinToolKind::ListReportTemplates => (
            "List built-in and organization report templates.",
            object_schema(json!({})),
            vec!["Reports", "Templates"],
        ),
        BuiltinToolKind::GetReportTemplate => (
            "Get one report template by id.",
            json!({"type": "object", "required": ["template_id"], "properties": {"template_id": {"type": "string"}}, "additionalProperties": false}),
            vec!["Reports", "Templates"],
        ),
        BuiltinToolKind::ListScheduledReports => (
            "List scheduled reports in the current organization.",
            object_schema(json!({"enabled_only": {"type": "boolean", "default": false}})),
            vec!["Reports", "Schedules"],
        ),
        BuiltinToolKind::GetScheduledReport => (
            "Get one scheduled report by id.",
            json!({"type": "object", "required": ["report_id"], "properties": {"report_id": {"type": "string"}}, "additionalProperties": false}),
            vec!["Reports", "Schedules"],
        ),
        BuiltinToolKind::ListReportDeliveries => (
            "List bounded delivery history for one scheduled report.",
            json!({"type": "object", "required": ["report_id"], "properties": {
                "report_id": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            }, "additionalProperties": false}),
            vec!["Reports", "Deliveries"],
        ),
        _ => unreachable!("reports catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "dashboard_reports",
        "reports",
        input,
        open_output(),
        &["reports.read"],
        &tags,
    )
}
