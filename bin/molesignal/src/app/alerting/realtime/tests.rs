// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::domain::{
    alerting::rule::{AlertQuery, AlertTrigger, ComparisonOp, SeverityThreshold},
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
        },
        trigger: AlertTrigger {
            operator: ComparisonOp::Gte,
            threshold,
            for_periods: 1,
            silence_secs: 0,
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

fn alerts_with(rules: Vec<AlertRule>) -> (Arc<RealtimeAlerts>, mpsc::Receiver<Command>) {
    let alerts = RealtimeAlerts::new();
    alerts.reload(rules);
    let inbox = alerts.take_inbox().expect("inbox is available once");
    (alerts, inbox)
}

fn drain(inbox: &mut mpsc::Receiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| inbox.try_recv().ok()).collect()
}

fn fired_counts(commands: &[Command]) -> Vec<u64> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Fire { count, .. } => Some(*count),
            Command::Clear { .. } => None,
        })
        .collect()
}

#[test]
fn fires_once_when_the_window_count_meets_the_trigger() {
    let (alerts, mut inbox) = alerts_with(vec![rule(
        "r1",
        "SELECT count(*) FROM app WHERE level = 'error'",
        3.0,
    )]);

    observe_and_record(&alerts, &errors(2), &[], at(0));
    observe_and_record(&alerts, &infos(10), &[], at(1));
    assert!(drain(&mut inbox).is_empty(), "two matches stay below three");

    observe_and_record(&alerts, &errors(1), &[], at(2));
    assert_eq!(fired_counts(&drain(&mut inbox)), vec![3]);

    observe_and_record(&alerts, &errors(5), &[], at(3));
    assert!(
        drain(&mut inbox).is_empty(),
        "an active trigger is not repeated"
    );
}

#[test]
fn records_the_sink_rejected_are_not_counted() {
    let (alerts, mut inbox) = alerts_with(vec![rule("r1", "SELECT count(*) FROM app", 3.0)]);
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
    assert!(drain(&mut inbox).is_empty());
    observe_and_record(&alerts, &errors(2), &[], at(1));
    assert_eq!(fired_counts(&drain(&mut inbox)), vec![3]);
}

#[test]
fn old_matches_leave_the_window_and_the_sweep_clears_the_trigger() {
    let (alerts, mut inbox) = alerts_with(vec![rule("r1", "SELECT count(*) FROM app", 3.0)]);

    observe_and_record(&alerts, &errors(3), &[], at(0));
    assert_eq!(fired_counts(&drain(&mut inbox)), vec![3]);

    alerts.sweep(at(30));
    assert!(
        drain(&mut inbox).is_empty(),
        "still inside the 60 second window"
    );

    alerts.sweep(at(61));
    let commands = drain(&mut inbox);
    assert!(matches!(commands.as_slice(), [Command::Clear { .. }]));

    // 恢复后重新累计，达到阈值会再次触发。
    observe_and_record(&alerts, &errors(3), &[], at(100));
    assert_eq!(fired_counts(&drain(&mut inbox)), vec![3]);
}

#[test]
fn reload_keeps_the_window_of_unchanged_rules_only() {
    let statement = "SELECT count(*) FROM app";
    let (alerts, mut inbox) = alerts_with(vec![rule("r1", statement, 3.0)]);
    observe_and_record(&alerts, &errors(2), &[], at(0));

    alerts.reload(vec![rule("r1", statement, 3.0)]);
    observe_and_record(&alerts, &errors(1), &[], at(1));
    assert_eq!(fired_counts(&drain(&mut inbox)), vec![3]);

    // 条件改变后窗口从零开始。
    alerts.reload(vec![rule(
        "r1",
        "SELECT count(*) FROM app WHERE level = 'error'",
        3.0,
    )]);
    observe_and_record(&alerts, &errors(2), &[], at(2));
    assert!(drain(&mut inbox).is_empty());
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
    let (alerts, mut inbox) = alerts_with(vec![tiered]);

    observe_and_record(&alerts, &errors(1), &[], at(0));
    assert!(drain(&mut inbox).is_empty());
    observe_and_record(&alerts, &errors(1), &[], at(1));
    observe_and_record(&alerts, &errors(1), &[], at(2));
    observe_and_record(&alerts, &errors(1), &[], at(3));
    let severities: Vec<_> = drain(&mut inbox)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fire { severity, .. } => Some(severity),
            Command::Clear { .. } => None,
        })
        .collect();
    assert_eq!(
        severities,
        vec![Some(Severity::Warning), Some(Severity::Critical)]
    );
}

#[test]
fn only_enabled_compilable_real_time_rules_in_the_same_stream_are_matched() {
    let mut disabled = rule("off", "SELECT count(*) FROM app", 1.0);
    disabled.enabled = false;
    let mut scheduled = rule("sched", "SELECT count(*) FROM app", 1.0);
    scheduled.kind = AlertRuleKind::Scheduled;
    let unsupported = rule("avg", "SELECT avg(latency) FROM app", 1.0);
    let (alerts, mut inbox) = alerts_with(vec![disabled, scheduled, unsupported]);

    observe_and_record(&alerts, &errors(5), &[], at(0));
    assert!(drain(&mut inbox).is_empty());

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
