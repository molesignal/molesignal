// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 改优先级与紧急度。

use serde_json::json;

use super::{Actor, IncidentActions, check_actionable};
use crate::{
    domain::alerting::{
        handling::EscalationReason,
        incident::{Incident, IncidentStatus},
        priority::Priority,
        timeline::TimelineKind,
        urgency::Urgency,
    },
    shared::{Result, ids::Id},
};

impl IncidentActions {
    /// 改优先级（`None` 是不设）。只改优先级，不重新通知。
    pub async fn set_priority(
        &self,
        actor: &Actor,
        incident_id: &Id,
        priority: Option<Priority>,
    ) -> Result<Incident> {
        let current = self.get(actor, incident_id).await?;
        check_actionable(&current)?;

        if let Some(priority) = priority {
            self.levels(&current.org_id)
                .await
                .check_priority(priority)?;
        }

        let mut from = None;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                if incident.handling.priority == priority {
                    return Ok(false);
                }

                from = Some(incident.handling.priority);
                incident.handling.priority = priority;
                Ok(true)
            })
            .await?;

        let Some(from) = from else {
            return Ok(incident);
        };

        self.record(
            actor,
            &incident,
            TimelineKind::PriorityChanged,
            json!({ "from": from, "to": priority }),
        )
        .await;

        self.emit(&incident).await;
        Ok(incident)
    }

    /// 改紧急度。改过之后支持时段不再改它。调到更急的一档时，未确认的事件从第一步重新通知（新一轮，
    /// 原因 `urgency_raised`）；调到不急的一档只改紧急度，之后按那一档升不升。
    pub async fn set_urgency(
        &self,
        actor: &Actor,
        incident_id: &Id,
        urgency: Urgency,
    ) -> Result<Incident> {
        let current = self.get(actor, incident_id).await?;
        check_actionable(&current)?;

        let levels = self.levels(&current.org_id).await;
        levels.check_urgency(&urgency)?;

        let policy = self.policy_of(&current).await?;
        let mut from = None;
        let mut raised = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                if incident.urgency() == urgency && incident.handling.urgency_set_by.is_some() {
                    return Ok(false);
                }

                from = Some(incident.urgency());
                raised = levels.rank(&urgency) < levels.rank(&incident.urgency())
                    && incident.status == IncidentStatus::Open;

                incident.handling.urgency = Some(urgency.clone());
                incident.handling.urgency_set_by = Some(actor.user_id.clone());
                incident.handling.urgency_by_policy = false;

                if raised {
                    let first = policy.as_ref().and_then(|policy| {
                        (0..policy.steps.len())
                            .find(|&index| policy.steps[index].applies_to(incident.severity))
                    });

                    incident.current_step = first.unwrap_or_default() as u32;
                    incident.current_loop = 0;
                    incident.current_step_started_at = actor.at;
                    incident.handling.assignees_pinned = false;
                    incident
                        .handling
                        .next_round(EscalationReason::UrgencyRaised);
                }

                Ok(true)
            })
            .await?;

        let Some(from) = from else {
            return Ok(incident);
        };

        self.record(
            actor,
            &incident,
            TimelineKind::UrgencyChanged,
            json!({ "from": from, "to": urgency }),
        )
        .await;

        if raised && let Some(policy) = &policy {
            self.page_step(&incident, policy).await;
        }

        self.emit(&incident).await;
        Ok(incident)
    }
}
