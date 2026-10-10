// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一轮 [`Decision`] 落到事件生命周期：开事件、升降级、自动解决，以及 `silence_secs` 冷却。

use std::collections::BTreeMap;

use super::{
    GROUP_WINDOW_SECS, RuleEvaluator,
    decision::Decision,
    events::{emit_lifecycle, enqueue_notify_event},
    incident_context::derive_incident_context,
};
use crate::{
    app::notify::{ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT},
    domain::{
        alerting::{
            incident::{
                Incident, IncidentStatus, TriggeringQuery, generate_title,
                resolve_incident_severity,
            },
            rule::AlertRule,
        },
        query::QueryResult,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

impl RuleEvaluator {
    /// 规则正在触发。`existing` 是同指纹最新的一条事件（任何状态）。
    pub(super) async fn on_firing(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        existing: Option<Incident>,
        query_result: &QueryResult,
        fingerprint: &str,
        now: TimestampMicros,
    ) -> Result<()> {
        match existing {
            Some(incident) if incident.status == IncidentStatus::Open => {
                self.update_severity(rule, incident, decision, now).await
            }
            // 已确认的事件仍是活跃事件；同一指纹同时只允许一条活跃事件。
            Some(incident) if incident.status == IncidentStatus::Acknowledged => Ok(()),
            previous => {
                if let Some(previous) = &previous
                    && in_silence(rule.trigger.silence_secs, previous.created_at, now)
                {
                    tracing::debug!(
                        rule_id = %rule.id,
                        previous_incident_id = %previous.id,
                        silence_secs = rule.trigger.silence_secs,
                        "incident suppressed by trigger.silence_secs"
                    );
                    return Ok(());
                }

                self.open_incident(rule, decision, query_result, fingerprint, now)
                    .await
            }
        }
    }

    /// 规则不再命中：解决 open 事件（已确认的事件保持原样，由人解决）。
    pub(super) async fn on_cleared(
        &self,
        rule: &AlertRule,
        existing: Option<Incident>,
        now: TimestampMicros,
    ) -> Result<()> {
        let Some(mut incident) = existing.filter(|inc| inc.status == IncidentStatus::Open) else {
            return Ok(());
        };

        incident.status = IncidentStatus::Resolved;
        incident.resolved_at = Some(now);

        // 评估期间被用户确认或解决时写入落空，保留用户的结果。
        let Some(incident) = self
            .incidents
            .update_if_status(incident, &[IncidentStatus::Open])
            .await?
        else {
            return Ok(());
        };

        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
        enqueue_notify_event(self.notify_engine.as_ref(), &incident, ALERT_RESOLVED_EVENT).await;
        self.eval_state.reset(&rule.id).await
    }

    /// 已 open 的事件按本轮结论处理严重度升降级（指纹不含 severity，同一逻辑告警单事件）。
    async fn update_severity(
        &self,
        rule: &AlertRule,
        incident: Incident,
        decision: &Decision,
        now: TimestampMicros,
    ) -> Result<()> {
        let new_severity = resolve_incident_severity(
            decision.severity,
            rule.severity,
            &incident.labels,
            decision.deviation_ratio,
        );

        if new_severity == incident.severity {
            return Ok(());
        }

        let mut updated = incident.clone();
        updated.severity = new_severity;

        // 升级（更高档）：重置升级进度，重新从头 page；降级仅更新字段。
        if new_severity > incident.severity {
            updated.current_step = 0;
            updated.current_loop = 0;
            updated.current_step_started_at = now;
        }

        if let Some(incident) = self
            .incidents
            .update_if_status(updated, &[IncidentStatus::Open])
            .await?
        {
            emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
        }

        Ok(())
    }

    /// 创建事件：冻结规则 labels / annotations，并采样跨信号 handle。
    async fn open_incident(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        query_result: &QueryResult,
        fingerprint: &str,
        now: TimestampMicros,
    ) -> Result<()> {
        let context = derive_incident_context(rule, query_result);
        let group_labels = context.labels.clone();

        let severity = resolve_incident_severity(
            decision.severity,
            rule.severity,
            &context.labels,
            decision.deviation_ratio,
        );

        let summary = generate_title(&rule.name, &context.affected_services);

        let incident = Incident {
            id: Id::new(),
            org_id: rule.org_id.clone(),
            rule_id: rule.id.clone(),
            escalation_policy_id: rule.escalation_policy_id.clone(),
            status: IncidentStatus::Open,
            severity,
            summary,
            fingerprint: fingerprint.to_owned(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: now,
            assignees: Vec::new(),
            labels: context.labels,
            annotations: rule.annotations.clone(),
            trace_ids: context.trace_ids,
            host_ids: context.host_ids,
            affected_services: context.affected_services,
            triggering_query: Some(TriggeringQuery {
                language: rule.query.language,
                statement: rule.query.statement.clone(),
                sample_values: context.sample_values,
            }),
            created_at: now,
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
        };

        let incident = self.incidents.create(incident).await?;
        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;

        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_TRIGGERED_EVENT,
        )
        .await;

        self.assign_to_group(rule, &group_labels, fingerprint, now)
            .await;
        Ok(())
    }

    /// 把刚创建的 incident 收拢进 incident_groups（告警分组规则）：
    /// 取该 org 第一条 enabled 且 matchers 全命中的 [`SemanticGroup`]，用其 `group_by`
    /// 派生 grouping key，scope = group id（于是跨规则、group_by 取值相同的相关
    /// incident 收成一组，类似 Alertmanager group_by）；无命中则按 incident 自身
    /// fingerprint 分组、scope = rule id（原始的逐规则分组行为）。
    ///
    /// 仅 [`with_grouping`](Self::with_grouping) 接上后才执行；grouping 失败只 warn，
    /// 不影响告警 / incident 主流程。
    pub(super) async fn assign_to_group(
        &self,
        rule: &AlertRule,
        incident_labels: &BTreeMap<String, String>,
        incident_fingerprint: &str,
        now: TimestampMicros,
    ) {
        let (Some(groups), Some(semantic)) = (&self.incident_groups, &self.semantic_groups) else {
            return;
        };

        let enabled = match semantic.list_enabled(&rule.org_id).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "load semantic groups failed; skip grouping");
                return;
            }
        };

        let (scope_id, group_fp) = match enabled.iter().find(|g| g.matches(incident_labels)) {
            Some(g) => (g.id.clone(), g.group_key(incident_labels)),
            None => (rule.id.clone(), incident_fingerprint.to_string()),
        };

        if let Err(e) = groups
            .upsert_for_incident(&rule.org_id, &scope_id, &group_fp, now, GROUP_WINDOW_SECS)
            .await
        {
            tracing::warn!(rule_id = %rule.id, error = %e, "incident grouping upsert failed");
        }
    }
}

/// 距上一条同指纹事件的创建不足 `silence_secs` 时处于静默期。0 表示不静默。
fn in_silence(
    silence_secs: u32,
    previous_created_at: TimestampMicros,
    now: TimestampMicros,
) -> bool {
    let window_us = i64::from(silence_secs).saturating_mul(1_000_000);
    window_us > 0 && now.0 < previous_created_at.0.saturating_add(window_us)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_window_is_measured_from_the_previous_incident() {
        let created = TimestampMicros(1_000_000_000);
        let secs = |s: i64| TimestampMicros(created.0 + s * 1_000_000);

        assert!(!in_silence(0, created, secs(1)));
        assert!(in_silence(300, created, secs(1)));
        assert!(in_silence(300, created, secs(299)));
        assert!(!in_silence(300, created, secs(300)));
        assert!(!in_silence(300, created, secs(900)));
    }
}
