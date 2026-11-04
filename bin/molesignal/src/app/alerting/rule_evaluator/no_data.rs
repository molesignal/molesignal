// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 无数据事件：`no_data = alert` 的规则（或按列拆分的一组）连续一段没有值时另开一个事件，有值后
//! 自动解决。它带 `alert.condition = no_data` 标签，指纹与按阈值开的事件分开，两者可以同时开着。

use super::{
    RuleEvaluator,
    decision::Decision,
    events::{emit_lifecycle, enqueue_notify_event},
    groups::Subject,
    incident_flow::{group_title, in_silence},
};
use crate::{
    app::{alerting::event_pipelines::Processed, notify::ALERT_TRIGGERED_EVENT},
    domain::{
        alerting::{
            incident::{
                CONDITION_LABEL, Incident, IncidentStatus, NO_DATA_CONDITION, TriggeringQuery,
                resolve_incident_severity,
            },
            rule::AlertRule,
        },
        pipeline::alert::AlertEventSource,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

impl RuleEvaluator {
    /// 有值了就解决开着的无数据事件；没有值连续够了轮数就开一个。返回这一段没有值期间开的无数据
    /// 事件是否已经被人解决：解决过的这一段不再开。
    pub(super) async fn settle_no_data(
        &self,
        rule: &AlertRule,
        decision: &Decision,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<bool> {
        if decision.data_returned {
            if let Some(incident) = self
                .incidents
                .find_by_fingerprint(&rule.org_id, &subject.no_data_fingerprint)
                .await?
            {
                self.resolve_open(incident, now).await?;
            }

            return Ok(false);
        }

        if !decision.no_data_firing {
            return Ok(false);
        }

        let previous = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &subject.no_data_fingerprint)
            .await?;

        match previous {
            Some(incident)
                if matches!(
                    incident.status,
                    IncidentStatus::Open | IncidentStatus::Acknowledged
                ) =>
            {
                Ok(false)
            }
            Some(incident)
                if decision
                    .no_data_since
                    .is_some_and(|since| incident.created_at.0 >= since.0) =>
            {
                Ok(true)
            }
            previous => {
                if let Some(previous) = &previous
                    && in_silence(rule.trigger.silence_secs, previous.created_at, now)
                {
                    return Ok(false);
                }

                self.open_no_data_incident(rule, subject, now).await?;
                Ok(false)
            }
        }
    }

    /// 开无数据事件：标签是规则的标签、这一组的取值与 `alert.condition = no_data`，标题与按阈值开的
    /// 事件相同。严重度取规则的 `severity`，没有时按标签，再没有是 warning。
    async fn open_no_data_incident(
        &self,
        rule: &AlertRule,
        subject: &Subject,
        now: TimestampMicros,
    ) -> Result<()> {
        let mut labels = rule.labels.clone();
        labels.extend(subject.labels.clone());
        labels.insert(CONDITION_LABEL.into(), NO_DATA_CONDITION.into());

        let summary = if subject.labels.is_empty() {
            rule.name.clone()
        } else {
            group_title(rule, &subject.labels)
        };

        let affected_services = labels
            .get("service")
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();

        let incident = Incident {
            id: Id::new(),
            org_id: rule.org_id.clone(),
            rule_id: rule.id.clone(),
            escalation_policy_id: rule.escalation_policy_id.clone(),
            status: IncidentStatus::Open,
            severity: resolve_incident_severity(None, rule.severity, &labels, None),
            summary,
            fingerprint: subject.no_data_fingerprint.clone(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: now,
            assignees: Vec::new(),
            labels,
            annotations: rule.annotations.clone(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services,
            triggering_query: Some(TriggeringQuery {
                language: rule.query.language,
                statement: rule.query.statement.clone(),
                sample_values: Vec::new(),
            }),
            created_at: now,
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
            team_id: None,
            routing: None,
            handling: crate::domain::alerting::handling::IncidentHandling {
                urgency: rule.trigger.urgency.clone(),
                priority: rule.trigger.priority,
                ..Default::default()
            },
            revision: 0,
        };

        let Processed::Kept(incident, trail) = self
            .processed(incident, AlertEventSource::NoData, &rule.name)
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
        Ok(())
    }
}
