// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

const MINUTE: i64 = 60_000_000;

fn incident() -> Incident {
    serde_json::from_value(json!({
        "id": "inc-1",
        "orgId": "org-a",
        "ruleId": "rule-1",
        "escalationPolicyId": "policy-1",
        "status": "open",
        "severity": "critical",
        "summary": "Checkout errors",
        "fingerprint": "fp",
        "currentStep": 0,
        "currentLoop": 0,
        "currentStepStartedAt": 0,
        "assignees": [],
        "labels": {"service": "default/checkout"},
        "annotations": {},
        "traceIds": [],
        "hostIds": [],
        "affectedServices": ["checkout"],
        "triggeringQuery": null,
        "createdAt": 600 * MINUTE,
    }))
    .expect("incident")
}

fn change(service: &str, kind: ChangeKind, minute: i64) -> DeploymentEvent {
    DeploymentEvent {
        id: Id::new(),
        kind,
        namespace: "default".into(),
        service: service.into(),
        environment: "prod".into(),
        version: if kind == ChangeKind::Deployment {
            "v42".into()
        } else {
            String::new()
        },
        status: DeploymentStatus::Succeeded,
        commit: Some("abc123".into()),
        repository_url: None,
        pull_request_url: None,
        author: Some("dana".into()),
        ci_url: Some("https://ci.example.com/run/7".into()),
        description: None,
        deployed_at: TimestampMicros(minute * MINUTE),
        created_by: Id::from_string("u1"),
        created_at: TimestampMicros(minute * MINUTE),
    }
}

#[test]
fn own_and_downstream_changes_near_the_trigger_are_listed_in_order() {
    let reported = [
        change("checkout", ChangeKind::Deployment, 590),
        change("payments", ChangeKind::FeatureFlag, 585),
        // Too early, and someone else's.
        change("checkout", ChangeKind::Configuration, 500),
        change("search", ChangeKind::Deployment, 595),
    ];

    let found = select_changes(&incident(), &reported, &["default/payments".into()], None);

    assert_eq!(found.from, TimestampMicros(570 * MINUTE));
    assert_eq!(found.to, TimestampMicros(610 * MINUTE));
    assert_eq!(
        found
            .changes
            .iter()
            .map(|change| (change.kind, change.service.as_deref(), change.downstream))
            .collect::<Vec<_>>(),
        [
            ("feature_flag", Some("payments"), true),
            ("deployment", Some("checkout"), false),
        ]
    );
    assert_eq!(found.changes[1].version.as_deref(), Some("v42"));
    assert_eq!(
        found.changes[1].link.as_deref(),
        Some("https://ci.example.com/run/7")
    );
    assert_eq!(found.changes[0].version, None, "a flag names no version");
}

#[test]
fn a_rule_edited_the_day_before_is_a_change_too() {
    let mut rule: AlertRule = serde_json::from_value(json!({
        "id": "rule-1",
        "orgId": "org-a",
        "name": "Checkout errors",
        "description": "",
        "enabled": true,
        "labels": {},
        "annotations": {},
        "query": {"language": "sql", "statement": "SELECT 1", "periodSecs": 60},
        "trigger": {"operator": "gt", "threshold": 1.0, "forPeriods": 1, "silenceSecs": 0},
        "severity": "critical",
        "escalationPolicyId": "policy-1",
        "createdAt": 0,
        "updatedAt": 540 * MINUTE,
    }))
    .expect("rule");

    let found = select_changes(&incident(), &[], &[], Some(&rule));
    assert_eq!(found.changes.len(), 1);
    assert_eq!(
        (found.changes[0].kind, found.changes[0].rule_name.as_deref()),
        ("rule", Some("Checkout errors"))
    );

    rule.updated_at = TimestampMicros(0);
    assert!(
        select_changes(&incident(), &[], &[], Some(&rule))
            .changes
            .is_empty(),
        "never edited"
    );
}

#[test]
fn changes_in_a_range_keep_the_named_services_and_kinds() {
    let reported = [
        change("checkout", ChangeKind::Deployment, 590),
        change("payments", ChangeKind::FeatureFlag, 585),
        change("search", ChangeKind::Configuration, 595),
        // Outside the range.
        change("checkout", ChangeKind::Configuration, 400),
    ];

    let from = TimestampMicros(500 * MINUTE);
    let to = TimestampMicros(600 * MINUTE);

    let all = select_reported(&reported, from, to, &[], &[], 10);
    assert_eq!(
        all.iter()
            .map(|change| change.service.as_deref())
            .collect::<Vec<_>>(),
        [Some("payments"), Some("checkout"), Some("search")],
        "in time order"
    );
    assert!(all.iter().all(|change| !change.downstream));

    let named = select_reported(&reported, from, to, &["default/checkout".into()], &[], 10);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].kind, "deployment");

    let flags = select_reported(&reported, from, to, &[], &[ChangeKind::FeatureFlag], 10);
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].service.as_deref(), Some("payments"));
}

#[test]
fn a_range_with_more_changes_than_asked_keeps_the_latest() {
    let reported = [
        change("checkout", ChangeKind::Deployment, 590),
        change("checkout", ChangeKind::Deployment, 591),
        change("checkout", ChangeKind::Deployment, 592),
    ];

    let latest = select_reported(
        &reported,
        TimestampMicros(0),
        TimestampMicros(600 * MINUTE),
        &[],
        &[],
        2,
    );

    assert_eq!(
        latest.iter().map(|change| change.at).collect::<Vec<_>>(),
        [TimestampMicros(591 * MINUTE), TimestampMicros(592 * MINUTE)]
    );
}
