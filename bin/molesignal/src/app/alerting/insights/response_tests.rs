// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::shared::time::TimestampMicros;

const MINUTE: i64 = 60_000_000;

fn incident(fields: Value) -> Incident {
    let mut base = json!({
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
        "labels": {},
        "annotations": {},
        "traceIds": [],
        "hostIds": [],
        "affectedServices": ["checkout"],
        "triggeringQuery": null,
        "createdAt": 0,
    });

    for (key, value) in fields.as_object().expect("object") {
        base[key] = value.clone();
    }

    serde_json::from_value(base).expect("incident")
}

fn entry(kind: TimelineKind, minutes: i64, actor: Option<&str>, detail: Value) -> TimelineEntry {
    TimelineEntry::new(
        &Id::from_string("org-a"),
        &Id::from_string("inc-1"),
        TimestampMicros(minutes * MINUTE),
        kind,
        actor.map(Id::from_string),
        detail,
    )
}

#[test]
fn the_first_acknowledgement_is_the_response_and_its_step_is_known() {
    let facts = incident_facts(
        &incident(json!({
            "status": "resolved",
            "acknowledgedAt": 12 * MINUTE,
            "acknowledgedBy": "u2",
            "resolvedAt": 30 * MINUTE,
            "resolvedBy": "u2",
            "teamId": "team-pay",
        })),
        &[
            entry(
                TimelineKind::Escalated,
                0,
                None,
                json!({"step": 0, "reason": "step"}),
            ),
            entry(
                TimelineKind::Escalated,
                5,
                None,
                json!({"step": 1, "reason": "step"}),
            ),
            entry(
                TimelineKind::Acknowledged,
                8,
                Some("u1"),
                json!({"via": "sms"}),
            ),
            entry(
                TimelineKind::Acknowledged,
                12,
                Some("u2"),
                json!({"via": "web"}),
            ),
        ],
    );

    assert_eq!(
        facts.response,
        Some(FirstResponse {
            after: 8 * MINUTE,
            by: Some(Id::from_string("u1")),
            via: "sms".into(),
        })
    );
    assert_eq!(facts.resolved_after, Some(30 * MINUTE));
    assert!(facts.escalated);
    assert_eq!(facts.reached, vec![0, 1]);
    assert_eq!(facts.acknowledged_at_step, Some(1));
}

#[test]
fn an_incident_resolved_without_acknowledgement_counts_its_resolver() {
    let by_hand = incident_facts(
        &incident(json!({
            "status": "resolved",
            "resolvedAt": 20 * MINUTE,
            "resolvedBy": "u3",
        })),
        &[entry(
            TimelineKind::Resolved,
            20,
            Some("u3"),
            json!({"via": "im"}),
        )],
    );

    assert_eq!(
        by_hand.response.as_ref().map(|r| (r.after, r.via.as_str())),
        Some((20 * MINUTE, "im"))
    );
    assert!(!by_hand.escalated);
    assert!(by_hand.reached.is_empty());

    let recovered = incident_facts(
        &incident(json!({"status": "resolved", "resolvedAt": 3 * MINUTE})),
        &[],
    );

    assert_eq!(recovered.response, None);
    assert_eq!(recovered.resolved_after, Some(3 * MINUTE));
}

#[test]
fn a_summary_gives_means_ranks_steps_and_ways() {
    let fact = |after: Option<i64>,
                escalated: bool,
                reached: Vec<u32>,
                at_step: Option<u32>,
                via: &str| {
        IncidentFacts {
            response: after.map(|after| FirstResponse {
                after,
                by: Some(Id::from_string("u1")),
                via: via.into(),
            }),
            resolved_after: after.map(|after| after * 2),
            escalated,
            reached,
            acknowledged_at_step: at_step,
            team_id: None,
            services: vec!["checkout".into()],
            priority: Some("P2".into()),
        }
    };

    let summary = summarize(&[
        fact(Some(MINUTE), false, vec![0], Some(0), "web"),
        fact(Some(3 * MINUTE), false, vec![0], Some(0), "im"),
        fact(Some(10 * MINUTE), true, vec![0, 1], Some(1), "im"),
        fact(None, true, vec![0, 1], None, "web"),
    ]);

    assert_eq!(summary.incidents, 4);
    assert_eq!(summary.responded, 3);
    assert!((summary.mtta.mean_secs - 280.0).abs() < 1e-9);
    assert!((summary.mtta.p50_secs - 180.0).abs() < 1e-9);
    assert!((summary.mtta.p90_secs - 600.0).abs() < 1e-9);
    assert!((summary.escalation_rate - 0.5).abs() < 1e-9);
    assert_eq!(
        summary.steps,
        vec![
            StepRate {
                step: 0,
                reached: 4,
                acknowledged: 2
            },
            StepRate {
                step: 1,
                reached: 2,
                acknowledged: 1
            },
        ]
    );
    assert_eq!(
        summary.ack_via,
        BTreeMap::from([("im".into(), 2), ("web".into(), 1)])
    );
    assert_eq!(summarize(&[]), ResponseSummary::default());
}

#[test]
fn a_breakdown_counts_each_service_and_the_first_responder() {
    let base = incident_facts(
        &incident(json!({
            "affectedServices": ["checkout", "cart"],
            "acknowledgedAt": 2 * MINUTE,
            "acknowledgedBy": "u1",
            "teamId": "team-pay",
        })),
        &[],
    );

    let rows = breakdown(&[base.clone(), base.clone()], Dimension::Service);
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        ["cart", "checkout"]
    );
    assert_eq!(rows[0].incidents, 2);
    assert!((rows[0].mtta_secs - 120.0).abs() < 1e-9);

    let people = breakdown(std::slice::from_ref(&base), Dimension::Person);
    assert_eq!(people[0].key, "u1");

    let teams = breakdown(&[base], Dimension::Team);
    assert_eq!(teams[0].key, "team-pay");
}
