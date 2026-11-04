// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde_json::json;

use super::{available, stream_fields};
use crate::{
    domain::synthetics::{
        MonitorKind, MonitorLifecycle, MonitorState, ProbeAttempt, ProbeOutcome, ProbeStepEvidence,
        SyntheticMonitor, SyntheticResult, TimingBreakdown,
    },
    shared::{ids::Id, time::TimestampMicros},
};

fn monitor() -> SyntheticMonitor {
    SyntheticMonitor {
        id: Id::from_string("check"),
        organization_id: Id::from_string("org"),
        name: "Checkout API".into(),
        description: String::new(),
        kind: MonitorKind::Http,
        lifecycle: MonitorLifecycle::Active,
        state: MonitorState::Healthy,
        team_id: Some(Id::from_string("payments")),
        tags: vec!["checkout".into(), "tier-1".into()],
        active_revision_id: None,
        draft_revision_id: None,
        next_due_at: None,
        created_by: Id::from_string("user"),
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
        archived_at: None,
    }
}

fn result(outcome: ProbeOutcome, attempts: Vec<ProbeAttempt>) -> SyntheticResult {
    SyntheticResult {
        id: Id::from_string("result"),
        organization_id: Id::from_string("org"),
        monitor_id: Id::from_string("check"),
        monitor_revision_id: Id::from_string("revision"),
        location_id: Id::from_string("fra"),
        agent_id: Some(Id::from_string("agent")),
        task_id: Id::from_string("task"),
        is_test: false,
        result_sequence: Some(7),
        scheduled_at: TimestampMicros(1_000),
        started_at: TimestampMicros(2_000),
        finished_at: TimestampMicros(9_000),
        received_at: TimestampMicros(9_500),
        outcome,
        attempts,
        assertions: Vec::new(),
        artifacts: Vec::new(),
        secret_versions: BTreeMap::new(),
        protocol_version: 1,
        metadata: BTreeMap::from([("trace_id".into(), "4bf92f35".into())]),
    }
}

fn attempt() -> ProbeAttempt {
    ProbeAttempt {
        number: 1,
        started_at: TimestampMicros(2_000),
        finished_at: TimestampMicros(9_000),
        outcome: ProbeOutcome::Failing,
        timing: TimingBreakdown {
            dns_time: Some(1_500),
            connect_time: Some(2_000),
            tls_time: None,
            first_byte_time: Some(3_250),
            total_time: Some(7_000),
        },
        error_category: Some("timeout".into()),
        error_message: None,
        bounded_response_excerpt: None,
        metadata: BTreeMap::new(),
        evidence: vec![ProbeStepEvidence {
            step_id: Id::from_string("step"),
            name: "Request".into(),
            action: "http".into(),
            started_at: TimestampMicros(2_000),
            finished_at: TimestampMicros(9_000),
            outcome: ProbeOutcome::Failing,
            error_category: None,
            error_message: None,
            metadata: BTreeMap::from([("status".into(), "504".into())]),
        }],
    }
}

#[test]
fn a_run_is_available_unless_it_failed_and_says_nothing_unless_judged() {
    assert_eq!(available(ProbeOutcome::Healthy), Some(true));
    assert_eq!(available(ProbeOutcome::Flaky), Some(true));
    assert_eq!(available(ProbeOutcome::Degraded), Some(true));
    assert_eq!(available(ProbeOutcome::Failing), Some(false));
    assert_eq!(available(ProbeOutcome::Unknown), None);
    assert_eq!(available(ProbeOutcome::Skipped), None);
}

#[test]
fn a_run_is_a_row_of_its_check_place_and_timings() {
    let fields = stream_fields(
        &result(ProbeOutcome::Failing, vec![attempt()]),
        Some(&monitor()),
        Some("Frankfurt"),
    );

    assert_eq!(fields["check_id"], json!("check"));
    assert_eq!(fields["check_name"], json!("Checkout API"));
    assert_eq!(fields["check_kind"], json!("http"));
    assert_eq!(fields["location_name"], json!("Frankfurt"));
    assert_eq!(fields["outcome"], json!("failing"));
    assert_eq!(fields["available"], json!(false));
    assert_eq!(fields["duration_ms"], json!(7.0));
    assert_eq!(fields["first_byte_ms"], json!(3.25));
    assert_eq!(fields["status_code"], json!(504));
    assert_eq!(fields["error_category"], json!("timeout"));
    assert_eq!(fields["trace_id"], json!("4bf92f35"));
    assert_eq!(fields["team_id"], json!("payments"));
    assert_eq!(fields["tags"], json!("checkout,tier-1"));
    assert!(!fields.contains_key("tls_ms"));
}

#[test]
fn a_skipped_run_has_no_availability_or_timings() {
    let mut skipped = result(ProbeOutcome::Skipped, Vec::new());
    skipped
        .metadata
        .insert("skip_reason".into(), "no_agent_online".into());

    let fields = stream_fields(&skipped, None, None);

    assert_eq!(fields["outcome"], json!("skipped"));
    assert_eq!(fields["skip_reason"], json!("no_agent_online"));
    assert_eq!(fields["attempts"], json!(0));

    for absent in ["available", "duration_ms", "check_name", "status_code"] {
        assert!(!fields.contains_key(absent), "{absent}");
    }
}
