// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::{
    alerting::incident::{IncidentStatus, Severity},
    apm::{DeploymentStatus, ServiceIdentity, TransactionIdentity, TransactionKind},
};
use kernel::ids::Id;
use serde_json::json;

use super::{
    logs::{ErrorLines, told},
    *,
};
use crate::app::{
    alerting::related::{IncidentChange, IncidentChanges, SimilarIncident},
    apm::{Insight, InsightKind, InsightSeverity},
};

const MINUTE: i64 = 60_000_000;
const BEGAN: i64 = 600 * MINUTE;

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
        "labels": {},
        "annotations": {},
        "traceIds": [],
        "hostIds": [],
        "affectedServices": ["checkout"],
        "triggeringQuery": null,
        "createdAt": BEGAN,
    }))
    .expect("incident")
}

fn change(service: &str, at_minute: i64) -> IncidentChange {
    IncidentChange {
        kind: "deployment",
        at: TimestampMicros(BEGAN + at_minute * MINUTE),
        service: Some(service.into()),
        downstream: false,
        version: Some("v42".into()),
        status: Some(DeploymentStatus::Succeeded),
        commit: Some("abc123".into()),
        description: None,
        author: Some("dana".into()),
        link: None,
        rule_id: None,
        rule_name: None,
    }
}

fn insight() -> Insight {
    Insight {
        kind: InsightKind::Anomaly,
        severity: InsightSeverity::Critical,
        service: ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None),
        transaction: Some(TransactionIdentity {
            name: "POST /orders".into(),
            kind: TransactionKind::Http,
        }),
        dependency: None,
        metric: Some("p95"),
        value: 820_000.0,
        previous: Some(310_000.0),
        last_week: Some(300_000.0),
        since: Some(TimestampMicros(BEGAN - 5 * MINUTE)),
        version: None,
        previous_version: None,
        calls: None,
        p95_time: None,
        changes: Vec::new(),
    }
}

#[test]
fn the_window_runs_from_before_it_began_to_a_little_after_but_not_past_now() {
    let window = incident_window(&incident(), TimestampMicros(BEGAN + 2 * MINUTE));

    assert_eq!(window.start.0, BEGAN - BEFORE);
    assert_eq!(window.end.0, BEGAN + 2 * MINUTE);
    assert_eq!(
        incident_window(&incident(), TimestampMicros(BEGAN + 600 * MINUTE))
            .end
            .0,
        BEGAN + AFTER
    );
}

#[test]
fn its_own_services_come_before_those_they_call_each_once() {
    let downstream = ["payments", "checkout", "payments", ""].map(String::from);

    assert_eq!(
        services_of(&incident(), &downstream),
        vec!["checkout", "payments"]
    );
}

#[test]
fn an_insight_keeps_what_moved_and_its_figures() {
    let piece = apm::insight_evidence(&insight());

    assert_eq!(piece.detail.as_deref(), Some("anomaly.p95"));
    assert_eq!(piece.subject.as_deref(), Some("POST /orders"));
    assert_eq!(piece.figures.get("lastWeek"), Some(&300_000.0));
    assert_eq!(
        piece.link.and_then(|link| link.environment).as_deref(),
        Some("prod")
    );
}

#[test]
fn error_logs_are_told_for_services_with_lines_the_most_now_first() {
    let window = TimeRange::new(TimestampMicros(0), TimestampMicros(MINUTE));
    let mut by_service = BTreeMap::new();

    by_service.insert(
        "cart".to_owned(),
        ErrorLines {
            now: 3,
            last_week: 2,
            streams: ["default".to_owned()].into(),
        },
    );
    by_service.insert(
        "checkout".to_owned(),
        ErrorLines {
            now: 120,
            last_week: 4,
            streams: ["default".to_owned()].into(),
        },
    );
    by_service.insert("quiet".to_owned(), ErrorLines::default());

    let evidence = told(by_service, window);

    assert_eq!(
        evidence
            .iter()
            .map(|piece| piece.service.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("checkout"), Some("cart")]
    );
    assert_eq!(evidence[0].figures.get("now"), Some(&120.0));
}

#[test]
fn the_brief_tells_the_nearest_changes_in_time_order_then_similar_incidents() {
    let brief = IncidentBrief {
        changes: Some(IncidentChanges {
            from: TimestampMicros(BEGAN - BEFORE),
            to: TimestampMicros(BEGAN + AFTER),
            changes: (0..12)
                .map(|index| change("checkout", index - 20))
                .collect(),
        }),
        similar: vec![SimilarIncident {
            id: Id::from_string("inc-0"),
            summary: "Checkout errors".into(),
            severity: Severity::Critical,
            status: IncidentStatus::Resolved,
            created_at: TimestampMicros(BEGAN - 1_440 * MINUTE),
            resolved_at: Some(TimestampMicros(BEGAN - 1_400 * MINUTE)),
            resolved_by: None,
            score: 5.0,
            reasons: vec!["fingerprint"],
            notes: 0,
            root_cause: Some("The pool ran out.".into()),
        }],
        ..IncidentBrief::default()
    };

    let evidence = brief_evidence(&brief, TimestampMicros(BEGAN));
    let changes = evidence
        .iter()
        .filter(|piece| piece.kind == EvidenceKind::Change)
        .collect::<Vec<_>>();

    assert_eq!(changes.len(), TOLD_CHANGES);
    // The eight nearest of twelve changes 20 to 9 minutes before: from 16 minutes before on.
    assert_eq!(changes[0].at, Some(TimestampMicros(BEGAN - 16 * MINUTE)));
    assert!(
        changes[0]
            .subject
            .as_deref()
            .unwrap_or_default()
            .contains("deployment · checkout v42")
    );

    let similar = evidence.last().expect("similar");
    assert_eq!(similar.kind, EvidenceKind::SimilarIncident);
    assert_eq!(
        similar
            .link
            .as_ref()
            .and_then(|link| link.incident_id.as_deref()),
        Some("inc-0")
    );
}

#[test]
fn the_model_reads_each_piece_on_a_labelled_line_with_its_units() {
    let mut logs = RcaEvidence::new(EvidenceKind::ErrorLogs, EvidenceSource::Logs);
    logs.service = Some("checkout".into());
    logs.figures.insert("now".into(), 120.0);
    logs.figures.insert("lastWeek".into(), 4.0);

    let evidence = labelled(vec![apm::insight_evidence(&insight()), logs], Vec::new());
    let text = render_evidence(&evidence);

    assert!(text.contains("[E1] APM insight · checkout · anomaly p95 · POST /orders · now 820.0 ms, the stretch before 310.0 ms, a week before 300.0 ms"), "{text}");
    assert!(text.contains("[E2] Error-level log lines · checkout · 120 in the window, 4 in the same window a week before"), "{text}");
    assert_eq!(render_evidence(&[]), "");
}
