// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 宣布重大事件时开会、建作战室：都是尽力而为，做不成只记日志，宣布照样成立。

use std::collections::HashMap;

use serde_json::json;

use super::{IncidentResponseService, meetings, war_room};
use crate::{
    domain::{
        alerting::{
            incident::Incident,
            meeting::MeetingMode,
            response::{IncidentResponse, ResponseRole, ResponseSettings},
            timeline::TimelineKind,
        },
        notify::connector::NotifyConnector,
    },
    shared::{Error, Result, ids::Id},
};

impl IncidentResponseService {
    /// 先开会（入会链接写进开场白），再建作战室。
    pub(super) async fn open_room(
        &self,
        incident: &Incident,
        settings: &ResponseSettings,
        response: &mut IncidentResponse,
    ) {
        let topic = format!("[{}] {}", incident.severity.as_str(), incident.summary);

        match self
            .book_meeting(incident, settings, response, &topic)
            .await
        {
            Ok(url) => response.meeting_url = url,
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "the meeting could not be booked")
            }
        }

        let Some(connector) = self
            .enabled_connector(incident, settings.war_room_connector_id.as_ref())
            .await
        else {
            return;
        };

        let people = self.people(incident, settings, response);
        let identities = self
            .identities(&incident.org_id, &connector.id, &people)
            .await;
        let owner = commander_identity(response, &identities);
        let intro = self.intro(incident, response);
        let members = identities.values().cloned().collect::<Vec<_>>();

        match war_room::create(&connector, &topic, owner, &members, &intro).await {
            Ok(room) => {
                self.record(
                    incident,
                    TimelineKind::WarRoomCreated,
                    response.declared_by.clone(),
                    json!({ "platform": room.platform, "url": room.url, "meetingUrl": response.meeting_url }),
                )
                .await;
                response.war_room = Some(room);
            }
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "war room could not be created")
            }
        }
    }

    /// 按设置开会，返回入会链接。飞书视频会议用所选的飞书 App（没选时用建群的连接器），会议所有人是
    /// 指挥官。
    async fn book_meeting(
        &self,
        incident: &Incident,
        settings: &ResponseSettings,
        response: &IncidentResponse,
        topic: &str,
    ) -> Result<Option<String>> {
        if settings.meeting != MeetingMode::FeishuVc {
            return meetings::book(settings, topic).await;
        }

        let connector_id = settings
            .meeting_connector_id
            .as_ref()
            .or(settings.war_room_connector_id.as_ref());
        let connector = self
            .enabled_connector(incident, connector_id)
            .await
            .ok_or_else(|| Error::invalid("no Feishu App connector is set for meetings"))?;

        let commander = response.roles.get(&ResponseRole::Commander).cloned();
        let identities = self
            .identities(&incident.org_id, &connector.id, commander.as_slice())
            .await;
        let owner = commander_identity(response, &identities);

        war_room::reserve_meeting(&connector, topic, owner)
            .await
            .map(Some)
    }

    /// 设置里选的连接器，读得到、启用着才用。
    async fn enabled_connector(
        &self,
        incident: &Incident,
        connector_id: Option<&Id>,
    ) -> Option<NotifyConnector> {
        let connector_id = connector_id?;

        match self.connectors.get(&incident.org_id, connector_id).await {
            Ok(connector) if connector.enabled => Some(connector),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "a response connector could not be read");
                None
            }
        }
    }

    fn intro(&self, incident: &Incident, response: &IncidentResponse) -> String {
        let mut lines = vec![
            format!("[{}] {}", incident.severity.as_str(), incident.summary),
            format!("{}/alerts/incidents/{}", self.external_url, incident.id),
        ];

        if let Some(meeting) = &response.meeting_url {
            lines.push(meeting.clone());
        }

        lines.join("\n")
    }

    /// 要知道的人：负责人、各角色与关注人，不重复。
    pub(super) fn people(
        &self,
        incident: &Incident,
        settings: &ResponseSettings,
        response: &IncidentResponse,
    ) -> Vec<Id> {
        let mut people: Vec<Id> = Vec::new();

        for id in incident
            .assignees
            .iter()
            .chain(response.roles.values())
            .chain(settings.stakeholder_ids.iter())
        {
            if !people.contains(id) {
                people.push(id.clone());
            }
        }

        people
    }

    /// 这些人在一个连接器里绑定的身份（飞书 open_id、Slack 用户 ID、钉钉 userId、企业微信 userid）。
    async fn identities(
        &self,
        org_id: &Id,
        connector_id: &Id,
        people: &[Id],
    ) -> HashMap<Id, String> {
        let endpoints = self
            .endpoints
            .list_for_organization(org_id)
            .await
            .unwrap_or_default();

        endpoints
            .into_iter()
            .filter(|endpoint| {
                endpoint.enabled
                    && &endpoint.connector_id == connector_id
                    && people.contains(&endpoint.user_id)
            })
            .map(|endpoint| (endpoint.user_id, endpoint.external_identity))
            .collect()
    }
}

/// 指挥官在这个连接器里的身份。
fn commander_identity<'a>(
    response: &IncidentResponse,
    identities: &'a HashMap<Id, String>,
) -> Option<&'a str> {
    response
        .roles
        .get(&ResponseRole::Commander)
        .and_then(|commander| identities.get(commander))
        .map(String::as_str)
}
