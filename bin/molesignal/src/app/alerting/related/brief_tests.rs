// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::{
    alerting::incident::{IncidentStatus, Severity},
    apm::DeploymentStatus,
};
use kernel::ids::Id;

use super::*;

const MINUTE: i64 = 60_000_000;
/// 2026-11-10 10:00 UTC.
const BEGAN: i64 = 1_794_304_800 * 1_000_000;

fn similar(
    summary: &str,
    resolved_after: Option<i64>,
    root_cause: Option<&str>,
) -> SimilarIncident {
    SimilarIncident {
        id: Id::new(),
        summary: summary.into(),
        severity: Severity::Critical,
        status: if resolved_after.is_some() {
            IncidentStatus::Resolved
        } else {
            IncidentStatus::Open
        },
        created_at: TimestampMicros(BEGAN - 3 * 1_440 * MINUTE),
        resolved_at: resolved_after
            .map(|minutes| TimestampMicros(BEGAN - 3 * 1_440 * MINUTE + minutes * MINUTE)),
        resolved_by: None,
        score: 5.0,
        reasons: vec!["fingerprint", "services"],
        notes: 2,
        root_cause: root_cause.map(str::to_string),
    }
}

fn deployment(service: &str, at_minute: i64, downstream: bool) -> IncidentChange {
    IncidentChange {
        kind: "deployment",
        at: TimestampMicros(BEGAN + at_minute * MINUTE),
        service: Some(service.into()),
        downstream,
        version: Some("v42".into()),
        status: Some(DeploymentStatus::Succeeded),
        commit: Some("abc123".into()),
        description: Some("Raise the pool\nto 50".into()),
        author: Some("dana".into()),
        link: None,
        rule_id: None,
        rule_name: None,
    }
}

fn changes(changes: Vec<IncidentChange>) -> IncidentChanges {
    IncidentChanges {
        from: TimestampMicros(BEGAN - BEFORE),
        to: TimestampMicros(BEGAN + 10 * MINUTE),
        changes,
    }
}

#[test]
fn an_empty_brief_says_nothing() {
    assert_eq!(IncidentBrief::default().render(), "");
}

#[test]
fn every_part_is_told_in_order() {
    let brief = IncidentBrief {
        guide: Some("Check the pool first.".into()),
        catalog: Some("checkout: the checkout API, team payments".into()),
        similar: vec![similar(
            "Checkout errors",
            Some(42),
            Some("The pool ran out."),
        )],
        changes: Some(changes(vec![deployment("checkout", -5, false)])),
        ..IncidentBrief::default()
    };

    let text = brief.render();

    let guide = text.find("Investigation guide").expect("guide");
    let catalog = text.find("service catalog").expect("catalog");
    let similar = text.find("most like it").expect("similar");
    let changed = text.find("Changes reported").expect("changes");

    assert!(
        guide < catalog && catalog < similar && similar < changed,
        "{text}"
    );
    assert!(
        text.contains("run them over the incident's time range"),
        "{text}"
    );
    assert!(text.contains("resolved after 42m"), "{text}");
    assert!(text.contains("root cause: The pool ran out."), "{text}");
    assert!(text.contains("alike in fingerprint, services"), "{text}");
    assert!(
        text.contains("2026-11-10 09:55 UTC · deployment · checkout v42 (succeeded)"),
        "{text}"
    );
    assert!(
        text.contains("Raise the pool to 50"),
        "a description stays on one line: {text}"
    );
}

#[test]
fn only_the_first_similar_incidents_are_told() {
    let brief = IncidentBrief {
        similar: (0..5)
            .map(|index| similar(&format!("Errors {index}"), None, None))
            .collect(),
        ..IncidentBrief::default()
    };

    let text = brief.render();

    assert!(text.contains("Errors 2"), "{text}");
    assert!(!text.contains("Errors 3"), "{text}");
    assert!(text.contains("still Open"), "{text}");
}

#[test]
fn a_window_without_changes_is_said_so() {
    let brief = IncidentBrief {
        changes: Some(changes(Vec::new())),
        ..IncidentBrief::default()
    };

    assert!(
        brief
            .render()
            .contains("none was reported for its services or the services they call"),
        "{}",
        brief.render()
    );
}

#[test]
fn the_changes_nearest_the_start_are_told_in_time_order() {
    let mut many: Vec<IncidentChange> = (0..14)
        .map(|index| deployment(&format!("svc-{index}"), -28 + 2 * index, index % 2 == 0))
        .collect();

    many.push(IncidentChange {
        kind: "rule",
        at: TimestampMicros(BEGAN - 60 * MINUTE),
        service: None,
        downstream: false,
        version: None,
        status: None,
        commit: None,
        description: None,
        author: None,
        link: None,
        rule_id: Some(Id::from_string("rule-1")),
        rule_name: Some("Checkout errors".into()),
    });

    let text = IncidentBrief {
        changes: Some(changes(many)),
        ..IncidentBrief::default()
    }
    .render();

    let told = text.lines().filter(|line| line.starts_with("- ")).count();

    assert_eq!(told, 10, "{text}");
    // The furthest from the start are left out: the oldest deployments and the rule edit.
    assert!(!text.contains("svc-0 "), "{text}");
    assert!(!text.contains("was edited"), "{text}");
    assert!(text.contains("svc-13"), "{text}");
    assert!(text.contains("(downstream)"), "{text}");
}

#[test]
fn durations_read_in_minutes_hours_and_days() {
    assert_eq!(duration(30_000_000), "under a minute");
    assert_eq!(duration(42 * MINUTE), "42m");
    assert_eq!(duration(125 * MINUTE), "2h 5m");
    assert_eq!(duration((2 * 1_440 + 180) * MINUTE), "2d 3h");
}

#[test]
fn a_long_guide_is_cut_to_leave_room_for_the_rest() {
    let brief = IncidentBrief {
        guide: Some("x".repeat(GUIDE_CHARS * 2)),
        similar: vec![similar("Checkout errors", Some(42), None)],
        ..IncidentBrief::default()
    };

    let text = brief.render();

    assert!(text.contains("[cut]"), "{text}");
    assert!(text.contains("Checkout errors"), "{text}");
    assert!(text.len() < GUIDE_CHARS + 1_000, "{}", text.len());
}
