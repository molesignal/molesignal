// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, open_output};
use crate::ToolSpec;

/// A chat answer keeps a task list and can stop to ask the person who asked. Only a Mole Agent
/// run carries them out: it shows the list and the question in the chat and waits for the answer.
pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    match kind {
        BuiltinToolKind::TodoWrite => ToolSpec::read(
            kind.name(),
            "Write the task list of this investigation: every step with its status. Send the whole list each time; mark one step in_progress at a time and a step completed as soon as it is done. Use it for work of three steps or more.",
            "platform",
            "session",
            json!({
                "type": "object",
                "required": ["items"],
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 20,
                        "items": {
                            "type": "object",
                            "required": ["content", "status"],
                            "properties": {
                                "content": {"type": "string", "maxLength": 200},
                                "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                            },
                            "additionalProperties": false
                        }
                    }
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Session"],
        )
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::AskUser => ToolSpec::read(
            kind.name(),
            "Ask the person who asked a question when the answer changes what to do next and cannot be found with the other tools: which environment, which service, whether a cause is plausible. Offer choices when there are a few likely answers. The run waits for the answer.",
            "platform",
            "session",
            json!({
                "type": "object",
                "required": ["question"],
                "properties": {
                    "question": {"type": "string", "maxLength": 500},
                    "kind": {"type": "string", "enum": ["single", "multiple", "text"], "default": "text"},
                    "options": {
                        "type": "array",
                        "maxItems": 8,
                        "items": {"type": "string", "maxLength": 120}
                    }
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Session"],
        )
        .pinned()
        .mole_agent_only(),
        _ => unreachable!("session catalog received a kind of another catalog"),
    }
}
