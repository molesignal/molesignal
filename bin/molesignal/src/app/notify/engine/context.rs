// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警事件只带告警的快照。存下事件之前补上模板常用、快照里却没有的：规则的说明、触发时的阈值，以及
//! 告警在 MoleSignal 里的地址。

use serde_json::{Number, Value};

use super::NotifyEngine;
use crate::{
    domain::{
        alerting::rule::{AlertRule, AlertRuleKind},
        notify::policy::NotifyEvent,
    },
    shared::ids::Id,
};

impl NotifyEngine {
    /// 给告警事件补上规则的说明、阈值与告警的地址。规则读不到（已经删了，或这是拨测的监控项）时只补地址。
    pub(super) async fn complete_alert_event(&self, event: &mut NotifyEvent) {
        if !event.event_type.starts_with("alert.") {
            return;
        }

        let rule = self.event_rule(event).await;
        complete_alert_attributes(&mut event.attributes, rule.as_ref(), &self.external_url);

        let levels = crate::app::alerting::importance::levels_of(
            self.importance.as_ref(),
            &event.organization_id,
        )
        .await;
        complete_importance(&mut event.attributes, &levels);
    }

    /// 事件所属的告警规则；不是这个组织的规则不算。
    pub(super) async fn event_rule(&self, event: &NotifyEvent) -> Option<AlertRule> {
        let rules = self.alert_rules.as_ref()?;
        let rule_id = event.attributes.get("rule_id").and_then(Value::as_str)?;

        match rules.get(&Id::from_string(rule_id)).await {
            Ok(rule) if rule.org_id == event.organization_id => Some(rule),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(%error, "no alert rule to complete a notify event with");
                None
            }
        }
    }
}

/// 把告警的地址、规则的说明与阈值写进事件的属性；事件里已经有的不改。
fn complete_alert_attributes(attributes: &mut Value, rule: Option<&AlertRule>, external_url: &str) {
    let Some(attributes) = attributes.as_object_mut() else {
        return;
    };

    let text = |key: &str| {
        attributes
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let alert_id = text("alert_id");
    let severity = text("severity");
    let base = external_url.trim().trim_end_matches('/');

    if !base.is_empty()
        && let Some(alert_id) = alert_id
    {
        attributes
            .entry("incident_url")
            .or_insert_with(|| Value::String(format!("{base}/alerts/incidents/{alert_id}")));
    }

    let Some(rule) = rule else {
        return;
    };

    if !rule.description.trim().is_empty()
        && let Some(Value::Object(snapshot)) = attributes.get_mut("rule")
        && snapshot
            .get("description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        snapshot.insert(
            "description".into(),
            Value::String(rule.description.clone()),
        );
    }

    if !attributes.contains_key("threshold")
        && let Some(threshold) = threshold_of(rule, severity.as_deref())
        && let Some(threshold) = Number::from_f64(threshold)
    {
        attributes.insert("threshold".into(), Value::Number(threshold));
    }
}

/// 紧急度这一档怎么通知（用个人的哪组步骤、免打扰时怎么算），以及优先级和紧急度的名称：通知引擎按前
/// 两项发，模板能用名称。
pub(super) fn complete_importance(
    attributes: &mut Value,
    levels: &crate::domain::alerting::importance::ImportanceLevels,
) {
    use crate::domain::alerting::{priority::Priority, urgency::Urgency};

    let Some(attributes) = attributes.as_object_mut() else {
        return;
    };

    let urgency = attributes
        .get("urgency")
        .and_then(Value::as_str)
        .and_then(Urgency::from_label)
        .unwrap_or_default();
    let level = levels.urgency(&urgency);

    attributes.insert("urgency_steps".into(), Value::from(level.steps.as_str()));
    attributes.insert(
        "urgency_quiet".into(),
        Value::from(level.quiet_hours.as_str()),
    );
    attributes
        .entry("urgency_name")
        .or_insert_with(|| Value::from(level.name.clone()));

    // 没有优先级、这一档没起名时为空，模板里不会原样留下 `{{priority_name}}`。
    let named = attributes
        .get("priority")
        .and_then(Value::as_str)
        .and_then(Priority::from_label)
        .and_then(|priority| {
            levels
                .priorities
                .iter()
                .find(|level| level.priority == priority)
        })
        .map(|level| level.name.clone())
        .unwrap_or_default();

    attributes
        .entry("priority_name")
        .or_insert_with(|| Value::from(named));
}

/// 告警触发时的阈值：分档的规则是与告警严重度相同的那一档，否则是触发条件的阈值。异常检测规则没有阈值。
fn threshold_of(rule: &AlertRule, severity: Option<&str>) -> Option<f64> {
    if rule.kind == AlertRuleKind::Anomaly {
        return None;
    }

    if rule.thresholds.is_empty() {
        return Some(rule.trigger.threshold);
    }

    rule.thresholds
        .iter()
        .find(|tier| Some(tier.severity.as_str()) == severity)
        .map(|tier| tier.threshold)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            alerting::{
                incident::Severity,
                rule::{AlertQuery, AlertTrigger, ComparisonOp, RuleState, SeverityThreshold},
            },
            query::QueryLanguage,
        },
        shared::time::TimestampMicros,
    };

    fn rule(kind: AlertRuleKind, thresholds: Vec<SeverityThreshold>) -> AlertRule {
        AlertRule {
            id: Id::from_string("rule-1"),
            org_id: Id::from_string("org-1"),
            name: "API unavailable".into(),
            description: "The API answers fewer than 99% of requests.".into(),
            enabled: true,
            kind,
            query: AlertQuery {
                language: QueryLanguage::Sql,
                statement: "SELECT count(*) FROM logs".into(),
                period_secs: 60,
                stream: None,
                apm: None,
                rum: None,
                group_by: Vec::new(),
                composite: None,
                forecast: None,
                outlier: None,
                heartbeat: None,
                delay_secs: None,
                discovery: None,
                home_cluster: None,
            },
            anomaly_params: None,
            trigger: AlertTrigger {
                operator: ComparisonOp::Gt,
                threshold: 500.0,
                for_periods: 1,
                silence_secs: 0,
                no_data: None,
                recovery: None,
                priority: None,
                urgency: None,
            },
            thresholds,
            severity: None,
            escalation_policy_id: Id::from_string("policy-1"),
            labels: Default::default(),
            annotations: Default::default(),
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn tier(severity: Severity, threshold: f64) -> SeverityThreshold {
        SeverityThreshold {
            severity,
            operator: ComparisonOp::Gt,
            threshold,
            for_periods: 1,
        }
    }

    fn snapshot() -> Value {
        json!({
            "alert_id": "incident-1",
            "rule_id": "rule-1",
            "severity": "critical",
            "rule": {"id": "rule-1", "name": "API unavailable", "description": ""}
        })
    }

    #[test]
    fn an_alert_gets_its_link_the_rule_description_and_the_threshold() {
        let mut attributes = snapshot();
        let rule = rule(AlertRuleKind::Scheduled, Vec::new());

        complete_alert_attributes(
            &mut attributes,
            Some(&rule),
            " https://molesignal.example.com/ ",
        );

        assert_eq!(
            attributes["incident_url"],
            "https://molesignal.example.com/alerts/incidents/incident-1"
        );
        assert_eq!(
            attributes["rule"]["description"],
            "The API answers fewer than 99% of requests."
        );
        assert_eq!(attributes["threshold"], 500.0);
    }

    #[test]
    fn a_tiered_rule_gives_the_threshold_of_the_alerts_severity() {
        let tiered = rule(
            AlertRuleKind::Scheduled,
            vec![
                tier(Severity::Warning, 200.0),
                tier(Severity::Critical, 800.0),
            ],
        );

        assert_eq!(threshold_of(&tiered, Some("critical")), Some(800.0));
        assert_eq!(threshold_of(&tiered, Some("warning")), Some(200.0));
        assert_eq!(threshold_of(&tiered, Some("info")), None);
        assert_eq!(
            threshold_of(&rule(AlertRuleKind::Anomaly, Vec::new()), None),
            None
        );
    }

    #[test]
    fn what_the_event_says_is_kept_and_no_site_means_no_link() {
        let mut attributes = snapshot();
        attributes["threshold"] = json!(90);
        attributes["rule"]["description"] = json!("Written by the evaluator");

        let rule = rule(AlertRuleKind::Scheduled, Vec::new());
        complete_alert_attributes(&mut attributes, Some(&rule), "");

        assert_eq!(attributes["threshold"], 90);
        assert_eq!(
            attributes["rule"]["description"],
            "Written by the evaluator"
        );
        assert!(attributes.get("incident_url").is_none());

        let mut without_rule = snapshot();
        complete_alert_attributes(&mut without_rule, None, "https://molesignal.example.com");

        assert!(without_rule.get("threshold").is_none());
        assert_eq!(without_rule["rule"]["description"], "");
    }
}
