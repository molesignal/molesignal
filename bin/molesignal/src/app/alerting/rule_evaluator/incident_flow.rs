// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一轮 [`Decision`] 落到事件生命周期：开事件、升降级、自动解决，以及 `silence_secs` 冷却。

use std::collections::BTreeMap;

use super::{
    GROUP_WINDOW_SECS, RuleEvaluator,
    decision::Decision,
    events::{emit_lifecycle, enqueue_notify_event},
    groups::Subject,
    incident_context::derive_incident_context,
};
use crate::{
    app::{
        alerting::{event_pipelines::Processed, rum_source, service::WRITE_ATTEMPTS},
        notify::{ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT},
    },
    domain::{
        alerting::{
            handling::EscalationReason,
            incident::{
                Incident, IncidentStatus, Severity, TriggeringQuery, generate_title,
                resolve_incident_severity,
            },
            rule::AlertRule,
        },
        pipeline::alert::AlertEventSource,
        query::QueryResult,
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

impl RuleEvaluator {
    /// 一轮结论落到 `subject` 的事件上：触发就开事件或调整严重度；满足恢复条件就解决 open 事件
    /// （命中但未到 for_periods 的 pending、在回差里的值、没有值而保持原状态的一轮都不动）；再看
    /// 无数据事件。返回这一段没有值期间开的无数据事件是否已经被人解决。
    pub(super) async fn settle(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        query_result: &QueryResult,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<bool> {
        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &subject.fingerprint)
            .await?;

        if decision.firing {
            self.on_firing(rule, decision, existing, query_result, subject, now)
                .await?;
        } else if decision.recovered {
            self.on_cleared(rule, existing, &subject.state_key, now)
                .await?;
        }

        self.settle_no_data(rule, decision, subject, now).await
    }

    /// 规则正在触发。`existing` 是同指纹最新的一条事件（任何状态）。
    async fn on_firing(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        existing: Option<Incident>,
        query_result: &QueryResult,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<()> {
        match existing {
            Some(incident) if incident.status == IncidentStatus::Open => {
                self.merge_children(rule, &incident).await;
                self.update_severity(rule, incident, decision, query_result, subject, now)
                    .await
            }
            // 已确认的事件仍是活跃事件；同一指纹同时只允许一条活跃事件。
            Some(incident) if incident.status == IncidentStatus::Acknowledged => {
                self.merge_children(rule, &incident).await;
                Ok(())
            }
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

                self.open_incident(rule, decision, query_result, subject, now)
                    .await
            }
        }
    }

    /// 规则不再命中：解决 open 事件（已确认的事件保持原样，由人解决），清零 `state_key` 那一组的
    /// 去抖状态。
    pub(super) async fn on_cleared(
        &self,
        rule: &AlertRule,
        existing: Option<Incident>,
        state_key: &str,
        now: TimestampMicros,
    ) -> Result<()> {
        let Some(incident) = existing else {
            return Ok(());
        };

        if self.resolve_open(incident, now).await? {
            self.eval_state.reset(&rule.id, state_key).await?;
        }

        Ok(())
    }

    /// 解决一个 open 事件，返回解决了没有。已确认的事件保持原样，由人解决。
    pub(super) async fn resolve_open(
        &self,
        incident: Incident,
        now: TimestampMicros,
    ) -> Result<bool> {
        self.resolve_in(incident, &[IncidentStatus::Open], now)
            .await
    }

    /// 开着或已确认的事件都解决，不记解决人，同规则自己恢复一样通知：用在事件不会再自己恢复的
    /// 时候（工单完结、监控项暂停或归档）。返回解决了没有。
    pub(super) async fn resolve_active(
        &self,
        incident: Incident,
        now: TimestampMicros,
    ) -> Result<bool> {
        self.resolve_in(
            incident,
            &[IncidentStatus::Open, IncidentStatus::Acknowledged],
            now,
        )
        .await
    }

    /// 工单完结时解决事件（连接器开了「工单完结时解决事件」），见 [`Self::resolve_active`]。
    pub async fn close_from_ticket(&self, incident_id: &Id, now: TimestampMicros) -> Result<bool> {
        let incident = self.incidents.get(incident_id).await?;
        self.resolve_active(incident, now).await
    }

    /// 事件还在 `statuses` 之一时解决它，返回解决了没有。
    async fn resolve_in(
        &self,
        incident: Incident,
        statuses: &[IncidentStatus],
        now: TimestampMicros,
    ) -> Result<bool> {
        let mut latest = incident;

        for _ in 0..WRITE_ATTEMPTS {
            if !statuses.contains(&latest.status) {
                return Ok(false);
            }

            let mut next = latest.clone();
            next.status = IncidentStatus::Resolved;
            next.resolved_at = Some(now);
            next.handling.clear_snooze();

            // 评估期间被别人改过（确认、转派、升级）时写入落空：重读，还在这些状态就再解决一次。
            let Some(incident) = self.incidents.update_if_status(next, statuses).await? else {
                latest = self.incidents.get(&latest.id).await?;
                continue;
            };

            emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
            enqueue_notify_event(self.notify_engine.as_ref(), &incident, ALERT_RESOLVED_EVENT)
                .await;
            return Ok(true);
        }

        Ok(false)
    }

    /// 已 open 的事件按本轮结论处理严重度升降级（指纹不含 severity，同一逻辑告警单事件）。严重度从
    /// 这一轮进来时的事件算起，和开事件时一样经过告警流水线。
    async fn update_severity(
        &self,
        rule: &AlertRule,
        incident: Incident,
        decision: &Decision,
        query_result: &QueryResult,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<()> {
        // 有人手动改过严重度：照人定的来。
        if incident.handling.severity_set_by.is_some() {
            return Ok(());
        }

        let incoming = incoming_incident(rule, decision, query_result, subject, now);

        let new_severity = self
            .processed_severity(&incoming, AlertEventSource::Rule, &rule.name)
            .await;

        if new_severity == incident.severity {
            return Ok(());
        }

        self.change_severity(incident, new_severity, now).await
    }

    /// 把 open 事件改成 `severity` 写回（`incident` 上别的改动一并写回）。升到更高档时重置升级进度，
    /// 换一轮从头通知；降级只改字段。写入落空（期间被别人改过）时留到下一轮。
    pub(super) async fn change_severity(
        &self,
        incident: Incident,
        severity: Severity,
        now: TimestampMicros,
    ) -> Result<()> {
        let mut updated = incident;

        if severity > updated.severity {
            updated.current_step = 0;
            updated.current_loop = 0;
            updated.current_step_started_at = now;
            updated.handling.assignees_pinned = false;
            updated
                .handling
                .next_round(EscalationReason::SeverityRaised);
        }

        updated.severity = severity;

        if let Some(incident) = self
            .incidents
            .update_if_status(updated, &[IncidentStatus::Open])
            .await?
        {
            emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
        }

        Ok(())
    }

    /// A RUM rule's incident is told by a few sessions that show the figure at its worst, found
    /// once, when the incident opens; every other rule's result is left as it is. Failing to find
    /// them never keeps an incident from opening.
    async fn with_rum_samples(
        &self,
        rule: &AlertRule,
        query_result: &QueryResult,
        now: TimestampMicros,
    ) -> Option<QueryResult> {
        let (rum, source) = rule.query.rum.as_ref().zip(self.rum.as_ref())?;

        let period = i64::from(rule.query.period_secs).saturating_mul(1_000_000);
        let range = TimeRange::new(TimestampMicros(now.0 - period), now);

        match source.samples(&rule.org_id, rum, range).await {
            Ok(samples) => Some(rum_source::with_samples(query_result.clone(), &samples)),
            Err(error) => {
                tracing::warn!(rule_id = %rule.id, %error, "RUM sessions for the incident could not be found");
                None
            }
        }
    }

    /// 创建事件：采样跨信号 handle，交给告警流水线，路由后写入。
    async fn open_incident(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        query_result: &QueryResult,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<()> {
        let told = self.with_rum_samples(rule, query_result, now).await;

        let mut incident = incoming_incident(
            rule,
            decision,
            told.as_ref().unwrap_or(query_result),
            subject,
            now,
        );

        // 评估延迟挪过窗口时，写下这一轮看的是哪一段数据。
        let delay = self.delay_micros(rule);

        if delay > 0 {
            let to = TimestampMicros(now.0.saturating_sub(delay));
            let period = i64::from(rule.query.period_secs).saturating_mul(1_000_000);

            incident.annotations.insert(
                EVALUATED_WINDOW_ANNOTATION.into(),
                format!(
                    "{}/{}",
                    rfc3339(TimestampMicros(to.0.saturating_sub(period))),
                    rfc3339(to)
                ),
            );
        }

        let Processed::Kept(incident, trail) = self
            .processed(incident, AlertEventSource::Rule, &rule.name)
            .await
        else {
            return Ok(());
        };

        let incident = self.routed(*incident).await;
        let incident = self.incidents.create(incident).await?;
        self.record_trail(&incident, trail).await;
        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;

        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_TRIGGERED_EVENT,
        )
        .await;

        self.assign_to_group(&incident, now).await;
        self.merge_children(rule, &incident).await;
        Ok(())
    }

    /// 把刚创建的 incident 收拢进 incident_groups（告警分组规则）：
    /// 取该 org 第一条 enabled 且 matchers 全命中的 [`SemanticGroup`]，用其 `group_by`
    /// 派生 grouping key，scope = group id（于是跨规则、group_by 取值相同的相关
    /// incident 收成一组，类似 Alertmanager group_by）；无命中则按 incident 自身
    /// fingerprint 分组、scope = rule id（原始的逐规则分组行为；拨测开的事件是监控项的 id）。
    ///
    /// 仅 [`with_grouping`](Self::with_grouping) 接上后才执行；grouping 失败只 warn，
    /// 不影响告警 / incident 主流程。
    pub(super) async fn assign_to_group(&self, incident: &Incident, now: TimestampMicros) {
        let (Some(groups), Some(semantic)) = (&self.incident_groups, &self.semantic_groups) else {
            return;
        };

        let rule_id = &incident.rule_id;

        let enabled = match semantic.list_enabled(&incident.org_id).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(%rule_id, error = %e, "load semantic groups failed; skip grouping");
                return;
            }
        };

        let (scope_id, group_fp) = match enabled.iter().find(|g| g.matches(&incident.labels)) {
            Some(g) => (g.id.clone(), g.group_key(&incident.labels)),
            None => (rule_id.clone(), incident.fingerprint.clone()),
        };

        if let Err(e) = groups
            .upsert_for_incident(
                &incident.org_id,
                &scope_id,
                &group_fp,
                now,
                GROUP_WINDOW_SECS,
            )
            .await
        {
            tracing::warn!(%rule_id, error = %e, "incident grouping upsert failed");
        }
    }
}

/// 评估延迟挪过窗口时，事件注解里记下这一轮看的数据区间（`开始/结束`，RFC 3339）。
pub const EVALUATED_WINDOW_ANNOTATION: &str = "evaluated_window";

fn rfc3339(at: TimestampMicros) -> String {
    at.to_datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 这一轮结论下事件进来时的样子：冻结规则 labels / annotations。按列拆分的一组，标签里再写上这一组
/// 的取值（盖过同名的规则标签），标题跟着这些取值。开事件和重定严重度都从它出发，告警流水线两次看到
/// 的是同样的事件。
fn incoming_incident(
    rule: &AlertRule,
    decision: &Decision,
    query_result: &QueryResult,
    subject: &Subject,
    now: TimestampMicros,
) -> Incident {
    let mut context = derive_incident_context(rule, query_result);

    context.labels.extend(subject.labels.clone());

    let severity = resolve_incident_severity(
        decision.severity,
        rule.severity,
        &context.labels,
        decision.deviation_ratio,
    );

    let summary = if subject.labels.is_empty() {
        generate_title(&rule.name, &context.affected_services)
    } else {
        group_title(rule, &subject.labels)
    };

    let mut annotations = rule.annotations.clone();
    annotations.extend(decision.annotations.clone());

    Incident {
        id: Id::new(),
        org_id: rule.org_id.clone(),
        rule_id: rule.id.clone(),
        escalation_policy_id: rule.escalation_policy_id.clone(),
        status: IncidentStatus::Open,
        severity,
        summary,
        fingerprint: subject.fingerprint.clone(),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: now,
        assignees: Vec::new(),
        labels: context.labels,
        annotations,
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
        team_id: None,
        routing: None,
        handling: crate::domain::alerting::handling::IncidentHandling {
            // 自动发现是启发式的：规则没说时按低紧急度，只通知第一级、不在免打扰时段打扰。
            urgency: rule.trigger.urgency.clone().or_else(|| {
                rule.query
                    .discovery
                    .is_some()
                    .then(crate::domain::alerting::urgency::Urgency::low)
            }),
            priority: rule.trigger.priority,
            ..Default::default()
        },
        revision: 0,
    }
}

/// 一组的事件标题：规则名加上这一组的取值，按规则里维度列的顺序，空值不写。
pub(super) fn group_title(rule: &AlertRule, group: &BTreeMap<String, String>) -> String {
    let values = rule
        .query
        .group_by
        .iter()
        .filter_map(|name| group.get(name))
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>();

    if values.is_empty() {
        rule.name.clone()
    } else {
        format!("{} · {}", rule.name, values.join(", "))
    }
}

/// 距上一条同指纹事件的创建不足 `silence_secs` 时处于静默期。0 表示不静默。
pub(super) fn in_silence(
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
