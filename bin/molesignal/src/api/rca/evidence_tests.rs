// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::{
    alerting::incident::{IncidentStatus, Severity, TriggeringQuery, TriggeringSample},
    query::QueryLanguage,
};
use kernel::ids::Id;

use super::*;

fn incident() -> Incident {
    Incident {
        id: Id::from_string("i1"),
        org_id: Id::from_string("o1"),
        rule_id: Id::from_string("r1"),
        escalation_policy_id: Id::from_string("e1"),
        status: IncidentStatus::Open,
        severity: Severity::Critical,
        summary: "High error rate".into(),
        fingerprint: "fp".into(),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: TimestampMicros(0),
        assignees: vec![],
        labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
        annotations: BTreeMap::new(),
        trace_ids: vec!["t-1".into()],
        host_ids: vec!["h-1".into()],
        affected_services: vec!["api".into(), "web".into()],
        triggering_query: Some(TriggeringQuery {
            language: QueryLanguage::Sql,
            statement: "SELECT count(*) FROM logs".into(),
            sample_values: vec![TriggeringSample {
                ts: TimestampMicros(1_700_000_000_000_000),
                value: 42.0,
                labels: BTreeMap::new(),
            }],
        }),
        created_at: TimestampMicros(1_700_000_000_000_000),
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}

#[test]
fn render_vars_carries_time_range_and_services() {
    let vars = render_vars(&incident(), RcaOutputLocale::ZhCn);
    assert_eq!(
        vars.get("streams").and_then(|v| v.as_str()),
        Some("api, web")
    );
    assert_eq!(vars.get("locale").and_then(|v| v.as_str()), Some("zh-cn"));
    assert_eq!(
        vars.get("output_language").and_then(|v| v.as_str()),
        Some("Simplified Chinese")
    );
    let tr = vars.get("time_range").and_then(|v| v.as_str()).unwrap();
    assert!(tr.contains("2023-11-14"), "time_range should be ISO: {tr}");
}

#[test]
fn build_evidence_includes_context_and_instruction() {
    let e = build_evidence(&incident(), "", RcaOutputLocale::EnUs);
    assert!(e.contains("High error rate"));
    assert!(e.contains("Affected services: api, web"));
    assert!(e.contains("Trace IDs: t-1"));
    assert!(e.contains("SELECT count(*) FROM logs"));
    assert!(e.contains("root-cause analysis"));
}

#[test]
fn streams_falls_back_to_na() {
    let mut i = incident();
    i.affected_services.clear();
    assert_eq!(streams_str(&i), "n/a");
}

#[test]
fn the_brief_goes_between_the_facts_and_the_instruction() {
    let brief = "Investigation guide of the alert rule:\nCheck the pool first.";
    let evidence = build_evidence(&incident(), brief, RcaOutputLocale::EnUs);

    let facts = evidence.find("Incident: High error rate").expect("facts");
    let guide = evidence.find("Check the pool first.").expect("brief");
    let instruction = evidence.find("root-cause analysis").expect("instruction");

    assert!(facts < guide && guide < instruction, "{evidence}");
}

#[test]
fn an_empty_brief_leaves_the_facts_and_the_instruction() {
    let without = build_evidence(&incident(), "  ", RcaOutputLocale::ZhCn);

    assert!(without.contains("仅依据以上证据"));
    assert!(!without.contains("\n\n\n"), "{without}");
}
