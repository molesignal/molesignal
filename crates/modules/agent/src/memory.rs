// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 记忆：个人的与团队的。
//!
//! 四类：preference（偏好）、procedure（做法）、fact（事实）、lesson（教训）；每条有名称、一句话
//! 说明何时适用、正文。写到哪里跟着会话：个人会话写发起人的记忆，团队会话写本队的。当前会话
//! 作用域的记忆目录（名称与说明）进系统提示，正文用 `read_memory` 按需读。preference 在它说明的
//! 范围内优先于 Skill 的默认做法；其余三类用之前先按当前情况核实。
//!
//! 来源：提问人说「记住 / 忘掉」时模型调 `remember` / `forget`；组织打开「会话结束后提炼」时，
//! 会话停下一段时间后由模型从对话里提炼。忘掉只是标记，可以撤销。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    agent::scope::Scope,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub const MAX_MEMORIES: usize = 200;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DESCRIPTION_CHARS: usize = 300;
pub const MAX_BODY_CHARS: usize = 4_000;
/// 进系统提示的目录最多这么多条，最近改过的在前。
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
pub struct Memory {
    pub id: Id,
    pub org_id: Id,
    /// 团队或个人。
    pub scope: Scope,
    pub kind: MemoryKind,
    pub name: String,
    /// 一句话说明何时适用。
    pub description: String,
    pub body: String,
    /// 从哪个会话记下的。
    pub source_chat_id: Option<Id>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    /// 忘掉的时间；撤销时清空。
    pub forgotten_at: Option<TimestampMicros>,
}

/// 一条记忆能不能收：作用域是团队或个人，名称、说明、正文都在长度里且不为空。
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

/// 进系统提示的记忆目录。`owner` 说明是谁的记忆，例如「你的」或团队名。
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

/// 一次提炼最多记下这么多条。
pub const DISTILL_MAX: usize = 5;

/// 让模型从一段对话里提炼记忆的指令。
pub const DISTILL_INSTRUCTION: &str = "Read the conversation above and pick out what is worth remembering for later chats: a preference the person stated, a procedure that worked, a lasting fact about their systems, or a lesson from a mistake. Leave out anything specific to this one incident, anything already in the memories listed, and anything you are not sure of. Answer with a JSON array only, at most 5 items, each {\"kind\": \"preference|procedure|fact|lesson\", \"name\": short title, \"description\": one sentence on when it applies, \"body\": the memory itself}. Answer [] when nothing is worth remembering.";

/// 读出模型提炼的记忆：一个 JSON 数组，可能包在代码块里；读不出或不合规的条目跳过。
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

/// 组织对记忆的设置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySettings {
    /// 会话停下一段时间后，由模型从对话里提炼记忆（计入预算）。
    pub distill_after_chat: bool,
    pub updated_by: Option<Id>,
    pub updated_at: Option<TimestampMicros>,
}

/// 一条要写的记忆。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDraft {
    pub kind: MemoryKind,
    pub name: String,
    pub description: String,
    pub body: String,
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// 一个作用域里没忘掉的记忆，最近改过的在前。
    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<Memory>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Memory>;
    /// 按名称找（不分大小写），没忘掉的。
    async fn find(&self, org_id: &Id, scope: &Scope, name: &str) -> Result<Option<Memory>>;
    /// 按名称写入：同名的替换，忘掉的同名记忆被它取代。超出条数时报错。
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
    /// 忘掉：标记，撤销时还原。
    async fn forget(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory>;
    /// 撤销忘掉；同名的已经又记过一条时报冲突。
    async fn restore(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory>;
    /// 彻底删除：撤销一次「记住」，或在记忆页删除。
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
