// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::shared::{
    self_telemetry::ResourceIdentity,
    trace_normalization::{CanonicalEvent, CanonicalSpan, FinishedSpan, finished_span_to_event},
};

fn span(kind: i32) -> CanonicalSpan {
    CanonicalSpan::new(
        "trace-1".into(),
        "span-1".into(),
        "fallback".into(),
        kind,
        1_000_000,
        3_000_000,
    )
}

#[test]
fn extracts_resource_transaction_status_and_instrumentation() {
    let mut span = span(2);
    span.resource
        .attributes
        .insert("service.namespace".into(), json!("shop"));
    span.resource
        .attributes
        .insert("service.name".into(), json!("checkout"));
    span.resource
        .attributes
        .insert("deployment.environment.name".into(), json!("production"));
    span.resource
        .attributes
        .insert("service.version".into(), json!("2.3.0"));
    span.resource
        .attributes
        .insert("telemetry.sdk.language".into(), json!("rust"));
    span.attributes
        .insert("http.request.method".into(), json!("POST"));
    span.attributes
        .insert("http.route".into(), json!("/orders/{order_id}"));
    span.attributes
        .insert("http.response.status_code".into(), json!(503));

    let fact = extract_apm_fact("org-1", &span).expect("fact");
    assert_eq!(fact.service.name, "checkout");
    assert_eq!(fact.service.environment, "production");
    assert_eq!(fact.service_version.as_deref(), Some("2.3.0"));
    assert_eq!(fact.instrumentation.language.as_deref(), Some("rust"));
    assert_eq!(fact.outcome, ApmOutcome::Error);
    assert_eq!(
        fact.transaction.as_ref().map(|value| value.name.as_str()),
        Some("POST /orders/{order_id}")
    );
    assert!(fact.dependency.is_none());
}

#[test]
fn self_telemetry_projects_environment_and_instrumentation() {
    let resource =
        ResourceIdentity::new("molesignal", "0.1.0", "production", "standalone", "node-1");
    let mut fields = serde_json::Map::new();
    resource.enrich(&mut fields);
    let event = finished_span_to_event(
        fields,
        FinishedSpan {
            name: "http.server".into(),
            trace_id: Some("0123456789abcdef0123456789abcdef".into()),
            span_id: Some("0123456789abcdef".into()),
            parent_span_id: None,
            kind: 2,
            start_time_unix_nano: 1_000,
            end_time_unix_nano: 2_000,
            status_code: "OK".into(),
            status_message: None,
        },
    );
    let span = CanonicalSpan::try_from_raw_event(&event).expect("canonical self span");
    let fact = extract_apm_fact("org-1", &span).expect("self telemetry APM fact");

    assert_eq!(fact.service.namespace, "molesignal");
    assert_eq!(fact.service.name, "molesignal");
    assert_eq!(fact.service.environment, "production");
    assert_eq!(fact.service_version.as_deref(), Some("0.1.0"));
    assert_eq!(fact.instrumentation.language.as_deref(), Some("rust"));
    assert_eq!(fact.instrumentation.sdk_name.as_deref(), Some("molesignal"));
    assert_eq!(fact.instrumentation.sdk_version.as_deref(), Some("0.1.0"));
}

#[test]
fn classifies_database_dependency_without_sql_or_raw_namespace() {
    let mut span = span(3);
    span.attributes
        .insert("db.system.name".into(), json!("postgresql"));
    span.attributes
        .insert("db.namespace".into(), json!("customer-private"));
    span.attributes
        .insert("db.operation.name".into(), json!("SELECT"));
    span.attributes.insert(
        "db.query.text".into(),
        json!("SELECT email FROM users WHERE id = 42"),
    );

    let fact = extract_apm_fact("org-1", &span).expect("fact");
    let dependency = fact.dependency.as_ref().expect("dependency");
    assert_eq!(dependency.category, DependencyCategory::Database);
    assert!(dependency.target.starts_with("postgresql"));
    assert!(!dependency.target.contains("customer-private"));
    assert_eq!(dependency.operation.as_deref(), Some("SELECT"));
    let serialized = serde_json::to_string(&fact).expect("serialize");
    assert!(!serialized.contains("SELECT email"));
    assert!(!serialized.contains("users WHERE"));
}

#[test]
fn exception_fingerprint_excludes_message_and_masks_volatile_values() {
    let mut first = span(2);
    first.status_code = "ERROR".into();
    first.events.push(CanonicalEvent {
        time_unix_nano: 2_000_000,
        name: "exception".into(),
        attributes: BTreeMap::from([
            ("exception.type".into(), json!("CheckoutFailure")),
            (
                "exception.message".into(),
                json!("order 123456 could not be completed"),
            ),
            (
                "exception.stacktrace".into(),
                json!("checkout::submit\nruntime::poll"),
            ),
        ]),
        dropped_attributes_count: 0,
    });
    let mut second = first.clone();
    second.events[0].attributes.insert(
        "exception.message".into(),
        json!("order 999999 could not be completed"),
    );
    let first = extract_apm_fact("org-1", &first).expect("first");
    let second = extract_apm_fact("org-1", &second).expect("second");
    assert_eq!(
        first.error.as_ref().map(|value| &value.fingerprint),
        second.error.as_ref().map(|value| &value.fingerprint)
    );
    assert_eq!(
        first.exception.and_then(|value| value.message),
        Some("order # could not be completed".into())
    );
}

#[test]
fn forbidden_raw_values_never_reach_fact() {
    let mut span = span(2);
    span.attributes
        .insert("http.request.method".into(), json!("GET"));
    span.attributes.insert(
        "url.full".into(),
        json!("https://example.test/users/alice?token=secret"),
    );
    span.attributes.insert(
        "http.request.header.authorization".into(),
        json!("Bearer abc"),
    );
    span.attributes
        .insert("http.route".into(), json!("/users/{id}"));
    span.attributes
        .insert("db.statement".into(), json!("SELECT password FROM users"));
    let fact = extract_apm_fact("org-1", &span).expect("fact");
    let serialized = serde_json::to_string(&fact).expect("serialize");
    for forbidden in [
        "example.test",
        "alice",
        "secret",
        "Bearer",
        "SELECT password",
    ] {
        assert!(!serialized.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn masked_values_do_not_reach_fact_or_persistence_snapshot() {
    let mut span = span(2);
    span.resource
        .attributes
        .insert("service.name".into(), json!("[REDACTED]"));
    span.resource
        .attributes
        .insert("deployment.environment.name".into(), json!("[REDACTED]"));
    span.attributes
        .insert("http.request.method".into(), json!("GET"));
    span.attributes
        .insert("http.route".into(), json!("/safe/{id}"));
    span.attributes.insert(
        "http.request.header.cookie".into(),
        json!("session=private"),
    );
    crate::shared::trace_normalization::sanitize_and_limit_span(
        &mut span,
        crate::shared::trace_normalization::TraceLimits::default(),
    );
    let fact = extract_apm_fact("org-1", &span).expect("fact");
    assert_eq!(fact.service.name, crate::domain::apm::DEFAULT_SERVICE_NAME);
    assert_eq!(
        fact.service.environment,
        crate::domain::apm::DEFAULT_DEPLOYMENT_ENVIRONMENT
    );
    let mut aggregator = crate::app::apm::ApmAggregator::new(
        "owner".into(),
        crate::domain::apm::HistogramSchema::v1(),
        1,
        1,
    )
    .expect("aggregator");
    aggregator.observe(fact, false).expect("observe");
    let persisted = serde_json::to_string(
        &aggregator.flush_batch(crate::shared::time::TimestampMicros::now(), 10),
    )
    .expect("serialize persistence batch");
    assert!(!persisted.contains("session=private"));
    assert!(!persisted.contains("[REDACTED]"));
}

#[test]
fn span_kind_and_low_cardinality_fallback_are_bounded() {
    let root = extract_apm_fact("org-1", &span(0)).expect("root");
    assert_eq!(root.span_kind, ApmSpanKind::Unspecified);
    assert_eq!(
        root.transaction.as_ref().map(|value| value.name.as_str()),
        Some("fallback")
    );

    let mut unsafe_name = span(2);
    unsafe_name.name = "GET /users/123456".into();
    assert!(
        extract_apm_fact("org-1", &unsafe_name)
            .expect("fact")
            .transaction
            .is_none()
    );
}

#[test]
fn deterministic_backend_fixture_catalog_covers_protocols_and_standalone_services() {
    let http = crate::shared::trace_fixtures::canonical_http_trace()
        .into_iter()
        .next()
        .and_then(|span| extract_apm_fact("org-fixture", &span))
        .expect("http fact");
    assert_eq!(
        http.transaction.as_ref().map(|value| value.kind),
        Some(TransactionKind::Http)
    );

    let rpc = crate::shared::trace_fixtures::canonical_grpc_trace()
        .into_iter()
        .next()
        .and_then(|span| extract_apm_fact("org-fixture", &span))
        .expect("rpc fact");
    assert_eq!(
        rpc.transaction.as_ref().map(|value| value.kind),
        Some(TransactionKind::Rpc)
    );

    let database = crate::shared::trace_fixtures::canonical_sql_trace()
        .into_iter()
        .next()
        .and_then(|span| extract_apm_fact("org-fixture", &span))
        .expect("database fact");
    assert_eq!(
        database.dependency.as_ref().map(|value| value.category),
        Some(DependencyCategory::Database)
    );

    let backend_error = crate::shared::trace_fixtures::canonical_error_trace()
        .into_iter()
        .next()
        .and_then(|span| extract_apm_fact("org-fixture", &span))
        .expect("error fact");
    assert_eq!(backend_error.outcome, ApmOutcome::Error);
    assert!(backend_error.error.is_some());

    let mut messaging = span(5);
    messaging.resource.attributes.clear();
    messaging
        .attributes
        .insert("messaging.system".into(), json!("kafka"));
    messaging.attributes.insert(
        "messaging.destination.template".into(),
        json!("orders.{region}"),
    );
    messaging
        .attributes
        .insert("messaging.operation.name".into(), json!("process"));
    let messaging = extract_apm_fact("org-fixture", &messaging).expect("messaging fact");
    assert_eq!(
        messaging.transaction.as_ref().map(|value| value.kind),
        Some(TransactionKind::Messaging)
    );
    assert_eq!(
        messaging.dependency.as_ref().map(|value| value.category),
        None,
        "consumer spans are Transactions rather than dependencies"
    );
    assert_eq!(
        messaging.service.name,
        crate::domain::apm::DEFAULT_SERVICE_NAME
    );

    let mut producer = span(4);
    producer
        .attributes
        .insert("messaging.system".into(), json!("kafka"));
    producer.attributes.insert(
        "messaging.destination.template".into(),
        json!("orders.{region}"),
    );
    assert_eq!(
        extract_apm_fact("org-fixture", &producer)
            .and_then(|fact| fact.dependency)
            .map(|value| value.category),
        Some(DependencyCategory::Messaging)
    );

    let mut external = span(3);
    external
        .attributes
        .insert("http.request.method".into(), json!("GET"));
    external
        .attributes
        .insert("server.address".into(), json!("inventory.internal"));
    assert_eq!(
        extract_apm_fact("org-fixture", &external)
            .and_then(|fact| fact.dependency)
            .map(|value| value.category),
        Some(DependencyCategory::ExternalHttp)
    );
}
