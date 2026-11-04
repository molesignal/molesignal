// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级派发器：推进 open incident 的 escalation step，并把每个升级目标转为
//! `alert.escalated` Notify 事件。连接器选择、接收人展开和投递状态均由 Notify
//! 引擎负责。
//!
//! 已确认的事件不再升级，但暂缓到点、或确认后过了升级策略设的时长还没解决时，退回未确认并重新通知
//! 负责人（[`EscalationReason::SnoozeEnded`]、[`EscalationReason::AckTimeout`]）。并入了别的事件的
//! 事件不升级。
//!
//! 多个 alert 节点按事件所属规则（`rule_id`）分担升级，与评估同一个 owner。事件 ID 稳定、写回走
//! `update_if_status`（比对状态与 `revision`），成员变化时两个节点短暂同时推进同一个事件也不会产生
//! 重复事件。

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    app::{
        cluster::{OwnershipView, PeerRole, WorkOwnership},
        notify::{NotifyEngine, alert_escalation_dispatch, alert_page_dispatch},
    },
    domain::alerting::{
        escalation::EscalationPolicy,
        handling::{EscalationReason, IncidentHandling},
        importance::ImportanceLevels,
        incident::{Incident, IncidentStatus, Severity},
        mute::{MuteRuleRepository, match_labels_for_incident},
        repositories::{EscalationPolicyRepository, IncidentRepository, ScheduleRepository},
        timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod assignees;
mod reach;
mod urgency;
use assignees::{ScheduleCache, apply_assignees};
use reach::ReachCache;
use urgency::holds_at_this_step;

pub struct EscalationDispatcher {
    incidents: Arc<dyn IncidentRepository>,
    policies: Arc<dyn EscalationPolicyRepository>,
    notify_engine: Arc<NotifyEngine>,
    /// 告警屏蔽（可选）：命中时暂停派发与升级推进。未注入 = 不屏蔽。
    mute_rules: Option<Arc<dyn MuteRuleRepository>>,
    /// 排班（可选）：用来解析 `schedule` 目标此刻的值班人并记为事件负责人。
    schedules: Option<Arc<dyn ScheduleRepository>>,
    /// 事件的时间线（可选）：升级到下一步、退回未确认记在这里。
    timeline: Option<Arc<dyn IncidentTimelineRepository>>,
    /// 多个 alert 节点按事件所属规则分担升级；默认不分担。
    ownership: Arc<WorkOwnership>,
    /// 组织的紧急度档位（可选）：哪一档往下升、支持时段开始时升到哪一档。未注入 = 内置的高、低。
    importance: Option<Arc<crate::app::alerting::ImportanceService>>,
    /// 告警抑制（可选）：每轮判定谁在压谁，被压的事件不往下升。未注入 = 不抑制。
    inhibitions: Option<Arc<crate::app::alerting::Inhibitions>>,
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
            timeline: None,
            ownership: WorkOwnership::solo(PeerRole::Alert),
            importance: None,
            inhibitions: None,
        }
    }

    /// 接上告警抑制：每轮判定谁在压谁，写回本节点负责的事件。
    pub fn with_inhibitions(mut self, inhibitions: Arc<crate::app::alerting::Inhibitions>) -> Self {
        self.inhibitions = Some(inhibitions);
        self
    }

    /// 注入组织的紧急度档位。
    pub fn with_importance(
        mut self,
        importance: Arc<crate::app::alerting::ImportanceService>,
    ) -> Self {
        self.importance = Some(importance);
        self
    }

    /// 与其余 alert 节点分担升级：`tick` 只推进所属规则归本节点的事件。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
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

    /// 注入时间线仓库；接上后升级到下一步、退回未确认会记在事件的时间线上。
    pub fn with_timeline(mut self, timeline: Arc<dyn IncidentTimelineRepository>) -> Self {
        self.timeline = Some(timeline);
        self
    }

    /// 单次调度：扫 `org_id` 下所有 active incident，推进所属规则归本节点的 open 事件，并把暂缓或
    /// 确认超时到点的已确认事件退回未确认。返回本节点处理的事件数。
    #[tracing::instrument(
        name = "worker.alert_dispatcher",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "alert_dispatcher")
    )]
    pub async fn tick(&self, org_id: &Id, now: TimestampMicros) -> Result<usize> {
        let view = self.ownership.view().await;
        let active = self.incidents.list_active(org_id).await?;

        let active = match &self.inhibitions {
            Some(inhibitions) => {
                let owned = |incident: &Incident| view.owns(incident.rule_id.as_str());
                inhibitions.settle(org_id, active, owned, now).await
            }
            None => active,
        };

        let acknowledged = owned_acknowledged_incidents(&active, &view);
        let open = owned_open_incidents(active, &view);

        if open.is_empty() && acknowledged.is_empty() {
            return Ok(0);
        }

        let policies = self.policies.list(org_id).await?;
        let levels =
            crate::app::alerting::importance::levels_of(self.importance.as_ref(), org_id).await;
        let mut schedules = ScheduleCache::new(self.schedules.as_deref());
        let mut reach = ReachCache::new();
        let owned = open.len() + acknowledged.len();

        for incident in acknowledged {
            let policy = EscalationPolicy::resolve(&policies, &incident.escalation_policy_id);

            let Some(reason) = retrigger_due(&incident, policy, now) else {
                continue;
            };

            if let Err(error) = self.retrigger(incident, reason, policy, now).await {
                tracing::warn!(error = %error, "acknowledged incident could not be re-triggered");
            }
        }

        for incident in open {
            if let Err(error) = self
                .advance(
                    incident,
                    &policies,
                    &levels,
                    &mut schedules,
                    &mut reach,
                    now,
                )
                .await
            {
                tracing::warn!(error = %error, "dispatcher step failed");
            }
        }

        Ok(owned)
    }

    async fn advance(
        &self,
        mut incident: Incident,
        policies: &[EscalationPolicy],
        levels: &ImportanceLevels,
        schedules: &mut ScheduleCache<'_>,
        reach: &mut ReachCache,
        now: TimestampMicros,
    ) -> Result<()> {
        if incident.handling.inhibition_holds(now) || self.is_muted(&incident, now).await {
            return Ok(());
        }

        // 别名 `default` 落到同名策略；找不到策略的事件没有可升级的对象。
        let Some(policy) = EscalationPolicy::resolve(policies, &incident.escalation_policy_id)
        else {
            return Ok(());
        };

        // 支持时段开始时，按策略定成不急的事件升到时段内那一档，从第一级重新通知。
        if self
            .raise_for_support_hours(&incident, policy, levels, schedules, now)
            .await?
        {
            return Ok(());
        }

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
            incident.handling.assignees_pinned = false;
            changed = true;
        }

        changed |= apply_assignees(&mut incident, &policy.steps[step_index], schedules, now).await;

        if changed {
            // 事件在评估到写入之间被确认、解决或改过时，写入落空，下一轮再看。
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
        if !told_by_hand(&incident.handling) {
            enqueue_step(&self.notify_engine, &incident, policy, step_index).await;
        }

        let step = &policy.steps[step_index];

        // 不往下升的那几档紧急度停在这一级，不整轮重复。
        if holds_at_this_step(&incident, policy, levels) {
            return Ok(());
        }

        // 谁都收不到的一级不等确认超时。
        let unreachable = !incident.step_timed_out(step.ack_timeout_secs, now)
            && next_applicable_step(policy, step_index + 1, severity).is_some()
            && self.step_unreachable(&incident, step, reach).await;

        if !unreachable && !incident.step_timed_out(step.ack_timeout_secs, now) {
            return Ok(());
        }

        if unreachable {
            self.record_unreachable(&incident, step_index, now).await;
        }

        if let Some(next) = next_applicable_step(policy, step_index + 1, severity) {
            incident.current_step = next as u32;
            incident.current_step_started_at = now;
            incident.handling.assignees_pinned = false;
            incident.handling.dispatch_reason = EscalationReason::Step;
            apply_assignees(&mut incident, &policy.steps[next], schedules, now).await;

            let Some(incident) = self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            else {
                return Ok(());
            };

            self.record_step(&incident, policy, next, now).await;
            enqueue_step(&self.notify_engine, &incident, policy, next).await;
            return Ok(());
        }

        if policy.repeat
            && incident.current_loop + 1 < policy.max_loops.max(1)
            && let Some(first) = next_applicable_step(policy, 0, severity)
        {
            incident.current_loop += 1;
            incident.current_step = first as u32;
            incident.current_step_started_at = now;
            incident.handling.assignees_pinned = false;
            incident.handling.dispatch_reason = EscalationReason::Step;
            apply_assignees(&mut incident, &policy.steps[first], schedules, now).await;

            let Some(incident) = self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            else {
                return Ok(());
            };

            self.record_step(&incident, policy, first, now).await;
            enqueue_step(&self.notify_engine, &incident, policy, first).await;
        }

        Ok(())
    }

    /// 暂缓到点或确认超时：退回未确认，重新通知负责人。没有负责人（这一步只广播给团队）时换一轮，
    /// 重新通知这一步的对象。之后没人确认，照升级策略往下升级。
    async fn retrigger(
        &self,
        incident: Incident,
        reason: EscalationReason,
        policy: Option<&EscalationPolicy>,
        now: TimestampMicros,
    ) -> Result<()> {
        let mut next = incident;
        let page_assignees = !next.assignees.is_empty();

        next.status = IncidentStatus::Open;
        next.acknowledged_at = None;
        next.acknowledged_by = None;
        next.current_step_started_at = now;
        next.handling.clear_snooze();

        if page_assignees {
            next.handling.assignees_pinned = true;
        } else {
            next.handling.next_round(reason);
        }

        let Some(incident) = self
            .incidents
            .update_if_status(next, &[IncidentStatus::Acknowledged])
            .await?
        else {
            return Ok(());
        };

        let key = self
            .record(
                &incident,
                TimelineKind::Retriggered,
                json!({ "reason": reason.as_str() }),
                now,
            )
            .await;

        if page_assignees {
            for user_id in &incident.assignees {
                let page = alert_page_dispatch(&incident, user_id, reason, &key);

                if let Err(error) = self.notify_engine.enqueue_event(page).await {
                    tracing::warn!(incident_id = %incident.id, %error, "re-trigger notification could not be enqueued");
                }
            }
        } else if let Some(policy) = policy {
            enqueue_step(
                &self.notify_engine,
                &incident,
                policy,
                incident.current_step as usize,
            )
            .await;
        }

        Ok(())
    }

    /// 时间线上记下照常升级到了哪一步。
    async fn record_step(
        &self,
        incident: &Incident,
        policy: &EscalationPolicy,
        step_index: usize,
        now: TimestampMicros,
    ) {
        let detail = json!({
            "step": step_index,
            "loop": incident.current_loop,
            "reason": EscalationReason::Step.as_str(),
            "targets": step_targets(policy, step_index),
        });

        self.record(incident, TimelineKind::Escalated, detail, now)
            .await;
    }

    /// 记一条时间线，返回它的 ID（没记上时是一个新 ID），给直接通知当键。失败只记日志。
    async fn record(
        &self,
        incident: &Incident,
        kind: TimelineKind,
        detail: Value,
        now: TimestampMicros,
    ) -> String {
        let entry = TimelineEntry::new(&incident.org_id, &incident.id, now, kind, None, detail);
        let key = entry.id.to_string();

        if let Some(timeline) = &self.timeline
            && let Err(error) = timeline.append(entry).await
        {
            tracing::warn!(incident_id = %incident.id, %error, "incident timeline entry could not be written");
        }

        key
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

/// 把升级策略某一步的每个对象转成通知事件。通知 ID 由轮次、遍数、步骤和对象组成，重复调用不会重复
/// 通知；手动升级、转派时也用它立即通知，不等下一轮调度。
/// 事件压着的时候有人转派或增派了：通知已经发给他们，这一步的通知从没发过，不再去叫策略里的人。这一步已经通知
/// 过时，稳定事件 ID 本来也不会再发一次。之后换一轮（暂缓到点这些）或升到下一步，照常通知。
fn told_by_hand(handling: &IncidentHandling) -> bool {
    handling.assignees_pinned
        && handling.inhibition_lifted_by.is_some()
        && handling.dispatch_round == 0
}

pub(crate) async fn enqueue_step(
    engine: &NotifyEngine,
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

        if let Err(error) = engine.enqueue_event(dispatch).await {
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

/// 一步里通知的对象，写进时间线。
pub(crate) fn step_targets(policy: &EscalationPolicy, step_index: usize) -> Value {
    policy
        .steps
        .get(step_index)
        .map(|step| serde_json::to_value(&step.targets).unwrap_or(Value::Null))
        .unwrap_or(Value::Null)
}

/// 已确认的事件此刻该不该退回未确认：暂缓到点，或者没暂缓、确认后过了升级策略设的时长还没解决。
fn retrigger_due(
    incident: &Incident,
    policy: Option<&EscalationPolicy>,
    now: TimestampMicros,
) -> Option<EscalationReason> {
    if incident.status != IncidentStatus::Acknowledged || incident.is_merged() {
        return None;
    }

    if let Some(until) = incident.handling.snoozed_until {
        return (until.0 <= now.0).then_some(EscalationReason::SnoozeEnded);
    }

    let wait = i64::from(policy?.ack_retrigger_secs?).saturating_mul(1_000_000);
    let acknowledged = incident.acknowledged_at?;

    (acknowledged.0.saturating_add(wait) <= now.0).then_some(EscalationReason::AckTimeout)
}

/// 本节点要看的已确认事件：所属规则归本节点，没有并入别的事件。
fn owned_acknowledged_incidents(incidents: &[Incident], view: &OwnershipView) -> Vec<Incident> {
    incidents
        .iter()
        .filter(|incident| {
            incident.status == IncidentStatus::Acknowledged
                && !incident.is_merged()
                && view.owns(incident.rule_id.as_str())
        })
        .cloned()
        .collect()
}

/// 本节点要推进的事件：open 状态（acknowledged 不再升级），没有并入别的事件，且所属规则归本节点。
fn owned_open_incidents(incidents: Vec<Incident>, view: &OwnershipView) -> Vec<Incident> {
    incidents
        .into_iter()
        .filter(|incident| {
            incident.status == IncidentStatus::Open
                && !incident.is_merged()
                && view.owns(incident.rule_id.as_str())
        })
        .collect()
}

/// 从 `from` 起找首个对 `severity` 适用的 step 下标。
fn next_applicable_step(
    policy: &EscalationPolicy,
    from: usize,
    severity: Severity,
) -> Option<usize> {
    (from..policy.steps.len()).find(|&index| policy.steps[index].applies_to(severity))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use super::*;
    use crate::app::cluster::ownership::test_support::Nodes;

    fn incident(rule_id: &str, status: IncidentStatus) -> Incident {
        let at = TimestampMicros(1_000_000_000);
        Incident {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            rule_id: Id::from_string(rule_id),
            escalation_policy_id: Id::from_string("pol-1"),
            status,
            severity: Severity::Warning,
            summary: "high latency".into(),
            fingerprint: format!("fp-{rule_id}"),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: at,
            assignees: Vec::new(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: Vec::new(),
            triggering_query: None,
            created_at: at,
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        }
    }

    async fn view_of(nodes: &std::sync::Arc<Nodes>, node_id: &str) -> OwnershipView {
        WorkOwnership::new(nodes.clone(), node_id, PeerRole::Alert, Duration::ZERO)
            .view()
            .await
    }

    #[test]
    fn a_person_who_reassigned_a_held_incident_is_the_only_one_who_pages() {
        let by = Id::from_string("u1");

        // 只是放开：压着的通知该发，策略这一步照常。
        let lifted = IncidentHandling {
            inhibition_lifted_by: Some(by.clone()),
            ..IncidentHandling::default()
        };
        assert!(!told_by_hand(&lifted));

        // 没被压过的事件，有人指定了负责人：这一步的通知已经发过，照旧走稳定事件 ID。
        let pinned = IncidentHandling {
            assignees_pinned: true,
            ..IncidentHandling::default()
        };
        assert!(!told_by_hand(&pinned));

        let reassigned = IncidentHandling {
            assignees_pinned: true,
            inhibition_lifted_by: Some(by.clone()),
            ..IncidentHandling::default()
        };
        assert!(told_by_hand(&reassigned));

        // 暂缓到点这些换了一轮：要重新通知。
        let next_round = IncidentHandling {
            dispatch_round: 1,
            ..reassigned
        };
        assert!(!told_by_hand(&next_round));
    }

    #[tokio::test]
    async fn each_open_incident_is_advanced_by_exactly_one_node() {
        let nodes = Nodes::of(&[("a", PeerRole::Alert), ("b", PeerRole::Alert)]);
        let incidents: Vec<Incident> = (0..40)
            .map(|index| incident(&format!("rule-{index}"), IncidentStatus::Open))
            .collect();

        let on_a = owned_open_incidents(incidents.clone(), &view_of(&nodes, "a").await);
        let on_b = owned_open_incidents(incidents, &view_of(&nodes, "b").await);

        assert!(
            !on_a.is_empty() && !on_b.is_empty(),
            "{} + {}",
            on_a.len(),
            on_b.len()
        );
        assert_eq!(on_a.len() + on_b.len(), 40);
        assert!(on_a.iter().all(|a| on_b.iter().all(|b| a.id != b.id)));
    }

    #[tokio::test]
    async fn incidents_of_one_rule_stay_on_one_node() {
        // 同一条规则的事件与它的评估落在同一个节点上。
        let nodes = Nodes::of(&[("a", PeerRole::Alert), ("b", PeerRole::Alert)]);
        let (view_a, view_b) = (view_of(&nodes, "a").await, view_of(&nodes, "b").await);
        let twins = vec![
            incident("rule-7", IncidentStatus::Open),
            incident("rule-7", IncidentStatus::Open),
        ];

        let on_a = owned_open_incidents(twins.clone(), &view_a).len();
        let on_b = owned_open_incidents(twins, &view_b).len();
        assert!(
            (on_a, on_b) == (2, 0) || (on_a, on_b) == (0, 2),
            "{on_a} + {on_b}"
        );
    }

    #[tokio::test]
    async fn acknowledged_and_resolved_incidents_are_never_advanced() {
        let solo = view_of(&Nodes::of(&[("a", PeerRole::Alert)]), "a").await;
        let incidents = vec![
            incident("rule-1", IncidentStatus::Acknowledged),
            incident("rule-2", IncidentStatus::Resolved),
            incident("rule-3", IncidentStatus::Open),
        ];

        let advanced = owned_open_incidents(incidents, &solo);
        assert_eq!(advanced.len(), 1);
        assert_eq!(advanced[0].rule_id, Id::from_string("rule-3"));
    }

    fn policy_waiting(secs: Option<u32>) -> EscalationPolicy {
        EscalationPolicy {
            id: Id::from_string("pol-1"),
            org_id: Id::from_string("org-a"),
            name: "primary".into(),
            steps: Vec::new(),
            repeat: false,
            max_loops: 1,
            ack_retrigger_secs: secs,
            urgency_rule: Default::default(),
            escalate_low_urgency: false,
        }
    }

    fn acknowledged_at(at_secs: i64) -> Incident {
        let mut incident = incident("rule-1", IncidentStatus::Acknowledged);
        incident.acknowledged_at = Some(TimestampMicros(at_secs * 1_000_000));
        incident
    }

    #[test]
    fn a_snooze_brings_the_incident_back_at_its_end() {
        let mut incident = acknowledged_at(0);
        incident.handling.snoozed_until = Some(TimestampMicros(600 * 1_000_000));

        let policy = policy_waiting(Some(300));
        let at = |secs: i64| TimestampMicros(secs * 1_000_000);

        // 暂缓优先于确认超时：300 秒时策略的时长到了，但还在暂缓。
        assert_eq!(retrigger_due(&incident, Some(&policy), at(300)), None);
        assert_eq!(
            retrigger_due(&incident, Some(&policy), at(600)),
            Some(EscalationReason::SnoozeEnded)
        );
    }

    #[test]
    fn an_acknowledged_incident_comes_back_after_the_policy_wait() {
        let incident = acknowledged_at(100);
        let at = |secs: i64| TimestampMicros(secs * 1_000_000);

        assert_eq!(
            retrigger_due(&incident, Some(&policy_waiting(Some(1800))), at(1899)),
            None
        );
        assert_eq!(
            retrigger_due(&incident, Some(&policy_waiting(Some(1800))), at(1900)),
            Some(EscalationReason::AckTimeout)
        );

        // 策略没设、没有策略：确认后不再提醒。
        assert_eq!(
            retrigger_due(&incident, Some(&policy_waiting(None)), at(99_999)),
            None
        );
        assert_eq!(retrigger_due(&incident, None, at(99_999)), None);
    }

    #[test]
    fn open_resolved_and_merged_incidents_never_come_back() {
        let at = TimestampMicros(10_000 * 1_000_000);
        let policy = policy_waiting(Some(300));

        let open = incident("rule-1", IncidentStatus::Open);
        assert_eq!(retrigger_due(&open, Some(&policy), at), None);

        let resolved = incident("rule-1", IncidentStatus::Resolved);
        assert_eq!(retrigger_due(&resolved, Some(&policy), at), None);

        let mut merged = acknowledged_at(0);
        merged.handling.merged_into = Some(Id::from_string("target"));
        assert_eq!(retrigger_due(&merged, Some(&policy), at), None);
    }

    #[tokio::test]
    async fn merged_incidents_are_neither_advanced_nor_watched() {
        let solo = view_of(&Nodes::of(&[("a", PeerRole::Alert)]), "a").await;

        let mut merged_open = incident("rule-1", IncidentStatus::Open);
        merged_open.handling.merged_into = Some(Id::from_string("target"));
        let mut merged_acked = incident("rule-2", IncidentStatus::Acknowledged);
        merged_acked.handling.merged_into = Some(Id::from_string("target"));
        let acked = incident("rule-3", IncidentStatus::Acknowledged);

        let all = vec![merged_open, merged_acked, acked];
        assert!(owned_open_incidents(all.clone(), &solo).is_empty());

        let watched = owned_acknowledged_incidents(&all, &solo);
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0].rule_id, Id::from_string("rule-3"));
    }
}
