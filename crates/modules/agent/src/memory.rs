// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Personal and team memories.
//!
//! Four categories: preference (preferences), procedure (practices), fact (facts), and lesson
//! (lessons). Each entry has a name, a one-sentence description of when it applies, and a body.
//! The session determines where memories are written: personal sessions write to the initiator's
//! memories, team sessions to the team's. The memory index (names and descriptions) for the
//! current session's scope goes into the system prompt; bodies are read on demand with
//! `read_memory`. Within its stated scope, a preference takes priority over a Skill's default
//! practices; the other three categories are checked against the current situation before use.
//!
//! Sources: the model calls `remember` / `forget` when the requester asks it to remember or forget
//! something; when the organization enables "Distill memories after chats", the model distills
//! memories from the conversation after the session has been idle for a while. Forgetting only
//! marks a memory and can be undone.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use crate::scope::Scope;

pub const MAX_MEMORIES: usize = 200;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DESCRIPTION_CHARS: usize = 300;
pub const MAX_BODY_CHARS: usize = 4_000;
/// The maximum number of entries in the system prompt's index, most recently modified first.
const INDEX_ENTRIES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Preference,
    Procedure,
    Fact,
    Lesson,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Procedure => "procedure",
            Self::Fact => "fact",
            Self::Lesson => "lesson",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Preference, Self::Procedure, Self::Fact, Self::Lesson]
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: Id,
    pub org_id: Id,
    /// Team or personal.
    pub scope: Scope,
    pub kind: MemoryKind,
    pub name: String,
    /// A one-sentence description of when it applies.
    pub description: String,
    pub body: String,
    /// The session from which the memory was recorded.
    pub source_chat_id: Option<Id>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    /// The time it was forgotten; cleared when undone.
    pub forgotten_at: Option<TimestampMicros>,
}

/// Checks whether a memory is acceptable: its scope is team or personal, and its name, description,
/// and body are nonempty and within length limits.
pub fn check_memory(scope: &Scope, name: &str, description: &str, body: &str) -> Result<()> {
    if *scope == Scope::Org {
        return Err(Error::invalid("a memory belongs to a person or a team"));
    }

    for (field, value, limit) in [
        ("name", name, MAX_NAME_CHARS),
        ("description", description, MAX_DESCRIPTION_CHARS),
        ("body", body, MAX_BODY_CHARS),
    ] {
        if value.trim().is_empty() {
            return Err(Error::invalid(format!("a memory needs a {field}")));
        }

        if value.chars().count() > limit {
            return Err(Error::invalid(format!(
                "the {field} of a memory is at most {limit} characters"
            )));
        }
    }

    Ok(())
}

/// The memory index for the system prompt. `owner` indicates whose memories these are, such as
/// `yours` or a team name.
pub fn index_prompt(memories: &[Memory], owner: &str) -> Option<String> {
    let mut active = memories
        .iter()
        .filter(|memory| memory.forgotten_at.is_none())
        .collect::<Vec<_>>();

    if active.is_empty() {
        return None;
    }

    active.sort_by_key(|memory| std::cmp::Reverse(memory.updated_at.0));
    active.truncate(INDEX_ENTRIES);

    let mut text = format!(
        "Memories of {owner}, kept from earlier chats. Read a body with read_memory when one applies. A preference wins over the default way of a skill within what it says; check a procedure, fact or lesson against the current state before relying on it. Save new ones with remember and drop wrong ones with forget.\n"
    );

    for memory in active {
        text.push_str(&format!(
            "\n- [{}] {}: {}",
            memory.kind.as_str(),
            memory.name,
            memory.description
        ));
    }

    Some(text)
}

/// The maximum number of memories recorded per extraction.
pub const DISTILL_MAX: usize = 5;

/// Instructions for the model to extract memories from a conversation.
pub const DISTILL_INSTRUCTION: &str = "Read the conversation above and pick out what is worth remembering for later chats: a preference the person stated, a procedure that worked, a lasting fact about their systems, or a lesson from a mistake. Leave out anything specific to this one incident, anything already in the memories listed, and anything you are not sure of. Answer with a JSON array only, at most 5 items, each {\"kind\": \"preference|procedure|fact|lesson\", \"name\": short title, \"description\": one sentence on when it applies, \"body\": the memory itself}. Answer [] when nothing is worth remembering.";

/// Reads memories extracted by the model: a JSON array, possibly wrapped in a code block; skips
/// unreadable or invalid entries.
pub fn parse_drafts(text: &str) -> Vec<MemoryDraft> {
    let text = text.trim();

    let start = text.find('[');
    let end = text.rfind(']');

    let (Some(start), Some(end)) = (start, end) else {
        return Vec::new();
    };

    if end < start {
        return Vec::new();
    }

    let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&text[start..=end]) else {
        return Vec::new();
    };

    items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<MemoryDraft>(item).ok())
        .filter(|draft| {
            let probe = Scope::Personal {
                owner_id: Id::from_string("probe"),
            };

            check_memory(&probe, &draft.name, &draft.description, &draft.body).is_ok()
        })
        .take(DISTILL_MAX)
        .collect()
}

/// The organization's memory settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettings {
    /// The model extracts memories from the conversation after the session has been idle for a
    /// while (counts toward the budget).
    pub distill_after_chat: bool,
    pub updated_by: Option<Id>,
    pub updated_at: Option<TimestampMicros>,
}

/// A memory to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDraft {
    pub kind: MemoryKind,
    pub name: String,
    pub description: String,
    pub body: String,
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Memories in a scope that have not been forgotten, most recently modified first.
    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<Memory>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Memory>;
    /// Finds a memory by name (case-insensitive) that has not been forgotten.
    async fn find(&self, org_id: &Id, scope: &Scope, name: &str) -> Result<Option<Memory>>;
    /// Writes by name: replaces any memory with the same name, including forgotten memories.
    /// Returns an error if the entry limit is exceeded.
    async fn remember(
        &self,
        org_id: &Id,
        scope: &Scope,
        draft: &MemoryDraft,
        source_chat_id: Option<&Id>,
        actor: &Id,
    ) -> Result<Memory>;
    async fn update(&self, org_id: &Id, id: &Id, draft: &MemoryDraft, actor: &Id)
    -> Result<Memory>;
    /// Forgets a memory by marking it; restores it when undone.
    async fn forget(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory>;
    /// Undoes forgetting; reports a conflict if another memory with the same name has been
    /// recorded.
    async fn restore(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory>;
    /// Permanently deletes a memory: undoes a "Remember" action or deletes it from the memory page.
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool>;
    async fn settings(&self, org_id: &Id) -> Result<MemorySettings>;
    async fn set_settings(
        &self,
        org_id: &Id,
        distill_after_chat: bool,
        actor: &Id,
    ) -> Result<MemorySettings>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(name: &str, kind: MemoryKind, at: i64) -> Memory {
        Memory {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            scope: Scope::Personal {
                owner_id: Id::from_string("u1"),
            },
            kind,
            name: name.into(),
            description: format!("when {name}"),
            body: "body".into(),
            source_chat_id: None,
            created_by: Id::from_string("u1"),
            updated_by: Id::from_string("u1"),
            created_at: TimestampMicros(at),
            updated_at: TimestampMicros(at),
            forgotten_at: None,
        }
    }

    #[test]
    fn a_memory_belongs_to_a_person_or_a_team_and_fits_its_limits() {
        let personal = Scope::Personal {
            owner_id: Id::from_string("u1"),
        };

        assert!(check_memory(&personal, "Time zone", "When writing times", "Use UTC+8").is_ok());
        assert!(check_memory(&Scope::Org, "x", "y", "z").is_err());
        assert!(check_memory(&personal, " ", "y", "z").is_err());
        assert!(check_memory(&personal, "x", "y", &"z".repeat(MAX_BODY_CHARS + 1)).is_err());
        assert_eq!(MemoryKind::parse("lesson"), Some(MemoryKind::Lesson));
        assert_eq!(MemoryKind::parse("rule"), None);
    }

    #[test]
    fn distilled_memories_are_read_from_a_json_array() {
        let text = "```json\n[{\"kind\": \"preference\", \"name\": \"Time zone\", \"description\": \"When writing times\", \"body\": \"Use UTC+8\"}, {\"kind\": \"rule\", \"name\": \"x\", \"description\": \"y\", \"body\": \"z\"}]\n```";

        let drafts = parse_drafts(text);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, MemoryKind::Preference);
        assert!(parse_drafts("nothing to remember").is_empty());
        assert!(parse_drafts("[]").is_empty());
    }

    #[test]
    fn the_index_lists_what_is_remembered_newest_first() {
        assert!(index_prompt(&[], "you").is_none());

        let mut forgotten = memory("old", MemoryKind::Fact, 1);
        forgotten.forgotten_at = Some(TimestampMicros(5));

        let text = index_prompt(
            &[
                memory("Time zone", MemoryKind::Preference, 2),
                memory("Rollback", MemoryKind::Procedure, 3),
                forgotten,
            ],
            "you",
        )
        .unwrap();

        assert!(text.contains("- [procedure] Rollback: when Rollback\n- [preference] Time zone"));
        assert!(!text.contains("old"));
    }
}
