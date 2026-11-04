// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 内部复盘：一个事件一份，状态草稿、评审中、已发布；正文是 Markdown，按模板分段（摘要、影响、时间线、
//! 根因、做得好与不好、改进项）。改进项各有负责人、截止日期，可以一项一单。与状态页的公开复盘各自独立，
//! 摘要可以复制过去。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MAX_POSTMORTEM_BODY_CHARS: usize = 100_000;
pub const MAX_ACTION_ITEMS: usize = 50;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostmortemStatus {
    #[default]
    Draft,
    InReview,
    Published,
}

impl PostmortemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InReview => "in_review",
            Self::Published => "published",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "in_review" => Self::InReview,
            "published" => Self::Published,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Postmortem {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    pub title: String,
    pub status: PostmortemStatus,
    pub body: String,
    pub owner_id: Option<Id>,
    #[serde(default)]
    pub reviewer_ids: Vec<Id>,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub published_at: Option<TimestampMicros>,
}

impl Postmortem {
    pub fn validate(&mut self) -> Result<()> {
        self.title = self.title.trim().to_string();

        if self.title.is_empty() || self.title.chars().count() > 200 {
            return Err(Error::invalid(
                "a postmortem's title has 1 to 200 characters",
            ));
        }

        if self.body.chars().count() > MAX_POSTMORTEM_BODY_CHARS {
            return Err(Error::invalid(format!(
                "a postmortem is at most {MAX_POSTMORTEM_BODY_CHARS} characters"
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionItemStatus {
    #[default]
    Open,
    Done,
}

/// 复盘里的一项改进。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: Id,
    pub org_id: Id,
    pub postmortem_id: Id,
    pub incident_id: Id,
    pub title: String,
    pub owner_id: Option<Id>,
    pub due_at: Option<TimestampMicros>,
    pub status: ActionItemStatus,
    /// 为它建的工单（`incident_tickets.id`）与它的 key、地址。
    pub ticket_id: Option<Id>,
    #[serde(default)]
    pub ticket_key: String,
    #[serde(default)]
    pub ticket_url: String,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl ActionItem {
    pub fn validate(&mut self) -> Result<()> {
        self.title = self.title.trim().to_string();

        if self.title.is_empty() || self.title.chars().count() > 300 {
            return Err(Error::invalid("an action item has 1 to 300 characters"));
        }

        Ok(())
    }

    /// 过了截止日期还没做完。
    pub fn overdue(&self, now: TimestampMicros) -> bool {
        self.status == ActionItemStatus::Open && self.due_at.is_some_and(|due| due.0 < now.0)
    }
}

/// 复盘列表里的一行：复盘本身和它改进项的数目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostmortemSummary {
    #[serde(flatten)]
    pub postmortem: Postmortem,
    pub incident_summary: String,
    pub open_actions: i64,
    pub overdue_actions: i64,
}

#[async_trait]
pub trait PostmortemRepository: Send + Sync {
    async fn list(&self, org_id: &Id, now: TimestampMicros) -> Result<Vec<PostmortemSummary>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<Postmortem>>;
    async fn for_incident(&self, org_id: &Id, incident_id: &Id) -> Result<Option<Postmortem>>;
    async fn save(&self, postmortem: Postmortem) -> Result<Postmortem>;
    async fn list_actions(&self, org_id: &Id, postmortem_id: &Id) -> Result<Vec<ActionItem>>;
    async fn get_action(&self, org_id: &Id, id: &Id) -> Result<Option<ActionItem>>;
    async fn save_action(&self, item: ActionItem) -> Result<ActionItem>;
    async fn delete_action(&self, org_id: &Id, postmortem_id: &Id, id: &Id) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_item_past_its_due_date_is_overdue() {
        let item = ActionItem {
            id: Id::from_string("a1"),
            org_id: Id::from_string("org"),
            postmortem_id: Id::from_string("pm"),
            incident_id: Id::from_string("inc"),
            title: "Add a pool alert".into(),
            owner_id: None,
            due_at: Some(TimestampMicros(100)),
            status: ActionItemStatus::Open,
            ticket_id: None,
            ticket_key: String::new(),
            ticket_url: String::new(),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        assert!(item.overdue(TimestampMicros(200)));
        assert!(!item.overdue(TimestampMicros(50)));
        assert!(
            !ActionItem {
                status: ActionItemStatus::Done,
                ..item
            }
            .overdue(TimestampMicros(200))
        );
    }

    #[test]
    fn statuses_read_back() {
        for status in [
            PostmortemStatus::Draft,
            PostmortemStatus::InReview,
            PostmortemStatus::Published,
        ] {
            assert_eq!(PostmortemStatus::parse(status.as_str()), status);
        }
    }
}
