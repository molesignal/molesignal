// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 立即升级与改严重度。

use serde_json::json;

use super::{Actor, IncidentActions, check_actionable, reopen};
use crate::{
    app::alerting::dispatcher,
    domain::alerting::{
        handling::EscalationReason,
        incident::{Incident, IncidentStatus, Severity},
        timeline::TimelineKind,
    },
    shared::{Error, Result, ids::Id},
};

impl IncidentActions {
    /// 立即升级到 `step`（从 0 数），不给时升到下一个适用的步骤；也可以选回前面的步骤再叫一遍。
    /// 换一轮通知，立即通知那一步的对象；已确认的事件退回未确认。
    pub async fn escalate(
        &self,
        actor: &Actor,
        incident_id: &Id,
        step: Option<u32>,
    ) -> Result<Incident> {
        let current = self.get(actor, incident_id).await?;
        check_actionable(&current)?;

        let Some(policy) = self.policy_of(&current).await? else {
            return Err(Error::invalid(
                "this incident has no escalation policy to escalate through",
            ));
        };

        let mut lifted = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                let target = match step {
                    Some(step) => {
                        let step = step as usize;

                        let Some(chosen) = policy.steps.get(step) else {
                            return Err(Error::invalid(format!(
                                "the escalation policy has {} steps",
                                policy.steps.len()
                            )));
                        };

                        if !chosen.applies_to(incident.severity) {
                            return Err(Error::invalid(
                                "that step does not apply to this incident's severity",
                            ));
                        }

                        step
                    }
                    None => ((incident.current_step as usize + 1)..policy.steps.len())
                        .find(|&index| policy.steps[index].applies_to(incident.severity))
                        .ok_or_else(|| {
                            Error::invalid("this is the last step; choose a step to escalate to")
                        })?,
                };

                incident.escalation_policy_id = policy.id.clone();
                incident.current_step = target as u32;
                incident.handling.assignees_pinned = false;
                incident.handling.next_round(EscalationReason::Manual);
                lifted = incident.handling.lift_inhibition(&actor.user_id);
                reopen(incident, actor.at);
                Ok(true)
            })
            .await?;

        self.record_lift(actor, &incident, lifted).await;

        let detail = json!({
            "step": incident.current_step,
            "loop": incident.current_loop,
            "reason": EscalationReason::Manual.as_str(),
            "targets": dispatcher::step_targets(&policy, incident.current_step as usize),
        });

        self.record(actor, &incident, TimelineKind::Escalated, detail)
            .await;
        self.page_step(&incident, &policy).await;
        self.emit(&incident).await;

        Ok(incident)
    }

    /// 改严重度。改过之后评估器不再改它。调高时，未确认的事件按新的严重度从第一步重新通知（和
    /// 评估器自己调高一样）；调低只改严重度。
    pub async fn set_severity(
        &self,
        actor: &Actor,
        incident_id: &Id,
        severity: Severity,
    ) -> Result<Incident> {
        let current = self.get(actor, incident_id).await?;
        check_actionable(&current)?;

        let policy = self.policy_of(&current).await?;
        let mut from = None;
        let mut raised = false;

        let incident = self
            .modify(actor, incident_id, |incident| {
                check_actionable(incident)?;

                if incident.severity == severity && incident.handling.severity_set_by.is_some() {
                    return Ok(false);
                }

                from = Some(incident.severity);
                raised = severity > incident.severity && incident.status == IncidentStatus::Open;

                incident.severity = severity;
                incident.handling.severity_set_by = Some(actor.user_id.clone());

                if raised {
                    let first = policy.as_ref().and_then(|policy| {
                        (0..policy.steps.len())
                            .find(|&index| policy.steps[index].applies_to(severity))
                    });

                    incident.current_step = first.unwrap_or_default() as u32;
                    incident.current_loop = 0;
                    incident.current_step_started_at = actor.at;
                    incident.handling.assignees_pinned = false;
                    incident
                        .handling
                        .next_round(EscalationReason::SeverityRaised);
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
            TimelineKind::SeverityChanged,
            json!({ "from": from, "to": severity }),
        )
        .await;

        if raised && let Some(policy) = &policy {
            self.page_step(&incident, policy).await;
        }

        self.emit(&incident).await;
        Ok(incident)
    }
}
