// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 照常通知：人看了觉得这个事件不该被压（[`crate::domain::alerting::inhibit`]）。立即升级、转派、增派也算
//! 人要叫人，顺带放开。放开之后这个事件不再被抑制；压着的通知由通知引擎下一次看时发出。

use serde_json::json;

use super::{Actor, IncidentActions, check_actionable};
use crate::{
    domain::alerting::{incident::Incident, timeline::TimelineKind},
    shared::{Result, ids::Id},
};

impl IncidentActions {
    /// 放开被抑制的事件。没压着时什么也不做。
    pub async fn lift_inhibition(&self, actor: &Actor, incident_id: &Id) -> Result<Incident> {
        let mut lifted = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;
                lifted = incident.handling.lift_inhibition(&actor.user_id);

                if lifted {
                    // 通知才刚要发出去：这一步的确认超时从现在算。
                    incident.current_step_started_at = actor.at;
                }

                Ok(lifted)
            })
            .await?;

        self.record_lift(actor, &incident, lifted).await;
        Ok(incident)
    }

    /// 真放开了就记一条时间线。
    pub(super) async fn record_lift(&self, actor: &Actor, incident: &Incident, lifted: bool) {
        if lifted {
            self.record(
                actor,
                incident,
                TimelineKind::InhibitionEnded,
                json!({ "reason": "lifted" }),
            )
            .await;
        }
    }
}
