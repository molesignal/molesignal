// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use super::*;
use crate::domain::apm::{
    BucketDimension, BucketMeasurements, DependencyIdentity, HistogramSchema, MergedBucket,
    ServiceIdentity, TransactionIdentity, TransactionKind,
};

fn measurements(requests: u64, errors: u64) -> BucketMeasurements {
    let mut measurements = BucketMeasurements::empty(&HistogramSchema::current());
    measurements.request_count = requests;
    measurements.error_count = errors;
    measurements
}

fn service(name: &str) -> ServiceIdentity {
    ServiceIdentity::new(Some("shop"), Some(name), Some("prod"), None)
}

fn dependency(
    caller: &str,
    category: DependencyCategory,
    target: &str,
    requests: u64,
) -> MergedBucket {
    MergedBucket {
        bucket_at: TimestampMicros(0),
        dimension: BucketDimension::Dependency {
            service: service(caller),
            version: None,
            dependency: DependencyIdentity {
                category,
                target: target.into(),
                operation: None,
            },
        },
        measurements: measurements(requests, 0),
    }
}

fn consumer(name: &str, transaction: &str, requests: u64) -> MergedBucket {
    MergedBucket {
        bucket_at: TimestampMicros(0),
        dimension: BucketDimension::Transaction {
            service: service(name),
            version: None,
            transaction: TransactionIdentity {
                name: transaction.into(),
                kind: TransactionKind::Messaging,
            },
        },
        measurements: measurements(requests, 1),
    }
}

fn call(client: &str, server: &str, requests: u64) -> GraphCall {
    GraphCall {
        client: client.into(),
        server: server.into(),
        request_count: requests,
        error_count: 0,
        p95_micros: Some(10_000),
    }
}

fn built(calls: &[GraphCall], dependencies: &[MergedBucket], consumers: &[MergedBucket]) -> Map {
    let schema = HistogramSchema::current();
    let mut map = Map::default();

    map.calls(calls);
    map.dependencies(dependencies, &schema)
        .expect("dependencies");
    map.consumers(consumers, &schema).expect("consumers");
    map
}

#[test]
fn services_externals_and_topics_are_joined_by_their_calls() {
    let mut map = built(
        &[call("web", "checkout", 100)],
        &[
            dependency("checkout", DependencyCategory::Database, "postgresql", 40),
            dependency("checkout", DependencyCategory::Messaging, "orders", 10),
            // The graph already paired this call: it is not drawn twice.
            dependency("web", DependencyCategory::Service, "checkout", 99),
            // A service that reports nothing of its own is still a service.
            dependency("checkout", DependencyCategory::Service, "legacy", 5),
        ],
        &[consumer("mailer", "process orders", 8)],
    );

    map.mark_changes(&Map::default());

    let kinds = map
        .edges
        .values()
        .map(|edge| (edge.id.as_str(), edge.kind))
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            ("service:checkout->database:postgresql", MapEdgeKind::Call),
            ("service:checkout->service:legacy", MapEdgeKind::Call),
            ("service:checkout->topic:orders", MapEdgeKind::Publish),
            ("service:web->service:checkout", MapEdgeKind::Call),
            ("topic:orders->service:mailer", MapEdgeKind::Consume),
        ],
    );
    assert_eq!(
        map.edges["service:web->service:checkout"].red.request_count,
        100
    );
    assert_eq!(map.nodes["database:postgresql"].kind, MapNodeKind::External);
    assert_eq!(map.nodes["database:postgresql"].red.request_count, 40);
    assert_eq!(map.nodes["topic:orders"].red.request_count, 10);
    assert_eq!(map.nodes["service:legacy"].red.request_count, 5);
    assert!((map.edges["topic:orders->service:mailer"].red.error_rate - 0.125).abs() < 1e-9);
}

#[test]
fn calls_new_and_gone_are_marked_against_the_stretch_before() {
    let mut now = built(
        &[call("web", "checkout", 10), call("web", "search", 3)],
        &[],
        &[],
    );
    let before = built(
        &[call("web", "checkout", 12), call("web", "legacy", 4)],
        &[],
        &[],
    );

    now.mark_changes(&before);

    assert_eq!(now.edges["service:web->service:checkout"].change, None);
    assert_eq!(
        now.edges["service:web->service:search"].change,
        Some(MapChange::New)
    );
    assert_eq!(
        now.edges["service:web->service:legacy"].change,
        Some(MapChange::Gone)
    );
    assert_eq!(
        now.edges["service:web->service:legacy"].red.request_count,
        0
    );
    assert_eq!(now.nodes["service:legacy"].change, Some(MapChange::Gone));

    // Nothing before: nothing is new.
    let mut first = built(&[call("web", "checkout", 10)], &[], &[]);
    first.mark_changes(&Map::default());

    assert!(first.edges.values().all(|edge| edge.change.is_none()));
}

#[test]
fn a_namespace_keeps_its_services_and_what_they_touch() {
    let mut map = built(
        &[
            call("web", "checkout", 10),
            call("checkout", "payments", 5),
            call("ads", "ranker", 2),
        ],
        &[],
        &[],
    );
    let namespaces = HashMap::from([
        ("web".to_owned(), "edge".to_owned()),
        ("checkout".to_owned(), "shop".to_owned()),
        ("payments".to_owned(), "pay".to_owned()),
        ("ads".to_owned(), "ads".to_owned()),
        ("ranker".to_owned(), "ads".to_owned()),
    ]);

    map.keep_namespace("shop", &namespaces);

    assert_eq!(map.edges.len(), 2);
    assert!(map.nodes.contains_key("service:web"));
    assert!(map.nodes.contains_key("service:payments"));
    assert!(!map.nodes.contains_key("service:ads"));
}

#[test]
fn a_big_map_keeps_its_busiest_nodes() {
    let calls = (0..10)
        .map(|index| call("web", &format!("service-{index}"), index))
        .collect::<Vec<_>>();
    let mut map = built(&calls, &[], &[]);

    map.mark_changes(&Map::default());

    assert!(map.keep_busiest(4));
    assert_eq!(map.nodes.len(), 4);
    assert!(map.nodes.contains_key("service:service-9"));
    assert!(
        map.edges
            .values()
            .all(|edge| map.nodes.contains_key(&edge.target))
    );
}

#[test]
fn a_consumer_reads_the_topic_after_its_operation() {
    assert_eq!(consumed_topic("process orders"), Some("orders"));
    assert_eq!(consumed_topic("receive orders.dlq"), Some("orders.dlq"));
    assert_eq!(consumed_topic("process"), None);
    assert_eq!(consumed_topic("process __other__"), None);
}
