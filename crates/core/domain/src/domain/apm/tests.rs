// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::shared::{ids::Id, time::TimestampMicros};

#[test]
fn service_identity_uses_stable_fallbacks_and_preferred_environment() {
    let fallback = ServiceIdentity::new(None, Some("  "), None, None);
    assert_eq!(fallback.namespace, DEFAULT_SERVICE_NAMESPACE);
    assert_eq!(fallback.name, DEFAULT_SERVICE_NAME);
    assert_eq!(fallback.environment, DEFAULT_DEPLOYMENT_ENVIRONMENT);

    let preferred = ServiceIdentity::new(
        Some(" shop "),
        Some(" checkout "),
        Some(" prod "),
        Some("legacy"),
    );
    assert_eq!(
        preferred,
        ServiceIdentity {
            namespace: "shop".into(),
            name: "checkout".into(),
            environment: "prod".into(),
        }
    );
}

#[test]
fn protocol_error_classification_matches_apm_contract() {
    assert_eq!(
        ProtocolStatus {
            otel_status: "ERROR".into(),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            http_status_code: Some(503),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            rpc_status_code: Some(7),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            http_status_code: Some(404),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Success
    );
    assert_eq!(ProtocolStatus::default().outcome(), ApmOutcome::Unknown);
}

#[test]
fn persisted_owner_snapshot_json_is_backward_compatible() {
    let fixture = r#"{
      "org_id":"org-1",
      "owner_id":"node-a",
      "bucket_at":1000000,
      "snapshot_seq":7,
      "dimension":{
        "kind":"transaction",
        "service":{"namespace":"shop","name":"checkout","environment":"prod"},
        "version":"2.5.0",
        "transaction":{"name":"POST /checkout","kind":"http"}
      },
      "measurements":{
        "request_count":2,
        "error_count":1,
        "latency":{
          "schema_version":1,
          "counts":[0,2],
          "sum_micros":3000,
          "min_micros":1000,
          "max_micros":2000
        },
        "exemplars":[]
      },
      "updated_at":2000000
    }"#;
    let snapshot: OwnerSnapshot = serde_json::from_str(fixture).expect("v1 fixture");
    assert_eq!(snapshot.schema_version, APM_PERSISTENCE_SCHEMA_VERSION);
    assert_eq!(snapshot.org_id, Id::from_string("org-1"));
    assert_eq!(snapshot.measurements.overflow_count, 0);
    assert!(matches!(
        snapshot.dimension,
        BucketDimension::Transaction { .. }
    ));

    let value = serde_json::to_value(snapshot).expect("serialize snapshot");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["dimension"]["kind"], "transaction");
    assert_eq!(value["updated_at"], TimestampMicros(2_000_000).0);
}

#[test]
fn span_kind_controls_service_and_dependency_contributions() {
    let mut fact = ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: Id::from_string("org-1"),
        service: ServiceIdentity::new(None, Some("api"), None, None),
        service_version: None,
        service_instance_id: None,
        instrumentation: InstrumentationMetadata::default(),
        trace_id: "trace".into(),
        span_id: "span".into(),
        parent_span_id: Some("parent".into()),
        event_time: TimestampMicros(1),
        duration_micros: 10,
        span_kind: ApmSpanKind::Client,
        outcome: ApmOutcome::Success,
        transaction: None,
        dependency: None,
        error: None,
        exception: None,
    };
    assert!(!fact.contributes_service_red());
    assert!(fact.contributes_dependency());

    fact.span_kind = ApmSpanKind::Unspecified;
    fact.parent_span_id = None;
    assert!(fact.contributes_service_red());
    assert!(!fact.contributes_dependency());
}

#[test]
fn merged_histogram_quantiles_use_all_bucket_counts() {
    let schema = HistogramSchema::v1();
    let mut first = LatencyHistogram::empty(&schema);
    for _ in 0..99 {
        first.observe(&schema, 1_000).expect("observe fast");
    }
    first.observe(&schema, 60_000_000).expect("observe slow");

    let mut second = LatencyHistogram::empty(&schema);
    for _ in 0..100 {
        second.observe(&schema, 2_000).expect("observe second");
    }

    // Taking max(P95(first), P95(second)) would report 2ms. Merging the
    // bucket populations first correctly reports 2ms for P50/P95/P99 because
    // only 99 of 200 observations are at or below 1ms, while still retaining
    // the 60s maximum.
    first.merge(&second).expect("compatible histogram merge");
    assert_eq!(first.count(), 200);
    assert_eq!(first.quantile(&schema, 0.50).expect("p50"), Some(2_000));
    assert_eq!(first.quantile(&schema, 0.95).expect("p95"), Some(2_000));
    assert_eq!(first.quantile(&schema, 0.99).expect("p99"), Some(2_000));
    assert_eq!(first.max_micros, Some(60_000_000));
}

#[test]
fn histogram_merge_rejects_schema_mismatch() {
    let schema = HistogramSchema::v1();
    let mut left = LatencyHistogram::empty(&schema);
    let mut right = LatencyHistogram::empty(&schema);
    right.schema_version = right.schema_version.saturating_add(1);
    assert!(left.merge(&right).is_err());
}
