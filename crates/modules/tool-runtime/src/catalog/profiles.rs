// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let filters = json!({
        "time_range": time_range_schema(), "service": {"type": "string"},
        "profile_type": {"type": "string"}, "trace_id": {"type": "string"},
        "label": {"type": "string", "description": "label selector as key:value"}
    });
    let (description, input, tags) = match kind {
        BuiltinToolKind::ListContinuousProfiles => {
            let mut fields = filters.clone();
            fields["limit"] =
                json!({"type": "integer", "minimum": 1, "maximum": 1000, "default": 100});
            (
                "List continuous-profile metadata with service, type, trace, and label filters.",
                object_schema(fields),
                vec!["Profiles", "Metadata"],
            )
        }
        BuiltinToolKind::GetProfileFlamegraph => {
            let mut fields = filters.clone();
            fields["span_id"] = json!({"type": "string"});
            fields["max_profiles"] =
                json!({"type": "integer", "minimum": 1, "maximum": 1000, "default": 200});
            (
                "Merge a bounded profile selection into a flamebearer flamegraph.",
                object_schema(fields),
                vec!["Profiles", "Flamegraph"],
            )
        }
        BuiltinToolKind::CompareProfiles => (
            "Compare baseline and comparison profile windows as a differential flamegraph. Requires profiling_enhanced.",
            json!({
                "type": "object", "required": ["baseline", "comparison"],
                "properties": {
                    "baseline": time_range_schema(), "comparison": time_range_schema(),
                    "service": {"type": "string"}, "profile_type": {"type": "string"},
                    "label": {"type": "string"},
                    "max_profiles": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
                }, "additionalProperties": false
            }),
            vec!["Profiles", "Diff"],
        ),
        _ => unreachable!("profiles catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "profiles",
        input,
        open_output(),
        &["streams.query", "sys.telemetry.read"],
        &tags,
    )
    .any_permission()
}
