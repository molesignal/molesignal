// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件的处置：宣布（手动，或新事件命中组织的自动规则）、三个角色、作战室与会议、任务清单、
//! 进展同步。作战室、会议与建单都是尽力而为：做不成只记日志，宣布本身照样成立。

mod meetings;
mod room;
mod tasks;
pub mod war_room;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{AlertIncidentLifecycleSink, tickets::TicketService};
use crate::{
    app::notify::{NotifyEngine, alert_dispatch},
    domain::{
        alerting::{
            incident::{Incident, IncidentStatus},
            meeting::MEETING_SECRET_KEYS,
            repositories::IncidentRepository,
            response::{
                IncidentResponse, IncidentResponseRepository, IncidentTask, MeetingMode,
                ResponseRole, ResponseSettings, StatusUpdate,
            },
            timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
        },
        notify::repositories::{NotifyConnectorRepository, UserNotifyEndpointRepository},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 进展的通知事件。
pub const INCIDENT_STATUS_UPDATE_EVENT: &str = "incident.status_update";

pub struct IncidentResponseService {
    repository: Arc<dyn IncidentResponseRepository>,
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    connectors: Arc<dyn NotifyConnectorRepository>,
    endpoints: Arc<dyn UserNotifyEndpointRepository>,
    notify: Arc<NotifyEngine>,
    tickets: Arc<TicketService>,
    external_url: String,
}

impl IncidentResponseService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<dyn IncidentResponseRepository>,
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        connectors: Arc<dyn NotifyConnectorRepository>,
        endpoints: Arc<dyn UserNotifyEndpointRepository>,
        notify: Arc<NotifyEngine>,
        tickets: Arc<TicketService>,
        external_url: String,
    ) -> Self {
        Self {
            repository,
            incidents,
            timeline,
            connectors,
            endpoints,
            notify,
            tickets,
            external_url: external_url.trim().trim_end_matches('/').to_string(),
        }
    }

    pub fn repository(&self) -> &Arc<dyn IncidentResponseRepository> {
        &self.repository
    }

    pub async fn get(
        &self,
        incident: &Incident,
    ) -> Result<(Option<IncidentResponse>, Vec<IncidentTask>)> {
        let response = self.repository.get(&incident.org_id, &incident.id).await?;
        let tasks = self
            .repository
            .list_tasks(&incident.org_id, &incident.id)
            .await?;

        Ok((response, tasks))
    }

    /// 宣布为重大事件；已经宣布过的原样返回。`by` 为空是自动规则宣布的。
    pub async fn declare(
        &self,
        incident: &Incident,
        by: Option<&Id>,
        commander: Option<Id>,
    ) -> Result<IncidentResponse> {
        if let Some(existing) = self.repository.get(&incident.org_id, &incident.id).await? {
            return Ok(existing);
        }

        if matches!(
            incident.status,
            IncidentStatus::Resolved | IncidentStatus::Closed
        ) {
            return Err(Error::invalid("a resolved incident is not declared major"));
        }

        let settings = self.repository.settings(&incident.org_id).await?;
        let now = TimestampMicros::now();
        let mut roles = BTreeMap::new();

        if let Some(commander) = commander.or_else(|| by.cloned()) {
            roles.insert(ResponseRole::Commander, commander);
        }

        let mut response = self
            .repository
            .save(IncidentResponse {
                incident_id: incident.id.clone(),
                org_id: incident.org_id.clone(),
                declared_at: now,
                declared_by: by.cloned(),
                roles,
                war_room: None,
                meeting_url: None,
                updated_at: now,
            })
            .await?;

        self.record(
            incident,
            TimelineKind::DeclaredMajor,
            by.cloned(),
            json!({ "auto": by.is_none() }),
        )
        .await;

        for (position, title) in settings.checklist.iter().enumerate() {
            self.repository
                .save_task(IncidentTask {
                    id: Id::new(),
                    org_id: incident.org_id.clone(),
                    incident_id: incident.id.clone(),
                    title: title.clone(),
                    assignee_id: None,
                    done: false,
                    done_by: None,
                    done_at: None,
                    position: i32::try_from(position).unwrap_or(i32::MAX),
                    created_by: by.cloned(),
                    created_at: now,
                })
                .await?;
        }

        self.open_room(incident, &settings, &mut response).await;
        response = self.repository.save(response).await?;

        let connectors = self
            .tickets
            .repository()
            .list_connectors(&incident.org_id)
            .await?;

        for connector in connectors
            .into_iter()
            .filter(|connector| connector.enabled && connector.on_major_incident)
        {
            if let Err(error) = self
                .tickets
                .create_with(&incident.org_id, &connector.id, incident, by)
                .await
            {
                tracing::warn!(incident_id = %incident.id, connector_id = %connector.id, %error, "major incident ticket failed");
            }
        }

        Ok(response)
    }

    pub async fn assign_role(
        &self,
        incident: &Incident,
        role: ResponseRole,
        user_id: Option<Id>,
        by: &Id,
    ) -> Result<IncidentResponse> {
        let mut response = self.declared(incident).await?;

        match &user_id {
            Some(user_id) => response.roles.insert(role, user_id.clone()),
            None => response.roles.remove(&role),
        };

        response.updated_at = TimestampMicros::now();
        let response = self.repository.save(response).await?;

        self.record(
            incident,
            TimelineKind::RoleAssigned,
            Some(by.clone()),
            json!({ "role": role.as_str(), "userId": user_id }),
        )
        .await;

        Ok(response)
    }

    async fn declared(&self, incident: &Incident) -> Result<IncidentResponse> {
        self.repository
            .get(&incident.org_id, &incident.id)
            .await?
            .ok_or_else(|| Error::invalid("declare the incident major first"))
    }

    /// 发进展：记进时间线，发到作战室，经通知引擎告诉关注人、各角色和负责人。
    pub async fn status_update(
        &self,
        incident: &Incident,
        mut update: StatusUpdate,
        by: &Id,
    ) -> Result<()> {
        update.validate()?;

        let response = self.declared(incident).await?;
        let settings = self.repository.settings(&incident.org_id).await?;

        self.record(
            incident,
            TimelineKind::StatusUpdate,
            Some(by.clone()),
            json!({ "impact": update.impact, "current": update.current, "next": update.next }),
        )
        .await;

        let text = [
            format!("[{}] {}", incident.severity.as_str(), incident.summary),
            update.impact.clone(),
            update.current.clone(),
            update.next.clone(),
            format!("{}/alerts/incidents/{}", self.external_url, incident.id),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

        if let Some(room) = &response.war_room {
            match self
                .connectors
                .get(&incident.org_id, &room.connector_id)
                .await
            {
                Ok(connector) => {
                    if let Err(error) = war_room::post(&connector, &room.chat_id, &text).await {
                        tracing::warn!(incident_id = %incident.id, %error, "status update could not reach the war room");
                    }
                }
                Err(error) => {
                    tracing::warn!(incident_id = %incident.id, %error, "war room connector could not be read")
                }
            }
        }

        let people = self.people(incident, &settings, &response);
        let mut dispatch = alert_dispatch(incident, INCIDENT_STATUS_UPDATE_EVENT);

        dispatch.event.id = format!(
            "{INCIDENT_STATUS_UPDATE_EVENT}:{}:{}",
            incident.id,
            Id::new()
        );
        dispatch.event.occurred_at = TimestampMicros::now();

        if let Value::Object(attributes) = &mut dispatch.event.attributes {
            attributes.insert(
                "user_ids".into(),
                Value::Array(
                    people
                        .iter()
                        .map(|id| Value::String(id.to_string()))
                        .collect(),
                ),
            );
            attributes.insert(
                "status_update".into(),
                json!({ "impact": update.impact, "current": update.current, "next": update.next, "author": by }),
            );
        }

        dispatch.message.title = format!("Update: {}", incident.summary);
        dispatch.message.text = text;

        if let Err(error) = self.notify.enqueue_event(dispatch).await {
            tracing::warn!(incident_id = %incident.id, %error, "status update notification could not be queued");
        }

        Ok(())
    }

    /// 新事件命中组织的自动规则就宣布为重大事件。出错只记日志。
    pub async fn on_incident(&self, incident: Incident) {
        if incident.revision != 0 || incident.status != IncidentStatus::Open {
            return;
        }

        let settings = match self.repository.settings(&incident.org_id).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "response settings could not be read");
                return;
            }
        };

        if settings.auto.matches(&incident)
            && let Err(error) = self.declare(&incident, None, None).await
        {
            tracing::warn!(incident_id = %incident.id, %error, "automatic major incident failed");
        }
    }

    /// 保存组织的设置：建群的连接器在本组织、是飞书 App 或 Slack App，飞书会议要飞书 App。
    /// 保存组织的设置。`secrets` 是新填的会议密钥（只写不读）：留空的保持原样，只留所选会议的。建群的连接器
    /// 要在本组织、能建群；飞书视频会议要用飞书 App。
    pub async fn save_settings(
        &self,
        org_id: &Id,
        mut settings: ResponseSettings,
        secrets: BTreeMap<String, String>,
        by: &Id,
    ) -> Result<ResponseSettings> {
        settings.secrets = self.repository.settings(org_id).await?.secrets;

        for (key, value) in secrets {
            if !MEETING_SECRET_KEYS.contains(&key.as_str()) {
                return Err(Error::invalid(format!("`{key}` is not a meeting secret")));
            }

            let value = value.trim().to_string();

            if !value.is_empty() {
                settings.secrets.insert(key, value);
            }
        }

        settings.validate()?;

        if let Some(connector_id) = &settings.war_room_connector_id {
            let connector = self.connectors.get(org_id, connector_id).await?;

            if !war_room::WAR_ROOM_TYPES.contains(&connector.connector_type.as_str()) {
                return Err(Error::invalid(
                    "war rooms are made with a connector that can make group chats",
                ));
            }
        }

        if settings.meeting == MeetingMode::FeishuVc {
            let connector_id = settings
                .meeting_connector_id
                .as_ref()
                .or(settings.war_room_connector_id.as_ref())
                .ok_or_else(|| Error::invalid("a Feishu meeting needs a Feishu App connector"))?;
            let connector = self.connectors.get(org_id, connector_id).await?;

            if connector.connector_type != war_room::LARK_APP {
                return Err(Error::invalid(
                    "a Feishu meeting is booked through a Feishu App connector",
                ));
            }
        }

        self.repository.save_settings(org_id, settings, by).await
    }

    async fn record(
        &self,
        incident: &Incident,
        kind: TimelineKind,
        actor: Option<Id>,
        detail: Value,
    ) {
        let entry = TimelineEntry::new(
            &incident.org_id,
            &incident.id,
            TimestampMicros::now(),
            kind,
            actor,
            detail,
        );

        if let Err(error) = self.timeline.append(entry).await {
            tracing::warn!(incident_id = %incident.id, %error, "response timeline entry could not be written");
        }
    }

    pub async fn incident(&self, org_id: &Id, id: &Id) -> Result<Incident> {
        let incident = self.incidents.get(id).await?;

        if &incident.org_id != org_id {
            return Err(Error::not_found("incident not found"));
        }

        Ok(incident)
    }
}

/// 新事件交给重大事件的自动规则：另起任务，不拖慢事件的写入。
pub struct ResponseLifecycleSink(pub Arc<IncidentResponseService>);

#[async_trait]
impl AlertIncidentLifecycleSink for ResponseLifecycleSink {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        if incident.revision != 0 {
            return Ok(());
        }

        let service = self.0.clone();
        let incident = incident.clone();

        tokio::spawn(async move { service.on_incident(incident).await });
        Ok(())
    }
}
