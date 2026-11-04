// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部来源开的事件：触发就开事件或调整严重度，确认就确认，恢复就解决。来源明确报了恢复，所以已
//! 确认的事件也一起解决（规则的事件被人确认后留给人解决）。
//!
//! 一个去重键同时最多一个活跃事件：`rule_id` 是来源的 id，带标签 `alert.source = external` 与
//! `external.kind`，指纹见 [`external_fingerprint`]。按来源的升级策略升级，没关联策略的事件只按
//! 通知策略通知。

use std::collections::BTreeMap;

use super::{
    RuleEvaluator,
    events::{emit_lifecycle, enqueue_notify_event},
};
use crate::{
    app::{
        alerting::{event_pipelines::Processed, service::WRITE_ATTEMPTS},
        notify::{ALERT_ACKNOWLEDGED_EVENT, ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT},
    },
    domain::{
        alerting::{
            handling::IncidentHandling,
            incident::{
                CONDITION_LABEL, EXTERNAL_KIND_LABEL, EXTERNAL_SOURCE, Incident, IncidentStatus,
                NO_DATA_CONDITION, SOURCE_LABEL, Severity,
            },
            source::{AlertOutcome, AlertSource, ExternalAction, ExternalAlert},
            urgency::Urgency,
        },
        pipeline::alert::AlertEventSource,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一个来源里一个去重键的事件指纹，与规则、拨测的指纹分开。
pub fn external_fingerprint(source_id: &Id, dedup_key: &str) -> String {
    blake3::hash(format!("external|{source_id}|{dedup_key}").as_bytes())
        .to_hex()
        .to_string()
}

/// 一条外部告警落到事件上的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSettlement {
    pub outcome: AlertOutcome,
    /// 告警落在哪个事件上（开的、改的或原样的）；没有可动的事件时为空。
    pub incident_id: Option<Id>,
    pub reason: Option<String>,
}

impl ExternalSettlement {
    fn on(outcome: AlertOutcome, incident: &Incident) -> Self {
        Self {
            outcome,
            incident_id: Some(incident.id.clone()),
            reason: None,
        }
    }

    fn ignored(reason: &str) -> Self {
        Self {
            outcome: AlertOutcome::Ignored,
            incident_id: None,
            reason: Some(reason.to_string()),
        }
    }
}

impl RuleEvaluator {
    /// 把一条外部告警落到事件上。`latest` 是同指纹最新的一条事件（任何状态）；调用方已经确认这
    /// 条消息不是晚到的旧消息，并且同一个去重键上同时只有它在处理。
    pub async fn settle_external(
        &self,
        source: &AlertSource,
        alert: &ExternalAlert,
        latest: Option<Incident>,
        now: TimestampMicros,
    ) -> Result<ExternalSettlement> {
        match alert.action {
            ExternalAction::Trigger => self.trigger_external(source, alert, latest, now).await,
            ExternalAction::Acknowledge => self.acknowledge_external(latest, now).await,
            ExternalAction::Resolve => self.resolve_external(latest, now).await,
        }
    }

    /// 没有活跃事件就开一个；open 的事件严重度变了就改（升档从升级第一步重来）；已确认的、严重度
    /// 被人改过的不动。
    async fn trigger_external(
        &self,
        source: &AlertSource,
        alert: &ExternalAlert,
        latest: Option<Incident>,
        now: TimestampMicros,
    ) -> Result<ExternalSettlement> {
        let severity = alert.severity.unwrap_or(source.severity);

        match latest {
            // 有人手动改过严重度：照人定的来，和规则一样。
            Some(incident)
                if incident.status == IncidentStatus::Open
                    && incident.handling.severity_set_by.is_some() =>
            {
                Ok(ExternalSettlement::on(AlertOutcome::Unchanged, &incident))
            }
            Some(incident) if incident.status == IncidentStatus::Open => {
                let incoming = external_incident(source, alert, severity, now);

                let severity = self
                    .processed_severity(&incoming, AlertEventSource::External, &source.name)
                    .await;

                if incident.severity == severity {
                    return Ok(ExternalSettlement::on(AlertOutcome::Unchanged, &incident));
                }

                let settled = ExternalSettlement::on(AlertOutcome::Updated, &incident);
                self.change_severity(incident, severity, now).await?;
                Ok(settled)
            }
            Some(incident) if incident.status == IncidentStatus::Acknowledged => {
                Ok(ExternalSettlement::on(AlertOutcome::Unchanged, &incident))
            }
            _ => self.open_external(source, alert, severity, now).await,
        }
    }

    async fn open_external(
        &self,
        source: &AlertSource,
        alert: &ExternalAlert,
        severity: Severity,
        now: TimestampMicros,
    ) -> Result<ExternalSettlement> {
        let incident = external_incident(source, alert, severity, now);
        let fingerprint = incident.fingerprint.clone();

        let (incident, trail) = match self
            .processed(incident, AlertEventSource::External, &source.name)
            .await
        {
            Processed::Kept(incident, trail) => (*incident, trail),
            Processed::Dropped(reason) => return Ok(ExternalSettlement::ignored(&reason)),
        };

        let incident = self.routed(incident).await;

        let incident = match self.incidents.create(incident).await {
            Ok(incident) => incident,
            // 同一个去重键的活跃事件已经有了（例如有人刚把旧事件重新打开）：落到它上面。
            Err(Error::Conflict(_)) => {
                let existing = self
                    .incidents
                    .find_by_fingerprint(&source.org_id, &fingerprint)
                    .await?;

                return Ok(ExternalSettlement {
                    outcome: AlertOutcome::Unchanged,
                    incident_id: existing.map(|incident| incident.id),
                    reason: None,
                });
            }
            Err(error) => return Err(error),
        };

        self.record_trail(&incident, trail).await;
        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;

        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_TRIGGERED_EVENT,
        )
        .await;

        self.assign_to_group(&incident, now).await;
        Ok(ExternalSettlement::on(AlertOutcome::Opened, &incident))
    }

    /// 确认 open 的事件，确认人为空（是发送方确认的）；停下后续的升级步骤和来电。
    async fn acknowledge_external(
        &self,
        latest: Option<Incident>,
        now: TimestampMicros,
    ) -> Result<ExternalSettlement> {
        let mut latest = latest;
        let mut written = None;

        for _ in 0..WRITE_ATTEMPTS {
            let mut incident = match latest.take() {
                Some(incident) if incident.status == IncidentStatus::Open => incident,
                Some(incident) if incident.status == IncidentStatus::Acknowledged => {
                    return Ok(ExternalSettlement::on(AlertOutcome::Unchanged, &incident));
                }
                _ => {
                    return Ok(ExternalSettlement::ignored(
                        "no active incident has this dedup key",
                    ));
                }
            };

            let id = incident.id.clone();
            incident.status = IncidentStatus::Acknowledged;
            incident.acknowledged_at = Some(now);
            incident.acknowledged_by = None;
            incident.handling.clear_snooze();

            // 期间被别人改过（转派、升级）：重读再确认。
            match self
                .incidents
                .update_if_status(incident, &[IncidentStatus::Open])
                .await?
            {
                Some(incident) => {
                    written = Some(incident);
                    break;
                }
                None => latest = Some(self.incidents.get(&id).await?),
            }
        }

        let Some(incident) = written else {
            return Ok(ExternalSettlement::ignored(
                "the incident changed while it was being acknowledged",
            ));
        };

        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;

        if let Some(engine) = &self.notify_engine
            && let Err(error) = engine
                .acknowledge_alert(&incident.org_id, &incident.id, now)
                .await
        {
            tracing::warn!(
                incident_id = %incident.id,
                error = %error,
                "notify alert acknowledgement update failed"
            );
        }

        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_ACKNOWLEDGED_EVENT,
        )
        .await;

        Ok(ExternalSettlement::on(
            AlertOutcome::Acknowledged,
            &incident,
        ))
    }

    /// 解决 open 或已确认的事件，解决人为空（是发送方报的恢复）。
    async fn resolve_external(
        &self,
        latest: Option<Incident>,
        now: TimestampMicros,
    ) -> Result<ExternalSettlement> {
        let mut latest = latest;
        let mut written = None;

        for _ in 0..WRITE_ATTEMPTS {
            let mut incident = match latest.take() {
                Some(incident) if incident.is_active() => incident,
                _ => {
                    return Ok(ExternalSettlement::ignored(
                        "no active incident has this dedup key",
                    ));
                }
            };

            let id = incident.id.clone();
            let status = incident.status;
            incident.status = IncidentStatus::Resolved;
            incident.resolved_at = Some(now);
            incident.handling.clear_snooze();

            // 期间被别人改过（确认、转派、升级）：重读，还没解决就再解决一次，恢复不能丢。
            match self.incidents.update_if_status(incident, &[status]).await? {
                Some(incident) => {
                    written = Some(incident);
                    break;
                }
                None => latest = Some(self.incidents.get(&id).await?),
            }
        }

        let Some(incident) = written else {
            return Ok(ExternalSettlement::ignored(
                "the incident changed while it was being resolved",
            ));
        };

        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
        enqueue_notify_event(self.notify_engine.as_ref(), &incident, ALERT_RESOLVED_EVENT).await;
        Ok(ExternalSettlement::on(AlertOutcome::Resolved, &incident))
    }
}

/// 外部告警进来时的样子：开事件和重定严重度都从它出发，告警流水线两次看到的是同样的事件。
fn external_incident(
    source: &AlertSource,
    alert: &ExternalAlert,
    severity: Severity,
    now: TimestampMicros,
) -> Incident {
    let labels = external_labels(source, alert);

    let summary = if alert.summary.is_empty() {
        format!("{} alert", source.name)
    } else {
        alert.summary.clone()
    };

    Incident {
        id: Id::new(),
        org_id: source.org_id.clone(),
        rule_id: source.id.clone(),
        // 不关联升级策略：找不到策略的事件没有可升级的对象，只按通知策略通知。
        escalation_policy_id: source
            .escalation_policy_id
            .clone()
            .unwrap_or_else(|| Id::from_string("")),
        status: IncidentStatus::Open,
        severity,
        summary,
        fingerprint: external_fingerprint(&source.id, &alert.dedup_key),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: now,
        assignees: Vec::new(),
        affected_services: first_of(&labels, &["service.name", "service"]),
        host_ids: first_of(&labels, &["host.name", "host", "hostname"]),
        labels,
        annotations: alert.annotations.clone(),
        trace_ids: Vec::new(),
        triggering_query: None,
        created_at: now,
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        // 发送方给了紧急度就用；没报严重度的告警按高紧急度（同 PagerDuty），其余按升级策略定。
        handling: IncidentHandling {
            priority: alert.priority,
            urgency: alert
                .urgency
                .clone()
                .or_else(|| alert.severity.is_none().then(Urgency::high)),
            ..IncidentHandling::default()
        },
        revision: 0,
    }
}

/// 事件的标签：告警自己的标签，来源的附加标签补上缺的，再写上来源与类型；无数据的告警再标上。
fn external_labels(source: &AlertSource, alert: &ExternalAlert) -> BTreeMap<String, String> {
    let mut labels = alert.labels.clone();

    for (key, value) in &source.labels {
        labels.entry(key.clone()).or_insert_with(|| value.clone());
    }

    labels.insert(SOURCE_LABEL.into(), EXTERNAL_SOURCE.into());
    labels.insert(EXTERNAL_KIND_LABEL.into(), source.kind.as_str().into());

    if alert.no_data {
        labels.insert(CONDITION_LABEL.into(), NO_DATA_CONDITION.into());
    }

    labels
}

/// 第一个有值的标签，作为受影响的服务或主机。
fn first_of(labels: &BTreeMap<String, String>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| labels.get(*key))
        .find(|value| !value.trim().is_empty())
        .map(|value| vec![value.clone()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::source::AlertSourceKind;

    fn source() -> AlertSource {
        AlertSource {
            id: Id::from_string("src-1"),
            org_id: Id::from_string("org-1"),
            name: "Prometheus".into(),
            kind: AlertSourceKind::Alertmanager,
            enabled: true,
            escalation_policy_id: None,
            severity: Severity::Warning,
            labels: [
                ("env".to_string(), "prod".to_string()),
                ("team".to_string(), "core".to_string()),
            ]
            .into(),
            token_prefix: "c16jb8m5zr".into(),
            token_hash: String::new(),
            home_cluster: String::new(),
            created_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
            last_intake_at: None,
            email: Default::default(),
        }
    }

    fn alert(labels: &[(&str, &str)]) -> ExternalAlert {
        ExternalAlert {
            action: ExternalAction::Trigger,
            dedup_key: "key".into(),
            summary: "Disk full".into(),
            severity: None,
            labels: labels
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            annotations: BTreeMap::new(),
            occurred_at: None,
            no_data: false,
            members: Vec::new(),
            priority: None,
            urgency: None,
        }
    }

    #[test]
    fn fingerprints_are_per_source_and_key() {
        let a = Id::from_string("src-a");
        let b = Id::from_string("src-b");

        assert_eq!(external_fingerprint(&a, "k"), external_fingerprint(&a, "k"));
        assert_ne!(external_fingerprint(&a, "k"), external_fingerprint(&b, "k"));
        assert_ne!(external_fingerprint(&a, "k"), external_fingerprint(&a, "j"));
    }

    #[test]
    fn the_alert_labels_win_and_the_source_fills_in() {
        let mut no_data = alert(&[("team", "payments"), ("service.name", "checkout")]);
        no_data.no_data = true;

        let labels = external_labels(&source(), &no_data);

        assert_eq!(labels["team"], "payments");
        assert_eq!(labels["env"], "prod");
        assert_eq!(labels[SOURCE_LABEL], EXTERNAL_SOURCE);
        assert_eq!(labels[EXTERNAL_KIND_LABEL], "alertmanager");
        assert_eq!(labels[CONDITION_LABEL], NO_DATA_CONDITION);
        assert_eq!(
            first_of(&labels, &["service.name", "service"]),
            vec!["checkout"]
        );
        assert!(first_of(&labels, &["host.name"]).is_empty());
    }
}
