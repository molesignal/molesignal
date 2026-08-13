// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let common = json!({
        "time_range": time_range_schema(), "application": {"type": "string"},
        "environment": {"type": "string"},
        "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
    });
    let (description, input, tags) = match kind {
        BuiltinToolKind::ListRumSessions => {
            let mut fields = common.clone();
            fields["end_user_id"] = json!({"type": "string"});
            (
                "List bounded real-user monitoring sessions.",
                object_schema(fields),
                vec!["RUM", "Sessions"],
            )
        }
        BuiltinToolKind::GetRumSession => (
            "Get one RUM session with a bounded action and error summary.",
            json!({"type": "object", "required": ["session_id"], "properties": {"session_id": {"type": "string"}, "time_range": time_range_schema(), "event_limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}, "additionalProperties": false}),
            vec!["RUM", "Sessions"],
        ),
        BuiltinToolKind::ListRumActions => {
            let mut fields = common.clone();
            fields["session_id"] = json!({"type": "string"});
            fields["action_type"] = json!({"type": "string"});
            (
                "List page views, actions, resources, and Web Vitals.",
                object_schema(fields),
                vec!["RUM", "Actions"],
            )
        }
        BuiltinToolKind::ListRumErrors => {
            let mut fields = common;
            fields["session_id"] = json!({"type": "string"});
            fields["fingerprint"] = json!({"type": "string"});
            (
                "List RUM errors by session or fingerprint.",
                object_schema(fields),
                vec!["RUM", "Errors"],
            )
        }
        BuiltinToolKind::GetRumRelatedTraces => (
            "Find traces directly linked to a RUM session, with bounded time-correlation fallback.",
            json!({"type": "object", "required": ["session_id"], "properties": {"session_id": {"type": "string"}, "time_range": time_range_schema(), "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}}, "additionalProperties": false}),
            vec!["RUM", "Traces", "Correlation"],
        ),
        _ => unreachable!("RUM catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "rum",
        input,
        open_output(),
        &["streams.query", "sys.telemetry.read"],
        &tags,
    )
    .any_permission()
}
