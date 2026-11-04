// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 读事件的时间线：记下的经过、通知了谁，以及并入它的事件。

use serde::Serialize;

use super::{Actor, IncidentActions};
use crate::{
    domain::{
        alerting::{
            incident::{IncidentStatus, Severity},
            timeline::TimelineEntry,
        },
        notify::delivery::{DeliveryStage, DeliveryStatus},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 时间线最多列多少条投递。
const MAX_DELIVERIES: u32 = 500;

/// 一个事件的时间线。触发与自动恢复由读的一方从事件本身补上。
#[derive(Debug, Clone, Serialize)]
pub struct IncidentTimeline {
    pub entries: Vec<TimelineEntry>,
    /// 这个事件的通知投递，按时间先后；同一次通知（`event_id`）的几条归在一起看。
    pub deliveries: Vec<TimelineDelivery>,
    /// 并入这个事件的事件。
    pub merged: Vec<MergedIncident>,
}

/// 一条通知投递：通知了谁、经哪个连接器、结果如何。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDelivery {
    pub id: Id,
    pub event_id: String,
    pub user_id: Option<Id>,
    pub connector_id: Option<Id>,
    /// 遮住一部分的地址（号码、邮箱、群）。
    pub target: Option<String>,
    pub stage: DeliveryStage,
    pub status: DeliveryStatus,
    pub error: Option<String>,
    pub created_at: TimestampMicros,
    pub delivered_at: Option<TimestampMicros>,
}

/// 并入的一个事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedIncident {
    pub id: Id,
    pub summary: String,
    pub status: IncidentStatus,
    pub severity: Severity,
    pub created_at: TimestampMicros,
    pub resolved_at: Option<TimestampMicros>,
}

impl IncidentActions {
    pub async fn timeline(&self, actor: &Actor, incident_id: &Id) -> Result<IncidentTimeline> {
        let incident = self.get(actor, incident_id).await?;
        let entries = self.timeline.list(&incident.org_id, &incident.id).await?;

        let deliveries = match &self.deliveries {
            Some(deliveries) => deliveries
                .list_for_alert(&incident.org_id, &incident.id, MAX_DELIVERIES)
                .await?
                .into_iter()
                .map(|delivery| TimelineDelivery {
                    id: delivery.id,
                    event_id: delivery.event_id,
                    user_id: delivery.recipient_user_id,
                    connector_id: delivery.connector_id,
                    target: delivery.target_value_masked,
                    stage: delivery.stage,
                    status: delivery.status,
                    error: delivery.error_message,
                    created_at: delivery.created_at,
                    delivered_at: delivery.delivered_at,
                })
                .collect(),
            None => Vec::new(),
        };

        let merged = self
            .incidents
            .list_merged_into(&incident.org_id, &incident.id)
            .await?
            .into_iter()
            .map(|child| MergedIncident {
                id: child.id,
                summary: child.summary,
                status: child.status,
                severity: child.severity,
                created_at: child.created_at,
                resolved_at: child.resolved_at,
            })
            .collect();

        Ok(IncidentTimeline {
            entries,
            deliveries,
            merged,
        })
    }

    /// 最近的几条备注，新的在前：给 Mole Agent 读事件时带上。
    pub async fn recent_notes(
        &self,
        actor: &Actor,
        incident_id: &Id,
        limit: u32,
    ) -> Result<Vec<TimelineEntry>> {
        let incident = self.get(actor, incident_id).await?;

        self.timeline
            .recent_notes(&incident.org_id, &incident.id, limit)
            .await
    }
}
