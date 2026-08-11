// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::{
        iam::{Team, TeamRepository},
        notify::{
            policy::NotifyEvent,
            recipient::{NotifyRecipient, RecipientResolver},
        },
    },
    shared::{Error, Result, ids::Id},
};

pub const TEAM_MEMBERS_RESOLVER: &str = "team_members";
pub const TEAM_LEAD_RESOLVER: &str = "team_lead";

#[derive(Debug, Deserialize)]
struct TeamConfig {
    #[serde(default)]
    team_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct TeamLeadConfig {
    #[serde(default)]
    team_id: Option<Id>,
    #[serde(default)]
    user_ids: Vec<Id>,
}

fn event_team_id(event: &NotifyEvent) -> Option<Id> {
    event
        .attributes
        .get("team_id")
        .or_else(|| event.attributes.get("teamId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(Id::from_string)
}

async fn get_team(
    teams: &dyn TeamRepository,
    event: &NotifyEvent,
    configured: Option<Id>,
) -> Result<Team> {
    let team_id = configured
        .or_else(|| event_team_id(event))
        .ok_or_else(|| Error::invalid("team resolver requires team_id in config or event"))?;
    teams
        .list(&event.organization_id)
        .await?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or_else(|| Error::not_found("team"))
}

pub struct TeamMembersResolver {
    teams: Arc<dyn TeamRepository>,
}

impl TeamMembersResolver {
    pub fn new(teams: Arc<dyn TeamRepository>) -> Self {
        Self { teams }
    }
}

#[async_trait]
impl RecipientResolver for TeamMembersResolver {
    fn resolver_type(&self) -> &'static str {
        TEAM_MEMBERS_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        serde_json::from_value::<TeamConfig>(config.clone())
            .map(|_| ())
            .map_err(|error| Error::invalid(format!("invalid team_members config: {error}")))
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config: TeamConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid team_members config: {error}")))?;
        let team = get_team(self.teams.as_ref(), event, config.team_id).await?;
        Ok(team
            .member_ids
            .into_iter()
            .map(|user_id| NotifyRecipient {
                user_id,
                team_id: Some(team.id.clone()),
            })
            .collect())
    }
}

pub struct TeamLeadResolver {
    teams: Arc<dyn TeamRepository>,
}

impl TeamLeadResolver {
    pub fn new(teams: Arc<dyn TeamRepository>) -> Self {
        Self { teams }
    }
}

#[async_trait]
impl RecipientResolver for TeamLeadResolver {
    fn resolver_type(&self) -> &'static str {
        TEAM_LEAD_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        let config: TeamLeadConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid team_lead config: {error}")))?;
        if config.user_ids.len() > 20 {
            return Err(Error::invalid(
                "team_lead config supports at most 20 leader users",
            ));
        }
        if config.user_ids.iter().collect::<HashSet<_>>().len() != config.user_ids.len() {
            return Err(Error::invalid("team_lead user_ids must be unique"));
        }
        Ok(())
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config: TeamLeadConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid team_lead config: {error}")))?;
        let team = get_team(self.teams.as_ref(), event, config.team_id).await?;
        let member_ids = team.member_ids.iter().collect::<HashSet<_>>();
        if config
            .user_ids
            .iter()
            .any(|user_id| !member_ids.contains(user_id))
        {
            return Err(Error::invalid(
                "team_lead contains a user outside the selected team",
            ));
        }
        // Team 当前没有独立 owner 列；未显式配置时沿用成员顺序中的第一位，
        // 让现有数据无需迁移即可启用团队负责人升级。
        let leaders = if config.user_ids.is_empty() {
            team.member_ids.first().cloned().into_iter().collect()
        } else {
            config.user_ids
        };
        Ok(leaders
            .into_iter()
            .map(|user_id| NotifyRecipient {
                user_id,
                team_id: Some(team.id.clone()),
            })
            .collect())
    }
}
