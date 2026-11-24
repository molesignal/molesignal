// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::alerting::incident::{IncidentStatus, Severity};
use kernel::time::TimestampMicros;
use serde_json::json;

use super::*;

fn change(kind: &'static str, service: Option<&str>, at: i64) -> IncidentChange {
    IncidentChange {
        kind,
        at: TimestampMicros(at),
        service: service.map(str::to_string),
        downstream: false,
        version: None,
        status: None,
        commit: None,
        description: None,
        author: None,
        link: None,
        rule_id: None,
        rule_name: None,
    }
}

#[test]
fn change_kinds_are_those_the_catalog_names() {
    assert_eq!(
        parse_kinds(&["deployment".into(), " feature_flag ".into(), "rule".into()]).unwrap(),
        vec![
            ChangeFeedKind::Deployment,
            ChangeFeedKind::FeatureFlag,
            ChangeFeedKind::Rule
        ]
    );
    assert!(parse_kinds(&["deploy".into()]).is_err());
    assert!(parse_kinds(&[]).unwrap().is_empty());
}

#[test]
fn arguments_outside_the_schema_are_refused() {
    assert!(parse_args::<ChangesArgs>(json!({"incidentId": "i1", "orgId": "o2"})).is_err());

    let args: ChangesArgs = parse_args(json!({
        "timeRange": {"startTime": 1, "endTime": 2},
        "services": ["checkout"],
        "kinds": ["deployment"],
        "limit": 3
    }))
    .unwrap();

    assert_eq!(args.services, ["checkout"]);
    assert_eq!(args.limit, Some(3));
    assert!(args.incident_id.is_none());
}

#[test]
fn an_incidents_changes_keep_the_named_services_kinds_and_its_rule() {
    let changes = vec![
        change("deployment", Some("default/checkout"), 1),
        change("feature_flag", Some("payments"), 2),
        change("configuration", Some("checkout"), 3),
        change("rule", None, 4),
    ];

    let kept = keep(
        changes.clone(),
        &["checkout".into()],
        &[ChangeFeedKind::Deployment],
        10,
    );

    assert_eq!(
        kept.iter().map(|change| change.kind).collect::<Vec<_>>(),
        ["deployment", "rule"]
    );

    let latest = keep(changes, &[], &[], 2);
    assert_eq!(
        latest.iter().map(|change| change.at.0).collect::<Vec<_>>(),
        [3, 4]
    );
}

#[test]
fn a_resolved_similar_incident_says_how_long_it_took() {
    let similar = SimilarIncident {
        id: Id::from_string("i2"),
        summary: "Checkout errors".into(),
        severity: Severity::Critical,
        status: IncidentStatus::Resolved,
        created_at: TimestampMicros(1_000_000),
        resolved_at: Some(TimestampMicros(61_000_000)),
        resolved_by: None,
        score: 7.0,
        reasons: vec!["fingerprint"],
        notes: 1,
        root_cause: Some("The pool ran out.".into()),
    };

    let value = similar_value(&similar);

    assert_eq!(value["duration"], json!(60_000_000));
    assert_eq!(value["rootCause"], json!("The pool ran out."));

    let open = similar_value(&SimilarIncident {
        resolved_at: None,
        status: IncidentStatus::Open,
        ..similar
    });

    assert!(open.get("duration").is_none());
}
