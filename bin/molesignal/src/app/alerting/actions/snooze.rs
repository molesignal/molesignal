// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 暂缓：先放一放，到点退回未确认、重新通知负责人（由调度器做，见 [`super::super::dispatcher`]）。

use serde_json::json;

use super::{Actor, IncidentActions, check_actionable};
use crate::{
    domain::alerting::{
        handling::{MAX_SNOOZE_SECS, MIN_SNOOZE_SECS},
        incident::{Incident, IncidentStatus},
        timeline::TimelineKind,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl IncidentActions {
    /// 暂缓 `duration_secs` 秒。未确认的事件先由暂缓的人确认。
    pub async fn snooze(
        &self,
        actor: &Actor,
        incident_id: &Id,
        duration_secs: u32,
    ) -> Result<Incident> {
        if !(MIN_SNOOZE_SECS..=MAX_SNOOZE_SECS).contains(&duration_secs) {
            return Err(Error::invalid(format!(
                "a snooze lasts between {MIN_SNOOZE_SECS} and {MAX_SNOOZE_SECS} seconds"
            )));
        }

        let current = self.get(actor, incident_id).await?;
        check_actionable(&current)?;

        if current.status == IncidentStatus::Open {
            self.service
                .acknowledge(incident_id, actor.user_id.clone(), actor.at, actor.via)
                .await?;
        }

        let until = TimestampMicros(
            actor
                .at
                .0
                .saturating_add(i64::from(duration_secs) * 1_000_000),
        );

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                if incident.status != IncidentStatus::Acknowledged {
                    return Err(Error::conflict(
                        "only an acknowledged incident can be snoozed",
                    ));
                }

                incident.handling.snoozed_until = Some(until);
                incident.handling.snoozed_by = Some(actor.user_id.clone());
                Ok(true)
            })
            .await?;

        self.record(
            actor,
            &incident,
            TimelineKind::Snoozed,
            json!({ "until": until, "durationSecs": duration_secs }),
        )
        .await;

        Ok(incident)
    }

    /// 取消暂缓：事件仍是已确认。没在暂缓时什么也不做。
    pub async fn cancel_snooze(&self, actor: &Actor, incident_id: &Id) -> Result<Incident> {
        let mut cancelled = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                if incident.handling.snoozed_until.is_none() {
                    return Ok(false);
                }

                incident.handling.clear_snooze();
                cancelled = true;
                Ok(true)
            })
            .await?;

        if cancelled {
            self.record(actor, &incident, TimelineKind::SnoozeCancelled, json!({}))
                .await;
        }

        Ok(incident)
    }
}
