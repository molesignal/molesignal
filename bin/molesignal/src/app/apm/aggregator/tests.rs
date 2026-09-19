// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::domain::apm::{
    APM_FACT_SCHEMA_VERSION, ApmSpanKind, InstrumentationMetadata, TransactionIdentity,
    TransactionKind,
};

fn fact(span_id: &str, kind: ApmSpanKind, duration: u64) -> ApmSpanFact {
    ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: Id::from_string("org-1"),
        service: ServiceIdentity::new(Some("shop"), Some("api"), Some("prod"), None),
        service_version: Some("v1".into()),
        service_instance_id: Some("instance-a".into()),
        instrumentation: InstrumentationMetadata {
            language: Some("rust".into()),
            sdk_name: Some("opentelemetry".into()),
            sdk_version: Some("1".into()),
        },
        trace_id: "trace-1".into(),
        span_id: span_id.into(),
        parent_span_id: (kind == ApmSpanKind::Client).then(|| "server".into()),
        event_time: TimestampMicros(61_000_000),
        duration_micros: duration,
        span_kind: kind,
        outcome: ApmOutcome::Success,
        transaction: Some(TransactionIdentity {
            name: "GET /orders/{id}".into(),
            kind: TransactionKind::Http,
        }),
        dependency: None,
        error: None,
        exception: None,
    }
}

#[test]
fn server_and_multiple_client_children_count_one_service_request() {
    let mut aggregator =
        ApmAggregator::new("owner-a".into(), HistogramSchema::v1(), 2, 2).expect("aggregator");
    aggregator
        .observe(fact("server", ApmSpanKind::Server, 20), true)
        .expect("server");
    aggregator
        .observe(fact("client-1", ApmSpanKind::Client, 10), true)
        .expect("client");
    aggregator
        .observe(fact("client-2", ApmSpanKind::Client, 15), true)
        .expect("client");
    let batch = aggregator.flush_batch(TimestampMicros(70_000_000), 100);
    let service = batch
        .snapshots
        .iter()
        .find(|snapshot| matches!(snapshot.dimension, BucketDimension::Service { .. }))
        .expect("service bucket");
    assert_eq!(service.measurements.request_count, 1);
    let transaction = batch
        .snapshots
        .iter()
        .find(|snapshot| matches!(snapshot.dimension, BucketDimension::Transaction { .. }))
        .expect("transaction bucket");
    assert_eq!(transaction.measurements.request_count, 1);
}

#[test]
fn flush_is_absolute_and_acknowledgement_clears_only_same_sequence() {
    let mut aggregator =
        ApmAggregator::new("owner-a".into(), HistogramSchema::v1(), 2, 2).expect("aggregator");
    aggregator
        .observe(fact("one", ApmSpanKind::Server, 10), false)
        .expect("observe");
    let first = aggregator.flush_batch(TimestampMicros(70_000_000), 100);
    let retry = aggregator.flush_batch(TimestampMicros(71_000_000), 100);
    assert_eq!(
        first.snapshots[0].snapshot_seq,
        retry.snapshots[0].snapshot_seq
    );
    assert_eq!(
        first.snapshots[0].measurements,
        retry.snapshots[0].measurements
    );
    aggregator.acknowledge(&first);
    assert_eq!(aggregator.pending_snapshot_count(), 0);
}

#[test]
fn exemplar_and_error_samples_are_bounded() {
    let mut aggregator =
        ApmAggregator::new("owner-a".into(), HistogramSchema::v1(), 2, 2).expect("aggregator");
    for (index, duration) in [10, 30, 20].into_iter().enumerate() {
        let mut value = fact(&format!("span-{index}"), ApmSpanKind::Server, duration);
        value.outcome = ApmOutcome::Error;
        value.error = Some(crate::domain::apm::ErrorIdentity {
            fingerprint: "group".into(),
            error_type: "Failure".into(),
            application_frame: None,
            transaction_name: None,
            overflow: false,
        });
        aggregator.observe(value, false).expect("observe");
    }
    let batch = aggregator.flush_batch(TimestampMicros(70_000_000), 100);
    assert_eq!(batch.error_samples.len(), 2);
    assert!(
        batch
            .snapshots
            .iter()
            .all(|snapshot| snapshot.measurements.exemplars.len() <= 2)
    );
}

#[test]
fn catalog_retains_multiple_versions_and_standalone_services_without_red() {
    let mut aggregator =
        ApmAggregator::new("owner-a".into(), HistogramSchema::v1(), 2, 2).expect("aggregator");
    let mut first = fact("v1", ApmSpanKind::Server, 10);
    first.service_version = Some("1.0.0".into());
    let mut second = fact("v2", ApmSpanKind::Server, 12);
    second.service_version = Some("2.0.0".into());
    let mut standalone = fact("worker", ApmSpanKind::Internal, 5);
    standalone.service = ServiceIdentity::new(Some("shop"), Some("worker"), Some("prod"), None);
    standalone.service_version = None;
    standalone.transaction = None;

    aggregator.observe(first, false).expect("v1");
    aggregator.observe(second, false).expect("v2");
    aggregator.observe(standalone, false).expect("standalone");
    let batch = aggregator.flush_batch(TimestampMicros(70_000_000), 100);

    assert_eq!(batch.versions.len(), 2);
    assert!(
        batch
            .services
            .iter()
            .any(|observation| observation.service.name == "worker")
    );
    assert!(
        batch
            .snapshots
            .iter()
            .all(|snapshot| snapshot.dimension.service().name != "worker"),
        "standalone internal work is catalogued without inventing request RED"
    );
}
