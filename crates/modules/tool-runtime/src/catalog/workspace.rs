// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::{BuiltinToolKind, open_output};
use crate::{RiskLevel, ToolSpec};

/// Knowledge, memory and the investigation record belong to the chat they are used in: which
/// team's knowledge it loads, whose memory it writes and which investigation it records all
/// follow from the chat. Only a Mole Agent run carries them out.
pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    match kind {
        BuiltinToolKind::ListKnowledge => ToolSpec::read(
            kind.name(),
            "List the knowledge files of the organization or of a team: path, size and when each was changed. MOLE.md is the entry file that says what the others hold.",
            "platform",
            "knowledge",
            scope_schema(json!({}), &[]),
            open_output(),
            &[],
            &["Knowledge"],
        )
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::ReadKnowledge => ToolSpec::read(
            kind.name(),
            "Read one knowledge file by its path, such as runbooks/payments.md. Paths written as @path in MOLE.md are knowledge files.",
            "platform",
            "knowledge",
            scope_schema(
                json!({ "path": {"type": "string", "maxLength": 512} }),
                &["path"],
            ),
            open_output(),
            &[],
            &["Knowledge"],
        )
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::SearchKnowledge => ToolSpec::read(
            kind.name(),
            "Search the knowledge files for words, such as a service name or an error. Returns matching lines with their file and line number.",
            "platform",
            "knowledge",
            scope_schema(
                json!({
                    "query": {"type": "string", "maxLength": 200},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                }),
                &["query"],
            ),
            open_output(),
            &[],
            &["Knowledge"],
        )
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::EditKnowledge => ToolSpec::read(
            kind.name(),
            "Write or delete one knowledge file of this chat's scope: a team chat writes its team's knowledge, a personal chat the organization's. Explain in the chat which file you change and how before calling it; the person who asked confirms each change.",
            "platform",
            "knowledge",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string", "maxLength": 512},
                    "content": {"type": "string", "description": "The whole new content of the file."},
                    "delete": {"type": "boolean", "description": "Delete the file instead of writing it."},
                    "summary": {"type": "string", "maxLength": 300, "description": "One sentence on what changes, shown in the confirmation."}
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Knowledge"],
        )
        .chat_record(RiskLevel::L1, false)
        .mole_agent_only(),
        BuiltinToolKind::Remember => ToolSpec::read(
            kind.name(),
            "Remember something for later chats when the person asks you to, or when you learn a lasting preference, procedure, fact or lesson. A personal chat writes the person's memory, a team chat the team's. A memory with the same name is replaced.",
            "platform",
            "memory",
            json!({
                "type": "object",
                "required": ["kind", "name", "description", "body"],
                "properties": {
                    "kind": {"type": "string", "enum": ["preference", "procedure", "fact", "lesson"]},
                    "name": {"type": "string", "maxLength": 120},
                    "description": {"type": "string", "maxLength": 300, "description": "One sentence that says when it applies."},
                    "body": {"type": "string", "maxLength": 4000}
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Memory"],
        )
        .chat_record(RiskLevel::L0, false)
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::Forget => ToolSpec::read(
            kind.name(),
            "Forget a memory of this chat's scope by its name, when the person asks you to or it turned out to be wrong.",
            "platform",
            "memory",
            json!({
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string", "maxLength": 120}},
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Memory"],
        )
        .chat_record(RiskLevel::L0, true)
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::ReadMemory => ToolSpec::read(
            kind.name(),
            "Read the body of a memory listed in the system prompt, by its name.",
            "platform",
            "memory",
            json!({
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string", "maxLength": 120}},
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Memory"],
        )
        .pinned()
        .mole_agent_only(),
        BuiltinToolKind::UpdateInvestigation => ToolSpec::read(
            kind.name(),
            "Record the investigation this chat belongs to: a step you took, a piece of evidence, a hypothesis and how well it holds, or the conclusion. The record is shown on the investigation page.",
            "platform",
            "investigation",
            json!({
                "type": "object",
                "required": ["record"],
                "properties": {
                    "record": {"type": "string", "enum": ["step", "evidence", "hypothesis", "conclusion"]},
                    "title": {"type": "string", "maxLength": 200, "description": "Step title, evidence label, or hypothesis statement."},
                    "summary": {"type": "string", "maxLength": 4000, "description": "What the step found, what the evidence shows, or the conclusion."},
                    "status": {"type": "string", "description": "Step: succeeded, failed or skipped. Hypothesis: proposed, testing, supported, insufficient_evidence or rejected."},
                    "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
                    "fact_status": {"type": "string", "enum": ["verified", "inference", "suggestion", "unverified"]},
                    "tool": {"type": "string", "description": "The tool a step or evidence came from."},
                    "query": {"type": "string", "maxLength": 4000}
                },
                "additionalProperties": false
            }),
            open_output(),
            &[],
            &["Investigation"],
        )
        .chat_record(RiskLevel::L0, false)
        .mole_agent_only(),
        _ => unreachable!("workspace catalog received a kind of another catalog"),
    }
}

/// `scope` 与 `team` 加上各工具自己的参数。
fn scope_schema(properties: Value, required: &[&str]) -> Value {
    let mut all = json!({
        "scope": {"type": "string", "enum": ["org", "team"], "description": "Whose knowledge: the organization's, or a team's. Defaults to this chat's team, else the organization."},
        "team": {"type": "string", "description": "Team name or id when scope is team."}
    });

    if let (Some(all), Value::Object(extra)) = (all.as_object_mut(), properties) {
        all.extend(extra);
    }

    json!({
        "type": "object",
        "required": required,
        "properties": all,
        "additionalProperties": false
    })
}
