// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警洞察里人的部分：响应（MTTA、升级率、各级确认率、确认途径，按团队、服务、个人拆分，和上一个
//! 同长度时段比）和每个人的值班负担。

mod load;
mod response;

use std::{collections::HashMap, sync::Arc};

pub use load::{NIGHT_END_HOUR, NIGHT_START_HOUR, PersonLoad, oncall_load};
pub use response::{
    BreakdownRow, Dimension, Durations, FirstResponse, IncidentFacts, ResponseSummary, StepRate,
    breakdown, incident_facts, summarize,
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::{
        alerting::{
            incident::Incident,
            repositories::{IncidentRepository, ScheduleRepository},
            timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
        },
        notify::repositories::{NotifyDeliveryRepository, UserNotifyPreferenceRepository},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一段时间的响应，和它之前同样长的一段。
#[derive(Debug, Clone, Serialize)]
pub struct ResponseInsights {
    pub current: ResponseSummary,
    pub previous: ResponseSummary,
    pub breakdown: ResponseBreakdown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseBreakdown {
    pub teams: Vec<BreakdownRow>,
    pub services: Vec<BreakdownRow>,
    pub people: Vec<BreakdownRow>,
    pub priorities: Vec<BreakdownRow>,
}

pub struct AlertInsights {
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
    schedules: Arc<dyn ScheduleRepository>,
    deliveries: Arc<dyn NotifyDeliveryRepository>,
    preferences: Arc<dyn UserNotifyPreferenceRepository>,
}

impl AlertInsights {
    pub fn new(
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
        schedules: Arc<dyn ScheduleRepository>,
        deliveries: Arc<dyn NotifyDeliveryRepository>,
        preferences: Arc<dyn UserNotifyPreferenceRepository>,
    ) -> Self {
        Self {
            incidents,
            timeline,
            schedules,
            deliveries,
            preferences,
        }
    }

    /// `[from, to)` 里开的事件的响应，以及前一段同样长的时间里的。并入别处的事件不算。
    pub async fn response(
        &self,
        org_id: &Id,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Result<ResponseInsights> {
        let previous_from = TimestampMicros(from.0 - (to.0 - from.0));
        let facts = self.facts(org_id, previous_from, to).await?;

        let (current, previous): (Vec<_>, Vec<_>) = facts
            .into_iter()
            .partition(|(created_at, _)| *created_at >= from.0);

        let current: Vec<IncidentFacts> = current.into_iter().map(|(_, fact)| fact).collect();
        let previous: Vec<IncidentFacts> = previous.into_iter().map(|(_, fact)| fact).collect();

        Ok(ResponseInsights {
            current: summarize(&current),
            previous: summarize(&previous),
            breakdown: ResponseBreakdown {
                teams: breakdown(&current, Dimension::Team),
                services: breakdown(&current, Dimension::Service),
                people: breakdown(&current, Dimension::Person),
                priorities: breakdown(&current, Dimension::Priority),
            },
        })
    }

    /// `[from, to)` 里每个人的值班负担，带上他在这段时间里先接手的事件数和平均接手时间。`schedule`
    /// 给了时只算这个排班。
    pub async fn oncall_load(
        &self,
        org_id: &Id,
        from: TimestampMicros,
        to: TimestampMicros,
        schedule: Option<&Id>,
    ) -> Result<Vec<PersonLoad>> {
        let schedules = self.schedules.list(org_id).await?;
        let pages = self.deliveries.pages_between(org_id, from, to).await?;

        let quiet_zones: HashMap<Id, String> = self
            .preferences
            .list_for_organization(org_id)
            .await?
            .into_iter()
            .filter_map(|preference| {
                let zone = preference
                    .quiet_hours
                    .as_ref()
                    .and_then(|quiet| quiet.get("timezone"))
                    .and_then(Value::as_str)?
                    .to_string();

                Some((preference.user_id, zone))
            })
            .collect();

        let mut loads = oncall_load(&schedules, &pages, &quiet_zones, from, to, schedule);

        let mut responses: HashMap<Id, Vec<i64>> = HashMap::new();

        for (_, fact) in self.facts(org_id, from, to).await? {
            if let Some(FirstResponse {
                after,
                by: Some(by),
                ..
            }) = fact.response
            {
                responses.entry(by).or_default().push(after);
            }
        }

        for load in &mut loads {
            if let Some(afters) = responses.get(&load.user_id) {
                load.responded = u32::try_from(afters.len()).unwrap_or(u32::MAX);
                load.mtta_secs =
                    afters.iter().sum::<i64>() as f64 / afters.len() as f64 / 1_000_000.0;
            }
        }

        Ok(loads)
    }

    /// `[from, to)` 里开的、没并入别处的事件，各自的开始时刻和响应。
    async fn facts(
        &self,
        org_id: &Id,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Result<Vec<(i64, IncidentFacts)>> {
        let incidents: Vec<Incident> = self
            .incidents
            .list_since(org_id, from)
            .await?
            .into_iter()
            .filter(|incident| !incident.is_merged() && incident.created_at.0 < to.0)
            .collect();

        let ids: Vec<Id> = incidents
            .iter()
            .map(|incident| incident.id.clone())
            .collect();

        let mut entries: HashMap<Id, Vec<TimelineEntry>> = HashMap::new();

        for entry in self
            .timeline
            .list_for_incidents(
                org_id,
                &ids,
                &[
                    TimelineKind::Acknowledged,
                    TimelineKind::Resolved,
                    TimelineKind::Escalated,
                ],
            )
            .await?
        {
            entries
                .entry(entry.incident_id.clone())
                .or_default()
                .push(entry);
        }

        Ok(incidents
            .iter()
            .map(|incident| {
                let own = entries
                    .get(&incident.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                (incident.created_at.0, incident_facts(incident, own))
            })
            .collect())
    }
}
