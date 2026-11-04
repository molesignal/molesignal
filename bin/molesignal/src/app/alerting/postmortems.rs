// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 内部复盘：一个事件一份，正文是 Markdown（页面按模板分段起好草稿，Mole Agent 也能起草）；改进项
//! 各有负责人与截止日期，可以一项一单。

use std::sync::Arc;

use super::tickets::TicketService;
use crate::{
    domain::alerting::{
        incident::Incident,
        postmortem::{
            ActionItem, ActionItemStatus, MAX_ACTION_ITEMS, Postmortem, PostmortemRepository,
            PostmortemStatus, PostmortemSummary,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 改一份复盘时传的；`None` 的不改。
#[derive(Debug, Default)]
pub struct PostmortemPatch {
    pub title: Option<String>,
    pub body: Option<String>,
    pub status: Option<PostmortemStatus>,
    pub owner_id: Option<Option<Id>>,
    pub reviewer_ids: Option<Vec<Id>>,
}

/// 改一项改进时传的；`None` 的不改。
#[derive(Debug, Default)]
pub struct ActionPatch {
    pub title: Option<String>,
    pub owner_id: Option<Option<Id>>,
    pub due_at: Option<Option<TimestampMicros>>,
    pub status: Option<ActionItemStatus>,
}

pub struct PostmortemService {
    repository: Arc<dyn PostmortemRepository>,
    tickets: Arc<TicketService>,
}

impl PostmortemService {
    pub fn new(repository: Arc<dyn PostmortemRepository>, tickets: Arc<TicketService>) -> Self {
        Self {
            repository,
            tickets,
        }
    }

    pub fn repository(&self) -> &Arc<dyn PostmortemRepository> {
        &self.repository
    }

    pub async fn list(&self, org_id: &Id) -> Result<Vec<PostmortemSummary>> {
        self.repository.list(org_id, TimestampMicros::now()).await
    }

    /// 为事件起一份复盘；已经有了就报冲突。
    pub async fn create(
        &self,
        incident: &Incident,
        title: &str,
        body: &str,
        by: &Id,
    ) -> Result<Postmortem> {
        if self
            .repository
            .for_incident(&incident.org_id, &incident.id)
            .await?
            .is_some()
        {
            return Err(Error::conflict("this incident already has a postmortem"));
        }

        let now = TimestampMicros::now();
        let mut postmortem = Postmortem {
            id: Id::new(),
            org_id: incident.org_id.clone(),
            incident_id: incident.id.clone(),
            title: if title.trim().is_empty() {
                incident.summary.clone()
            } else {
                title.to_string()
            },
            status: PostmortemStatus::Draft,
            body: body.to_string(),
            owner_id: Some(by.clone()),
            reviewer_ids: Vec::new(),
            created_by: Some(by.clone()),
            created_at: now,
            updated_at: now,
            published_at: None,
        };

        postmortem.validate()?;
        self.repository.save(postmortem).await
    }

    pub async fn update(&self, org_id: &Id, id: &Id, patch: PostmortemPatch) -> Result<Postmortem> {
        let mut postmortem = self
            .repository
            .get(org_id, id)
            .await?
            .ok_or_else(|| Error::not_found("postmortem not found"))?;

        if let Some(title) = patch.title {
            postmortem.title = title;
        }

        if let Some(body) = patch.body {
            postmortem.body = body;
        }

        if let Some(owner_id) = patch.owner_id {
            postmortem.owner_id = owner_id;
        }

        if let Some(reviewer_ids) = patch.reviewer_ids {
            postmortem.reviewer_ids = reviewer_ids;
        }

        if let Some(status) = patch.status {
            if status == PostmortemStatus::Published
                && postmortem.status != PostmortemStatus::Published
            {
                postmortem.published_at = Some(TimestampMicros::now());
            }

            postmortem.status = status;
        }

        postmortem.updated_at = TimestampMicros::now();
        postmortem.validate()?;
        self.repository.save(postmortem).await
    }

    pub async fn add_action(
        &self,
        postmortem: &Postmortem,
        title: &str,
        owner_id: Option<Id>,
        due_at: Option<TimestampMicros>,
    ) -> Result<ActionItem> {
        let existing = self
            .repository
            .list_actions(&postmortem.org_id, &postmortem.id)
            .await?;

        if existing.len() >= MAX_ACTION_ITEMS {
            return Err(Error::invalid(format!(
                "a postmortem has at most {MAX_ACTION_ITEMS} action items"
            )));
        }

        let now = TimestampMicros::now();
        let mut item = ActionItem {
            id: Id::new(),
            org_id: postmortem.org_id.clone(),
            postmortem_id: postmortem.id.clone(),
            incident_id: postmortem.incident_id.clone(),
            title: title.to_string(),
            owner_id,
            due_at,
            status: ActionItemStatus::Open,
            ticket_id: None,
            ticket_key: String::new(),
            ticket_url: String::new(),
            created_at: now,
            updated_at: now,
        };

        item.validate()?;
        self.repository.save_action(item).await
    }

    pub async fn update_action(
        &self,
        postmortem: &Postmortem,
        id: &Id,
        patch: ActionPatch,
    ) -> Result<ActionItem> {
        let mut item = self.action_of(postmortem, id).await?;

        if let Some(title) = patch.title {
            item.title = title;
        }

        if let Some(owner_id) = patch.owner_id {
            item.owner_id = owner_id;
        }

        if let Some(due_at) = patch.due_at {
            item.due_at = due_at;
        }

        if let Some(status) = patch.status {
            item.status = status;
        }

        item.updated_at = TimestampMicros::now();
        item.validate()?;
        self.repository.save_action(item).await
    }

    async fn action_of(&self, postmortem: &Postmortem, id: &Id) -> Result<ActionItem> {
        self.repository
            .get_action(&postmortem.org_id, id)
            .await?
            .filter(|item| item.postmortem_id == postmortem.id)
            .ok_or_else(|| Error::not_found("action item not found"))
    }

    /// 为一项改进建一张单；建过的原样返回。
    pub async fn ticket_for_action(
        &self,
        postmortem: &Postmortem,
        incident: &Incident,
        id: &Id,
        connector_id: &Id,
        by: &Id,
    ) -> Result<ActionItem> {
        let mut item = self.action_of(postmortem, id).await?;

        if item.ticket_id.is_some() {
            return Ok(item);
        }

        let connector = self
            .tickets
            .repository()
            .get_connector(&postmortem.org_id, connector_id)
            .await?
            .ok_or_else(|| Error::not_found("ticket connector not found"))?;

        let ticket = self
            .tickets
            .create_for_action(&connector, incident, &item.id, &item.title, Some(by))
            .await?;

        item.ticket_id = Some(ticket.id);
        item.ticket_key = ticket.external_key;
        item.ticket_url = ticket.url;
        item.updated_at = TimestampMicros::now();

        self.repository.save_action(item).await
    }
}
