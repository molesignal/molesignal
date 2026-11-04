// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 紧急度在升级里的两件事（见 [`crate::domain::alerting::importance`]）：
//!
//! - 不往下升的那几档（内置的低）只通知升级策略的第一级，不往下升、不整轮重复；策略开了
//!   `escalate_low_urgency` 时照常。
//! - 策略按支持时段定紧急度、又要在支持时段开始时升级的：开事件以后支持时段开始过、还没人确认、紧急度
//!   是按策略定的、又比时段内那一档不急，就升到那一档，从第一级重新通知（新一轮，原因
//!   `urgency_raised`），时间线记一笔。

use serde_json::json;

use super::{
    EscalationDispatcher,
    assignees::{ScheduleCache, apply_assignees},
    enqueue_step, next_applicable_step,
};
use crate::{
    domain::alerting::{
        escalation::EscalationPolicy,
        handling::EscalationReason,
        importance::ImportanceLevels,
        incident::{Incident, IncidentStatus},
        timeline::TimelineKind,
    },
    shared::{Result, time::TimestampMicros},
};

/// 这一档不往下升、策略又不让它照常升级：停在这一级。
pub(super) fn holds_at_this_step(
    incident: &Incident,
    policy: &EscalationPolicy,
    levels: &ImportanceLevels,
) -> bool {
    !levels.urgency(&incident.urgency()).escalates && !policy.escalate_low_urgency
}

impl EscalationDispatcher {
    /// 支持时段开始了：把该升的事件升到时段内那一档，从第一级重新通知。返回升了没有。
    pub(super) async fn raise_for_support_hours(
        &self,
        incident: &Incident,
        policy: &EscalationPolicy,
        levels: &ImportanceLevels,
        schedules: &mut ScheduleCache<'_>,
        now: TimestampMicros,
    ) -> Result<bool> {
        let current = incident.urgency();

        let Some(target) = policy.urgency_rule.raises_to() else {
            return Ok(false);
        };

        if levels.rank(&current) <= levels.rank(target)
            || !incident.handling.urgency_by_policy
            || incident.handling.urgency_set_by.is_some()
        {
            return Ok(false);
        }

        let Some(began) = policy
            .urgency_rule
            .support_hours_began(incident.created_at, now)
        else {
            return Ok(false);
        };

        let Some(first) = next_applicable_step(policy, 0, incident.severity) else {
            return Ok(false);
        };

        let mut raised = incident.clone();
        raised.handling.urgency = Some(target.clone());
        raised.handling.urgency_by_policy = false;
        raised.handling.next_round(EscalationReason::UrgencyRaised);
        raised.handling.assignees_pinned = false;
        raised.current_step = first as u32;
        raised.current_loop = 0;
        raised.current_step_started_at = now;
        apply_assignees(&mut raised, &policy.steps[first], schedules, now).await;

        let Some(raised) = self
            .incidents
            .update_if_status(raised, &[IncidentStatus::Open])
            .await?
        else {
            return Ok(false);
        };

        let detail = json!({
            "from": current.as_str(),
            "to": target.as_str(),
            "reason": "support_hours",
            "supportHoursBeganAt": began,
        });

        self.record(&raised, TimelineKind::UrgencyChanged, detail, now)
            .await;
        enqueue_step(&self.notify_engine, &raised, policy, first).await;
        Ok(true)
    }
}
