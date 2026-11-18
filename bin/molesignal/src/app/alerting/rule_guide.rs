// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The investigation guide of an alert rule, written for a model: the steps to take, the queries
//! to run first and the dashboards to open when one of its incidents is looked into. The incident
//! context of Mole Agent and the evidence of the default root-cause analysis carry it.

use std::fmt::Write as _;

use domain::{
    alerting::rule::{AlertRule, GuideQuery},
    query::QueryLanguage,
};

/// What the rule's guide tells someone looking into one of its incidents, written for a model:
/// the steps, the queries to run first (with their language and stream), the dashboards. `None`
/// when the rule has no guide.
pub fn guide_facts(rule: &AlertRule) -> Option<String> {
    let guide = &rule.guide;

    if guide.is_empty() {
        return None;
    }

    let mut text = format!("Investigation guide of the alert rule \"{}\".", rule.name);
    let steps = guide.steps.trim();

    if !steps.is_empty() {
        text.push_str("\nSteps:\n");
        text.push_str(steps);
    }

    if !guide.queries.is_empty() {
        text.push_str("\nQueries to run first, over the incident's time range:");

        for (index, query) in guide.queries.iter().enumerate() {
            let _ = write!(
                text,
                "\n{}. {} ({}):\n{}",
                index + 1,
                query.name.trim(),
                source_of(query),
                query.statement.trim()
            );
        }
    }

    if !guide.dashboards.is_empty() {
        let ids = guide
            .dashboards
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let _ = write!(text, "\nDashboards to open (ids): {ids}");
    }

    Some(text)
}

/// What a query reads, as one phrase: its language and, for SQL, the stream.
fn source_of(query: &GuideQuery) -> String {
    match (query.language, &query.stream) {
        (QueryLanguage::Sql, Some(stream)) => {
            format!(
                "SQL on the {} stream `{}`",
                stream.stream_type.external_name(),
                stream.name
            )
        }
        (QueryLanguage::Sql, None) => "SQL".to_string(),
        (QueryLanguage::Promql, _) => "PromQL".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::{
        alerting::rule::{AlertQuery, AlertTrigger, ComparisonOp, RuleGuide, RuleState},
        query::StreamHint,
        stream::StreamType,
    };
    use kernel::{ids::Id, time::TimestampMicros};

    use super::*;

    fn rule(guide: RuleGuide) -> AlertRule {
        AlertRule {
            id: Id::from_string("rule-1"),
            org_id: Id::from_string("org-1"),
            name: "Public API 5xx rate".into(),
            description: String::new(),
            enabled: true,
            kind: Default::default(),
            query: AlertQuery {
                language: QueryLanguage::Promql,
                statement: "sum(rate(http_requests_total{code=~\"5..\"}[5m]))".into(),
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
            trigger: AlertTrigger {
                operator: ComparisonOp::Gt,
                threshold: 2.0,
                for_periods: 3,
                silence_secs: 300,
                no_data: None,
                recovery: None,
                priority: None,
                urgency: None,
            },
            anomaly_params: None,
            thresholds: Vec::new(),
            severity: None,
            escalation_policy_id: Id::from_string("default"),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            guide,
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn a_rule_without_a_guide_has_nothing_to_say() {
        assert_eq!(guide_facts(&rule(RuleGuide::default())), None);
    }

    #[test]
    fn the_steps_queries_and_dashboards_are_written_in_order() {
        let facts = guide_facts(&rule(RuleGuide {
            steps: "1. Check the gateway.\n2. Look for a deploy.".into(),
            queries: vec![
                GuideQuery {
                    name: "Errors by route".into(),
                    language: QueryLanguage::Sql,
                    stream: Some(StreamHint {
                        name: "app_logs".into(),
                        stream_type: StreamType::LOGS,
                    }),
                    statement: "SELECT route, count(*) FROM app_logs GROUP BY route".into(),
                },
                GuideQuery {
                    name: "5xx by instance".into(),
                    language: QueryLanguage::Promql,
                    stream: None,
                    statement: "sum by (instance) (rate(http_requests_total[5m]))".into(),
                },
            ],
            dashboards: vec![Id::from_string("dash-api")],
        }))
        .unwrap();

        assert!(
            facts.starts_with("Investigation guide of the alert rule \"Public API 5xx rate\".")
        );
        assert!(facts.contains("Steps:\n1. Check the gateway.\n2. Look for a deploy."));
        assert!(facts.contains(
            "1. Errors by route (SQL on the logs stream `app_logs`):\nSELECT route, count(*)"
        ));
        assert!(facts.contains("2. 5xx by instance (PromQL):\nsum by (instance)"));
        assert!(facts.ends_with("Dashboards to open (ids): dash-api"));

        let steps = facts.find("Steps:").unwrap();
        let queries = facts.find("Queries to run first").unwrap();
        let dashboards = facts.find("Dashboards to open").unwrap();
        assert!(steps < queries && queries < dashboards);
    }

    #[test]
    fn only_the_parts_a_guide_has_are_written() {
        let facts = guide_facts(&rule(RuleGuide {
            dashboards: vec![Id::from_string("dash-api")],
            ..RuleGuide::default()
        }))
        .unwrap();

        assert!(!facts.contains("Steps:"));
        assert!(!facts.contains("Queries to run first"));
        assert!(facts.contains("Dashboards to open (ids): dash-api"));
    }
}
