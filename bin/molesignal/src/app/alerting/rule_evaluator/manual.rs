// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;
use serde_json::Value;

use super::{
    RuleEvaluator,
    evaluation::{compare_value, compute_fingerprint, first_cell_f64},
    events::{emit_lifecycle, enqueue_notify_event},
};
use crate::{
    app::notify::ALERT_TRIGGERED_EVENT,
    domain::alerting::{
        incident::{
            Incident, IncidentStatus, Severity, TriggeringQuery, generate_title,
            resolve_incident_severity,
        },
        rule::{AlertRule, AlertRuleKind},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize)]
pub struct AlertRuleTestResult {
    pub matched: bool,
    pub value: Option<f64>,
    pub severity: Option<Severity>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub scanned_rows: u64,
    pub took_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anomaly: Option<AlertRuleAnomalyTest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertRuleAnomalyTest {
    pub baseline: f64,
    pub deviation: f64,
    pub score: f64,
    pub reason: String,
}

impl RuleEvaluator {
    /// Execute one rule without touching streak state, incidents, notification queues, or the
    /// rule's `last_eval_at`. This is the canonical dry-run entry point for HTTP and tools.
    pub async fn test_rule(
        &self,
        rule: &AlertRule,
        now: TimestampMicros,
    ) -> Result<AlertRuleTestResult> {
        let stream =
            rule.query.stream.clone().ok_or_else(|| {
                crate::shared::Error::invalid("alert rule query.stream is required")
            })?;
        let period_us = i64::from(rule.query.period_secs).saturating_mul(1_000_000);
        let query_result = self
            .run_window_result(rule, &stream, TimestampMicros(now.0 - period_us), now)
            .await?;
        let value = first_cell_f64(&query_result).filter(|value| !value.is_nan());
        let (matched, severity, anomaly) = if rule.kind == AlertRuleKind::Anomaly {
            let decision = self
                .eval_anomaly(rule, &stream, &query_result, now, period_us)
                .await
                .ok_or_else(|| {
                    crate::shared::Error::invalid(
                        "anomaly rule could not build a complete baseline",
                    )
                })?;
            let severity = decision.firing.then(|| {
                resolve_incident_severity(
                    None,
                    rule.severity,
                    &rule.labels,
                    Some(decision.deviation_ratio()),
                )
            });
            (
                decision.firing,
                severity,
                Some(AlertRuleAnomalyTest {
                    baseline: decision.baseline,
                    deviation: decision.deviation,
                    score: decision.score,
                    reason: decision.reason,
                }),
            )
        } else if rule.thresholds.is_empty() {
            let matched = value.is_some_and(|value| {
                compare_value(value, &rule.trigger.operator, rule.trigger.threshold)
            });
            (
                matched,
                matched.then(|| resolve_incident_severity(None, rule.severity, &rule.labels, None)),
                None,
            )
        } else {
            let severity = value.and_then(|value| {
                rule.thresholds
                    .iter()
                    .filter(|threshold| {
                        compare_value(value, &threshold.operator, threshold.threshold)
                    })
                    .map(|threshold| threshold.severity)
                    .max()
            });
            (severity.is_some(), severity, None)
        };
        Ok(AlertRuleTestResult {
            matched,
            value,
            severity,
            columns: query_result.columns,
            rows: query_result.rows.into_iter().take(20).collect(),
            scanned_rows: query_result.scanned_rows,
            took_ms: query_result.took_ms,
            anomaly,
        })
    }

    /// Open the incident represented by a rule without evaluating its threshold. Existing open
    /// incidents are returned unchanged, making retries idempotent.
    pub async fn trigger_rule(
        &self,
        rule: &AlertRule,
        actor: &Id,
        now: TimestampMicros,
    ) -> Result<Incident> {
        let fingerprint = compute_fingerprint(rule);
        if let Some(existing) = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &fingerprint)
            .await?
            && matches!(
                existing.status,
                IncidentStatus::Open | IncidentStatus::Acknowledged
            )
        {
            return Ok(existing);
        }
        let mut annotations = rule.annotations.clone();
        annotations.insert("manual_triggered_by".into(), actor.0.clone());
        let affected_services = rule
            .labels
            .get("service")
            .or_else(|| rule.labels.get("service_name"))
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        let candidate = Incident {
            id: Id::new(),
            org_id: rule.org_id.clone(),
            rule_id: rule.id.clone(),
            escalation_policy_id: rule.escalation_policy_id.clone(),
            status: IncidentStatus::Open,
            severity: resolve_incident_severity(None, rule.severity, &rule.labels, None),
            summary: generate_title(&rule.name, &affected_services),
            fingerprint: fingerprint.clone(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: now,
            assignees: Vec::new(),
            labels: rule.labels.clone(),
            annotations,
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
        };
        let (incident, created) = match self.incidents.create(candidate.clone()).await {
            Ok(incident) => (incident, true),
            Err(Error::Conflict(_)) => {
                let existing = self
                    .incidents
                    .find_by_fingerprint(&rule.org_id, &fingerprint)
                    .await?;
                match existing {
                    Some(incident)
                        if matches!(
                            incident.status,
                            IncidentStatus::Open | IncidentStatus::Acknowledged
                        ) =>
                    {
                        (incident, false)
                    }
                    _ => (self.incidents.create(candidate).await?, true),
                }
            }
            Err(error) => return Err(error),
        };
        if !created {
            return Ok(incident);
        }
        emit_lifecycle(self.lifecycle_sink.as_ref(), &incident).await;
        enqueue_notify_event(
            self.notify_engine.as_ref(),
            &incident,
            ALERT_TRIGGERED_EVENT,
        )
        .await;
        self.assign_to_group(rule, &rule.labels, &fingerprint, now)
            .await;
        Ok(incident)
    }
}
