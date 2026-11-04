// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件：宣布以后有三个固定角色（指挥官、沟通负责人、记录员）、作战室（能建群的消息连接器建的
//! 群）、会议链接、任务清单，进展同步给作战室与关注人。组织的设置里定建群用哪个连接器、会议怎么开、
//! 宣布时套用的检查清单、默认关注人，以及哪些新事件开出来就算重大事件。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use super::meeting::MeetingMode;
use super::{
    incident::{Incident, Severity},
    meeting::{GoogleMeetSettings, TencentMeetingSettings, ZoomSettings, validate_meeting},
    priority::Priority,
    routing::{LabelCondition, conditions_hold, validate_label_conditions},
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MAX_TASKS: usize = 50;
pub const MAX_TASK_TITLE_CHARS: usize = 200;
pub const MAX_CHECKLIST_ITEMS: usize = 30;
pub const MAX_STATUS_UPDATE_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseRole {
    Commander,
    Communications,
    Scribe,
}

impl ResponseRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Commander => "commander",
            Self::Communications => "communications",
            Self::Scribe => "scribe",
        }
    }
}

/// 作战室：建在哪个平台、群 ID 与入群链接。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarRoom {
    pub platform: String,
    pub connector_id: Id,
    pub chat_id: String,
    pub url: String,
}

/// 一个事件的重大事件处置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentResponse {
    pub incident_id: Id,
    pub org_id: Id,
    pub declared_at: TimestampMicros,
    /// 谁宣布的；自动规则宣布的为空。
    pub declared_by: Option<Id>,
    #[serde(default)]
    pub roles: BTreeMap<ResponseRole, Id>,
    pub war_room: Option<WarRoom>,
    pub meeting_url: Option<String>,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentTask {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    pub title: String,
    pub assignee_id: Option<Id>,
    pub done: bool,
    pub done_by: Option<Id>,
    pub done_at: Option<TimestampMicros>,
    pub position: i32,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
}

/// 哪些新事件开出来就算重大事件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MajorAutoRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<LabelCondition>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
    /// 优先级至少这么高（P1 最高）；设了时，没有优先级的事件不算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_priority: Option<Priority>,
}

impl MajorAutoRule {
    pub fn matches(&self, incident: &Incident) -> bool {
        self.enabled
            && self
                .min_severity
                .is_none_or(|floor| incident.severity >= floor)
            && self.min_priority.is_none_or(|bar| {
                incident
                    .handling
                    .priority
                    .is_some_and(|priority| priority.at_least(bar))
            })
            && conditions_hold(&self.conditions, &incident.labels)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseSettings {
    /// 建作战室用的消息连接器（能建群的：飞书 App、Slack App、钉钉、企业微信）；空着不建群。
    #[serde(default)]
    pub war_room_connector_id: Option<Id>,
    #[serde(default)]
    pub meeting: MeetingMode,
    /// 固定的会议链接。
    #[serde(default)]
    pub meeting_url: String,
    /// 预约飞书视频会议用的飞书 App 连接器；空着用建群的连接器。
    #[serde(default)]
    pub meeting_connector_id: Option<Id>,
    #[serde(default)]
    pub tencent_meeting: TencentMeetingSettings,
    #[serde(default)]
    pub zoom: ZoomSettings,
    #[serde(default)]
    pub google_meet: GoogleMeetSettings,
    /// 宣布时套用的任务。
    #[serde(default)]
    pub checklist: Vec<String>,
    /// 进展同步给这些人（另加事件的负责人和各角色）。
    #[serde(default)]
    pub stakeholder_ids: Vec<Id>,
    #[serde(default)]
    pub auto: MajorAutoRule,
    /// 会议的密钥（键见 [`super::meeting::MEETING_SECRET_KEYS`]），另外加密保存，不随设置读出。
    #[serde(skip)]
    pub secrets: BTreeMap<String, String>,
    /// 哪些密钥已经设了，给页面看。
    #[serde(default, skip_deserializing)]
    pub secrets_set: Vec<String>,
}

impl ResponseSettings {
    pub fn validate(&mut self) -> Result<()> {
        self.checklist = std::mem::take(&mut self.checklist)
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();

        if self.checklist.len() > MAX_CHECKLIST_ITEMS
            || self
                .checklist
                .iter()
                .any(|item| item.chars().count() > MAX_TASK_TITLE_CHARS)
        {
            return Err(Error::invalid(format!(
                "a checklist has at most {MAX_CHECKLIST_ITEMS} items of {MAX_TASK_TITLE_CHARS} characters"
            )));
        }

        self.meeting_url = self.meeting_url.trim().to_string();

        if self.meeting == MeetingMode::FixedUrl
            && !self
                .meeting_url
                .to_ascii_lowercase()
                .starts_with("https://")
        {
            return Err(Error::invalid("a fixed meeting link starts with https://"));
        }

        if self.meeting == MeetingMode::FeishuVc
            && self.meeting_connector_id.is_none()
            && self.war_room_connector_id.is_none()
        {
            return Err(Error::invalid(
                "a Feishu meeting is booked through a Feishu App connector",
            ));
        }

        validate_meeting(
            self.meeting,
            &mut self.tencent_meeting,
            &mut self.zoom,
            &mut self.google_meet,
            &self.secrets,
        )?;

        // 只留所选会议的密钥：换了会议方式，旧的密钥不再存着。
        let keep = self.meeting.secret_key();
        self.secrets.retain(|key, _| Some(key.as_str()) == keep);

        validate_label_conditions(&self.auto.conditions)
    }
}

/// 一次进展：影响、现状、下一步。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub impact: String,
    pub current: String,
    pub next: String,
}

impl StatusUpdate {
    pub fn validate(&mut self) -> Result<()> {
        for field in [&mut self.impact, &mut self.current, &mut self.next] {
            *field = field.trim().to_string();
        }

        if self.impact.is_empty() && self.current.is_empty() && self.next.is_empty() {
            return Err(Error::invalid("a status update says something"));
        }

        if [&self.impact, &self.current, &self.next]
            .iter()
            .any(|field| field.chars().count() > MAX_STATUS_UPDATE_CHARS)
        {
            return Err(Error::invalid(format!(
                "each part of a status update is at most {MAX_STATUS_UPDATE_CHARS} characters"
            )));
        }

        Ok(())
    }
}

#[async_trait]
pub trait IncidentResponseRepository: Send + Sync {
    async fn get(&self, org_id: &Id, incident_id: &Id) -> Result<Option<IncidentResponse>>;
    async fn save(&self, response: IncidentResponse) -> Result<IncidentResponse>;
    async fn list_tasks(&self, org_id: &Id, incident_id: &Id) -> Result<Vec<IncidentTask>>;
    async fn save_task(&self, task: IncidentTask) -> Result<IncidentTask>;
    async fn delete_task(&self, org_id: &Id, incident_id: &Id, id: &Id) -> Result<bool>;
    async fn settings(&self, org_id: &Id) -> Result<ResponseSettings>;
    async fn save_settings(
        &self,
        org_id: &Id,
        settings: ResponseSettings,
        by: &Id,
    ) -> Result<ResponseSettings>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_need_a_link_or_a_feishu_app_for_meetings() {
        let mut fixed = ResponseSettings {
            meeting: MeetingMode::FixedUrl,
            meeting_url: "http://meet".into(),
            ..ResponseSettings::default()
        };
        assert!(fixed.validate().is_err());

        fixed.meeting_url = " https://meet.example.com/war-room ".into();
        assert!(fixed.validate().is_ok());
        assert_eq!(fixed.meeting_url, "https://meet.example.com/war-room");

        let mut vc = ResponseSettings {
            meeting: MeetingMode::FeishuVc,
            ..ResponseSettings::default()
        };
        assert!(vc.validate().is_err());
    }

    #[test]
    fn a_status_update_says_something_within_bounds() {
        let mut empty = StatusUpdate {
            impact: " ".into(),
            current: String::new(),
            next: String::new(),
        };
        assert!(empty.validate().is_err());

        let mut fine = StatusUpdate {
            impact: "Checkout down for EU".into(),
            current: " Rolling back ".into(),
            next: String::new(),
        };
        assert!(fine.validate().is_ok());
        assert_eq!(fine.current, "Rolling back");
    }
}
