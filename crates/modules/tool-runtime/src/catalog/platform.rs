// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    match kind {
        BuiltinToolKind::GetPlatformCapabilities => ToolSpec::read(
            kind.name(),
            "Return the complete builtin-tool domain inventory, exposure policy, and intentionally excluded unsafe entrypoints.",
            "platform",
            "capabilities",
            object_schema(json!({})),
            open_output(),
            &["agent.use"],
            &["Discovery", "Capabilities", "Safety"],
        ),
        _ => unreachable!("platform catalog received unrelated kind"),
    }
}
