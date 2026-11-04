// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::domain::{
    alerting::{
        incident::Severity,
        rule::{AlertQuery, AlertTrigger, ComparisonOp, SeverityThreshold},
    },
    query::{QueryLanguage, StreamHint},
};

const SECOND: i64 = 1_000_000;

fn rule(id: &str, statement: &str, threshold: f64) -> AlertRule {
    AlertRule {
        id: Id::from_string(id),
        org_id: Id::from_string("org"),
        name: id.into(),
        description: String::new(),
        enabled: true,
        kind: AlertRuleKind::RealTime,
        query: AlertQuery {
            language: QueryLanguage::Sql,
            statement: statement.into(),
            period_secs: 60,
            stream: Some(StreamHint {
                name: "app".into(),
                stream_type: StreamType::LOGS,
            }),
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
            operator: ComparisonOp::Gte,
            threshold,
            for_periods: 1,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        },
        anomaly_params: None,
        thresholds: Vec::new(),
        severity: None,
        escalation_policy_id: Id::from_string("default"),
        labels: Default::default(),
        annotations: Default::default(),
        last_eval_at: None,
        last_state: Default::default(),
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn errors(count: usize) -> Vec<RawEvent> {
    (0..count)
        .map(|_| RawEvent {
            timestamp: TimestampMicros(0),
            fields: match json!({"level": "error"}) {
                serde_json::Value::Object(map) => map,
                _ => unreachable!(),
            },
        })
        .collect()
}

fn infos(count: usize) -> Vec<RawEvent> {
    errors(count)
        .into_iter()
        .map(|mut event| {
            event.fields.insert("level".into(), json!("info"));
            event
        })
        .collect()
}

fn observe_and_record(
    alerts: &RealtimeAlerts,
    events: &[RawEvent],
    rejected: &[IntakeError],
    now: TimestampMicros,
) {
    if let Some(pending) = alerts.observe(&Id::from_string("org"), "app", StreamType::LOGS, events)
    {
        alerts.record(pending, rejected, now);
    }
}

fn at(seconds: i64) -> TimestampMicros {
    TimestampMicros(1_000_000 * SECOND + seconds * SECOND)
}

fn alerts_with(rules: Vec<AlertRule>) -> Arc<RealtimeAlerts> {
    let alerts = RealtimeAlerts::new();
    alerts.reload(rules);
    alerts
}

/// 这条规则在本节点窗口内的命中数。
fn count(alerts: &RealtimeAlerts, id: &str, now: TimestampMicros) -> u64 {
    alerts
        .counts(&[Id::from_string(id)], now)
        .into_iter()
        .map(|(_, count)| count)
        .sum()
}

/// 已加载的这条规则的触发条件。
fn spec_of(alerts: &RealtimeAlerts, id: &str) -> spec::RealtimeSpec {
    let entries = alerts.entries.read().clone();

    entries
        .values()
        .flatten()
        .find(|entry| entry.rule.id.as_str() == id)
        .map(|entry| entry.runtime.spec.clone())
        .expect("rule is loaded")
}

/// 记录都写完以后，汇总节点每秒拿这个计数走一步，记下开了几次、解决了几次。
fn steps(alerts: &RealtimeAlerts, id: &str, moments: &[i64]) -> Vec<coordinator::Step> {
    let spec = spec_of(alerts, id);
    let mut standing = coordinator::Standing::default();

    moments
        .iter()
        .filter_map(|moment| {
            coordinator::decide(&mut standing, spec.level(count(alerts, id, at(*moment))))
        })
        .collect()
}

#[test]
fn counts_the_matches_inside_the_window() {
    let alerts = alerts_with(vec![rule(
        "r1",
        "SELECT count(*) FROM app WHERE level = 'error'",
        3.0,
    )]);

    observe_and_record(&alerts, &errors(2), &[], at(0));
    observe_and_record(&alerts, &infos(10), &[], at(1));
    assert_eq!(count(&alerts, "r1", at(1)), 2, "infos do not match");

    observe_and_record(&alerts, &errors(1), &[], at(2));
    assert_eq!(count(&alerts, "r1", at(2)), 3);

    // 60 秒的窗口：第 0 秒的两条在第 60 秒离开。
    assert_eq!(count(&alerts, "r1", at(60)), 1);
    assert_eq!(count(&alerts, "r1", at(62)), 0);
    assert!(
        alerts
            .counts(&[Id::from_string("unknown")], at(2))
            .is_empty()
    );
}

#[test]
fn records_the_sink_rejected_are_not_counted() {
    let alerts = alerts_with(vec![rule("r1", "SELECT count(*) FROM app", 3.0)]);
    let rejected = vec![
        IntakeError {
            index: 0,
            reason: "bad".into(),
        },
        IntakeError {
            index: 1,
            reason: "bad".into(),
        },
    ];

    observe_and_record(&alerts, &errors(3), &rejected, at(0));
    assert_eq!(count(&alerts, "r1", at(0)), 1);
}

#[test]
fn the_summed_count_fires_once_and_clears_when_old_matches_leave() {
    let alerts = alerts_with(vec![rule("r1", "SELECT count(*) FROM app", 3.0)]);

    observe_and_record(&alerts, &errors(3), &[], at(0));

    assert_eq!(
        steps(&alerts, "r1", &[0, 1, 30, 61]),
        vec![
            coordinator::Step::Fire { severity: None },
            coordinator::Step::Clear
        ]
    );
}

#[test]
fn reload_keeps_the_window_of_unchanged_rules_only() {
    let statement = "SELECT count(*) FROM app";
    let alerts = alerts_with(vec![rule("r1", statement, 3.0)]);
    observe_and_record(&alerts, &errors(2), &[], at(0));

    alerts.reload(vec![rule("r1", statement, 3.0)]);
    observe_and_record(&alerts, &errors(1), &[], at(1));
    assert_eq!(count(&alerts, "r1", at(1)), 3);

    // 条件改变后窗口从零开始。
    alerts.reload(vec![rule(
        "r1",
        "SELECT count(*) FROM app WHERE level = 'error'",
        3.0,
    )]);
    observe_and_record(&alerts, &errors(2), &[], at(2));
    assert_eq!(count(&alerts, "r1", at(2)), 2);
}

#[test]
fn higher_tiers_raise_the_severity_of_an_active_trigger() {
    let mut tiered = rule("r1", "SELECT count(*) FROM app", 0.0);
    tiered.thresholds = vec![
        SeverityThreshold {
            severity: Severity::Warning,
            operator: ComparisonOp::Gte,
            threshold: 2.0,
            for_periods: 1,
        },
        SeverityThreshold {
            severity: Severity::Critical,
            operator: ComparisonOp::Gte,
            threshold: 4.0,
            for_periods: 1,
        },
    ];
    let alerts = alerts_with(vec![tiered]);
    let mut standing = coordinator::Standing::default();
    let mut fired = Vec::new();

    // 记录陆续写进来，汇总节点每秒看一次。
    for (second, matches) in [(0, 1), (1, 1), (2, 0), (3, 2)] {
        if matches > 0 {
            observe_and_record(&alerts, &errors(matches), &[], at(second));
        }

        let level = spec_of(&alerts, "r1").level(count(&alerts, "r1", at(second)));
        fired.extend(coordinator::decide(&mut standing, level));
    }

    assert_eq!(
        fired,
        vec![
            coordinator::Step::Fire {
                severity: Some(Severity::Warning)
            },
            coordinator::Step::Fire {
                severity: Some(Severity::Critical)
            }
        ]
    );
}

#[test]
fn only_enabled_compilable_real_time_rules_in_the_same_stream_are_matched() {
    let mut disabled = rule("off", "SELECT count(*) FROM app", 1.0);
    disabled.enabled = false;
    let mut scheduled = rule("sched", "SELECT count(*) FROM app", 1.0);
    scheduled.kind = AlertRuleKind::Scheduled;
    let unsupported = rule("avg", "SELECT avg(latency) FROM app", 1.0);
    let alerts = alerts_with(vec![disabled, scheduled, unsupported]);

    assert!(
        alerts
            .observe(&Id::from_string("org"), "app", StreamType::LOGS, &errors(5))
            .is_none()
    );

    alerts.reload(vec![rule("on", "SELECT count(*) FROM app", 1.0)]);
    assert!(
        alerts
            .observe(
                &Id::from_string("other-org"),
                "app",
                StreamType::LOGS,
                &errors(1)
            )
            .is_none()
    );
    assert!(
        alerts
            .observe(
                &Id::from_string("org"),
                "other",
                StreamType::LOGS,
                &errors(1)
            )
            .is_none()
    );
    assert!(
        alerts
            .observe(
                &Id::from_string("org"),
                "app",
                StreamType::METRICS,
                &errors(1)
            )
            .is_none()
    );
    assert!(
        alerts
            .observe(&Id::from_string("org"), "app", StreamType::LOGS, &errors(1))
            .is_some()
    );
}
