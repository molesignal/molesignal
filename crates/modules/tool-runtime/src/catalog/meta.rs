// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    match kind {
        BuiltinToolKind::ToolSearch => ToolSpec::read(
            kind.name(),
            "Search the enabled tool catalog by capability, domain, tag, or exact name. Use this when the required tool is not already visible.",
            "platform",
            "discovery",
            object_schema(json!({
                "query": {"type": "string", "description": "Capability, domain, tag, or exact tool name."},
                "domain": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 10},
                "include_schema": {"type": "boolean", "default": true}
            })),
            json!({"type": "object", "properties": {"tools": {"type": "array"}}}),
            &[],
            &["Discovery", "Meta"],
        )
        .meta(),
        BuiltinToolKind::ToolsCall => ToolSpec::read(
            kind.name(),
            "Call one enabled tool discovered with tool_search. The target tool keeps its own permissions, risk policy, timeout, and audit behavior.",
            "platform",
            "dispatch",
            json!({
                "type": "object",
                "required": ["name", "arguments"],
                "properties": {
                    "name": {"type": "string"},
                    "arguments": {"type": "object"}
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Dispatch", "Meta"],
        )
        .meta(),
        _ => unreachable!("meta catalog received non-meta kind"),
    }
}
