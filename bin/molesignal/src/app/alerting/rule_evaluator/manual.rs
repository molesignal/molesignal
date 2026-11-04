// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::{
    RuleEvaluator,
    evaluation::{compare_value, compute_fingerprint, first_cell_f64},
    events::{emit_lifecycle, enqueue_notify_event},
    groups::split_groups,
    round::with_no_data_value,
    window::row_limit,
};
use crate::{
    app::notify::ALERT_TRIGGERED_EVENT,
    domain::{
        alerting::{
            incident::{
                Incident, IncidentStatus, Severity, TriggeringQuery, generate_title,
                resolve_incident_severity,
            },
            rule::{AlertRule, AlertRuleKind},
        },
        query::QueryResult,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    /// 这一次没有值，规则也不把没有值当作 0（见 `trigger.no_data`）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_data: bool,
    /// 按列拆分的规则：每组的值与结论，按结果里的顺序。`matched` / `severity` 是各组合起来的，
    /// `value` 是第一个命中的组的值。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<AlertRuleGroupTest>,
    /// 组数超过上限，多出的组没有评估。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub groups_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertRuleGroupTest {
    pub labels: BTreeMap<String, String>,
    pub value: Option<f64>,
    pub matched: bool,
    pub severity: Option<Severity>,
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
        if rule.kind.without_query() {
            return Err(crate::shared::Error::invalid(format!(
                "a {} rule runs no query to test",
                rule.kind.as_str()
            )));
        }

        let stream = rule
            .query
            .stream
            .clone()
            .or_else(|| {
                rule.query
                    .apm
                    .is_some()
                    .then(crate::app::alerting::apm_source::apm_stream_hint)
            })
            .or_else(|| {
                rule.query
                    .rum
                    .is_some()
                    .then(crate::app::alerting::rum_source::rum_stream_hint)
            })
            .ok_or_else(|| crate::shared::Error::invalid("alert rule query.stream is required"))?;

        let period_us = i64::from(rule.query.period_secs).saturating_mul(1_000_000);

        let query_result = self
            .run_window_result(rule, &stream, TimestampMicros(now.0 - period_us), now)
            .await?;

        if !rule.query.group_by.is_empty() {
            return test_grouped(rule, query_result);
        }

        let value = with_no_data_value(
            rule,
            first_cell_f64(&query_result).filter(|value| !value.is_nan()),
        );

        let (matched, severity, anomaly) = if let Some(current) = value
            && rule.kind == AlertRuleKind::Anomaly
        {
            let decision = self
                .eval_anomaly(rule, &stream, current, now, period_us)
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
        } else {
            let (matched, severity) = threshold_verdict(rule, value, &rule.labels);
            (matched, severity, None)
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
            no_data: value.is_none(),
            groups: Vec::new(),
            groups_truncated: false,
        })
    }

    /// Open the incident represented by a rule without evaluating its threshold. Existing open
    /// incidents are returned unchanged, making retries idempotent. A rule split by columns gets
    /// one incident for the whole rule, which its next evaluation resolves as a vanished group.
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
            team_id: None,
            routing: None,
            handling: crate::domain::alerting::handling::IncidentHandling {
                urgency: rule.trigger.urgency.clone(),
                priority: rule.trigger.priority,
                ..Default::default()
            },
            revision: 0,
        };

        let candidate = self.routed(candidate).await;

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

        self.assign_to_group(&incident, now).await;
        Ok(incident)
    }
}

/// 阈值规则对一个值的结论：命中没有、命中的严重度。`labels` 是事件会带的标签。
fn threshold_verdict(
    rule: &AlertRule,
    value: Option<f64>,
    labels: &BTreeMap<String, String>,
) -> (bool, Option<Severity>) {
    if rule.thresholds.is_empty() {
        let matched = value.is_some_and(|value| {
            compare_value(value, &rule.trigger.operator, rule.trigger.threshold)
        });

        return (
            matched,
            matched.then(|| resolve_incident_severity(None, rule.severity, labels, None)),
        );
    }

    let severity = value.and_then(|value| {
        rule.thresholds
            .iter()
            .filter(|threshold| compare_value(value, &threshold.operator, threshold.threshold))
            .map(|threshold| threshold.severity)
            .max()
    });

    (severity.is_some(), severity)
}

/// 按列拆分的规则的试运行：每组各比各的，不碰去抖状态。
fn test_grouped(rule: &AlertRule, query_result: QueryResult) -> Result<AlertRuleTestResult> {
    let (groups, cut) = split_groups(&rule.query.group_by, &query_result)?;

    let limit = row_limit(rule);
    let groups_truncated = cut || (limit > 1 && query_result.rows.len() >= limit);

    let groups = groups
        .into_iter()
        .map(|group| {
            let mut labels = rule.labels.clone();
            labels.extend(group.labels.clone());

            let (matched, severity) = threshold_verdict(rule, group.value, &labels);

            AlertRuleGroupTest {
                labels: group.labels,
                value: group.value,
                matched,
                severity,
            }
        })
        .collect::<Vec<_>>();

    let first_matched = groups.iter().find(|group| group.matched);

    Ok(AlertRuleTestResult {
        matched: first_matched.is_some(),
        value: first_matched.and_then(|group| group.value),
        severity: groups.iter().filter_map(|group| group.severity).max(),
        columns: query_result.columns,
        rows: query_result.rows.into_iter().take(20).collect(),
        scanned_rows: query_result.scanned_rows,
        took_ms: query_result.took_ms,
        anomaly: None,
        no_data: false,
        groups,
        groups_truncated,
    })
}
