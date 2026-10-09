// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级派发器：推进 open incident 的 escalation step，并把每个升级目标转为
//! `alert.escalated` Notify 事件。连接器选择、接收人展开和投递状态均由 Notify
//! 引擎负责。

use std::sync::Arc;

use crate::{
    app::notify::{NotifyEngine, alert_escalation_dispatch},
    domain::alerting::{
        escalation::EscalationPolicy,
        incident::{Incident, IncidentStatus, Severity},
        mute::{MuteRuleRepository, match_labels_for_incident},
        repositories::{EscalationPolicyRepository, IncidentRepository, ScheduleRepository},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod assignees;
use assignees::{ScheduleCache, apply_assignees};

pub struct EscalationDispatcher {
    incidents: Arc<dyn IncidentRepository>,
    policies: Arc<dyn EscalationPolicyRepository>,
    notify_engine: Arc<NotifyEngine>,
    /// 告警屏蔽（可选）：命中时暂停派发与升级推进。未注入 = 不屏蔽。
    mute_rules: Option<Arc<dyn MuteRuleRepository>>,
    /// 排班（可选）：用来解析 `schedule` 目标此刻的值班人并记为事件负责人。
    schedules: Option<Arc<dyn ScheduleRepository>>,
}

impl EscalationDispatcher {
    pub fn new(
        incidents: Arc<dyn IncidentRepository>,
        policies: Arc<dyn EscalationPolicyRepository>,
        notify_engine: Arc<NotifyEngine>,
    ) -> Self {
        Self {
            incidents,
            policies,
            notify_engine,
            mute_rules: None,
            schedules: None,
        }
    }

    /// 注入告警屏蔽仓库；接上后命中 mute 的 incident 暂停派发。
    pub fn with_mute_rules(mut self, mute_rules: Arc<dyn MuteRuleRepository>) -> Self {
        self.mute_rules = Some(mute_rules);
        self
    }

    /// 注入排班仓库；接上后 `schedule` 目标的值班人会写进事件的 `assignees`。
    pub fn with_schedules(mut self, schedules: Arc<dyn ScheduleRepository>) -> Self {
        self.schedules = Some(schedules);
        self
    }

    /// 单次调度：扫所有 active incident；只推进 open 状态，acknowledged 状态不再升级。
    #[tracing::instrument(
        name = "worker.alert_dispatcher",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "alert_dispatcher")
    )]
    pub async fn tick(&self, org_id: &Id, now: TimestampMicros) -> Result<()> {
        let open: Vec<Incident> = self
            .incidents
            .list_active(org_id)
            .await?
            .into_iter()
            .filter(|incident| incident.status == IncidentStatus::Open)
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        let policies = self.policies.list(org_id).await?;
        let mut schedules = ScheduleCache::new(self.schedules.as_deref());
        for incident in open {
            if let Err(error) = self.advance(incident, &policies, &mut schedules, now).await {
                tracing::warn!(error = %error, "dispatcher step failed");
            }
        }
        Ok(())
    }

    async fn advance(
        &self,
        mut incident: Incident,
        policies: &[EscalationPolicy],
        schedules: &mut ScheduleCache<'_>,
        now: TimestampMicros,
    ) -> Result<()> {
        if self.is_muted(&incident, now).await {
            return Ok(());
        }

        // 别名 `default` 落到同名策略；找不到策略的事件没有可升级的对象。
        let Some(policy) = EscalationPolicy::resolve(policies, &incident.escalation_policy_id)
        else {
            return Ok(());
        };
        let severity = incident.severity;
        let Some(step_index) =
            next_applicable_step(policy, incident.current_step as usize, severity)
        else {
            return Ok(());
        };

        let mut changed = false;
        if incident.escalation_policy_id != policy.id {
            // 把别名固化成真实策略 id：之后改名或另建 default 策略都不会让进行中的事件换策略。
            incident.escalation_policy_id = policy.id.clone();
            changed = true;
        }
        if step_index as u32 != incident.current_step {
            incident.current_step = step_index as u32;
            incident.current_step_started_at = now;
            changed = true;
        }
        changed |= apply_assignees(&mut incident, &policy.steps[step_index], schedules, now).await;
        if changed {
            // 事件在评估到写入之间被确认或解决时，写入落空，不再继续升级。
            let Some(updated) = self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            else {
                return Ok(());
            };
            incident = updated;
        }

        // enqueue 使用稳定事件 ID，重复 tick 不会产生重复事件或重复投递。
        self.dispatch_step(&incident, policy, step_index).await;

        let step = &policy.steps[step_index];
        if !incident.step_timed_out(step.ack_timeout_secs, now) {
            return Ok(());
        }

        if let Some(next) = next_applicable_step(policy, step_index + 1, severity) {
            incident.current_step = next as u32;
            incident.current_step_started_at = now;
            apply_assignees(&mut incident, &policy.steps[next], schedules, now).await;
            let Some(incident) = self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            else {
                return Ok(());
            };
            self.dispatch_step(&incident, policy, next).await;
            return Ok(());
        }

        if policy.repeat
            && incident.current_loop + 1 < policy.max_loops.max(1)
            && let Some(first) = next_applicable_step(policy, 0, severity)
        {
            incident.current_loop += 1;
            incident.current_step = first as u32;
            incident.current_step_started_at = now;
            apply_assignees(&mut incident, &policy.steps[first], schedules, now).await;
            let Some(incident) = self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            else {
                return Ok(());
            };
            self.dispatch_step(&incident, policy, first).await;
        }
        Ok(())
    }

    async fn dispatch_step(
        &self,
        incident: &Incident,
        policy: &EscalationPolicy,
        step_index: usize,
    ) {
        let Some(step) = policy.steps.get(step_index) else {
            return;
        };
        for (target_index, target) in step.targets.iter().enumerate() {
            let dispatch = alert_escalation_dispatch(
                incident,
                step_index,
                incident.current_loop,
                target_index,
                target,
            );
            if let Err(error) = self.notify_engine.enqueue_event(dispatch).await {
                tracing::warn!(
                    incident_id = %incident.id,
                    step_index,
                    target_index,
                    error = %error,
                    "notify alert escalation event enqueue failed"
                );
            }
        }
    }

    /// 该 incident 是否被任一 enabled mute 规则屏蔽。未注入 mute 仓库时恒 false。
    async fn is_muted(&self, incident: &Incident, now: TimestampMicros) -> bool {
        let Some(repo) = &self.mute_rules else {
            return false;
        };
        let match_labels = match_labels_for_incident(&incident.labels, &incident.id);
        match repo.list_enabled(&incident.org_id).await {
            Ok(rules) => rules.iter().any(|rule| rule.is_muting(&match_labels, now)),
            Err(error) => {
                tracing::warn!(error = %error, "mute rule check failed; not muting");
                false
            }
        }
    }
}

/// 从 `from` 起找首个对 `severity` 适用的 step 下标。
fn next_applicable_step(
    policy: &EscalationPolicy,
    from: usize,
    severity: Severity,
) -> Option<usize> {
    (from..policy.steps.len()).find(|&index| policy.steps[index].applies_to(severity))
}
