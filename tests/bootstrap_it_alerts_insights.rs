// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /alerts/insights` aggregation over seeded incidents.

mod common;

use std::collections::BTreeMap;

use molesignal::{
    domain::alerting::incident::{Incident, IncidentStatus, Severity},
    shared::{ids::Id, time::TimestampMicros},
};
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
fn mk_incident(
    org: &Id,
    rule: &str,
    svc: &str,
    sev: Severity,
    status: IncidentStatus,
    created_micros: i64,
    resolved_micros: Option<i64>,
) -> Incident {
    Incident {
        id: Id::new(),
        org_id: org.clone(),
        rule_id: Id::from_string(rule.to_string()),
        escalation_policy_id: Id::new(),
        status,
        severity: sev,
        summary: "seeded".into(),
        // unique fingerprint per incident to avoid dedup collisions
        fingerprint: Id::new().0,
        current_step: 0,
        current_loop: 0,
        current_step_started_at: TimestampMicros(created_micros),
        assignees: vec![],
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        trace_ids: vec![],
        host_ids: vec![],
        affected_services: vec![svc.to_string()],
        triggering_query: None,
        created_at: TimestampMicros(created_micros),
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: resolved_micros.map(TimestampMicros),
        resolved_by: None,
    }
}

#[tokio::test]
async fn insights_aggregate_seeded_incidents() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let org = &s.root_org_id;
    let now = TimestampMicros::now().0;
    let hour = 3_600_000_000_i64;

    // noisy: resolved 30s after creation
    let noisy = mk_incident(
        org,
        "rule-web",
        "web",
        Severity::Critical,
        IncidentStatus::Resolved,
        now - 2 * hour,
        Some(now - 2 * hour + 30_000_000),
    );
    // slow: resolved 1h after creation
    let slow = mk_incident(
        org,
        "rule-api",
        "api",
        Severity::Warning,
        IncidentStatus::Resolved,
        now - hour,
        Some(now - hour + hour),
    );
    // open: still active, web again
    let open = mk_incident(
        org,
        "rule-web",
        "web",
        Severity::Error,
        IncidentStatus::Open,
        now - hour / 2,
        None,
    );
    for inc in [noisy, slow, open] {
        s.state
            .alerting
            .service
            .incidents
            .create(inc)
            .await
            .expect("seed");
    }

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/alerts/insights?window_secs=86400",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "insights failed: {}",
        resp.status()
    );
    let v: Value = resp.json().await.unwrap();

    assert_eq!(v["total"], 3);
    assert_eq!(v["active"], 1, "one open incident");
    assert_eq!(v["closed"], 2, "two resolved incidents");
    // 1 of 2 resolved was noisy (< 60s)
    assert!(
        (v["noise_rate"].as_f64().unwrap() - 0.5).abs() < 1e-9,
        "noise_rate should be 0.5; got {}",
        v["noise_rate"]
    );
    // by_hour is always 24 buckets
    assert_eq!(v["by_hour"].as_array().unwrap().len(), 24);
    // top service is web with 2
    let top = &v["top_services"].as_array().unwrap()[0];
    assert_eq!(top["key"], "web");
    assert_eq!(top["count"], 2);
    // severity tally
    assert_eq!(v["by_severity"]["critical"], 1);
    assert_eq!(v["by_severity"]["warning"], 1);
    assert_eq!(v["by_severity"]["error"], 1);
    // mttr is the mean of 30s and 3600s = 1815s
    assert!(
        (v["mttr_secs"].as_f64().unwrap() - 1815.0).abs() < 1.0,
        "mttr_secs ~ 1815; got {}",
        v["mttr_secs"]
    );
}
