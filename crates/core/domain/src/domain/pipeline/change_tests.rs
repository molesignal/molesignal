// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::*;

const MINUTE: i64 = 60_000_000;
/// 2026-11-24 10:00 UTC.
const NOW: i64 = 1_795_514_400 * 1_000_000;

fn org() -> Id {
    Id::from_string("org")
}

fn deployment(verb: &str, generation: i64, image: &str, replicas: i64, created: &str) -> Value {
    json!({
        "body": {
            "type": verb,
            "object": {
                "kind": "Deployment",
                "metadata": {
                    "name": "checkout-api",
                    "namespace": "shop",
                    "generation": generation,
                    "creationTimestamp": created,
                    "labels": {"app.kubernetes.io/name": "checkout"}
                },
                "spec": {
                    "replicas": replicas,
                    "template": {"spec": {"containers": [{"name": "app", "image": image}]}}
                }
            }
        },
        "k8s.cluster.name": "prod-eu"
    })
}

fn note(watched: &mut WatchedObjects, value: &Value) -> Option<NotedChange> {
    let spec = ChangeSpec {
        environment: String::new(),
        environment_field: "k8s.cluster.name".into(),
    };

    watched.note(&org(), value, &spec, TimestampMicros(NOW))
}

const OLD: &str = "2026-01-01T00:00:00Z";

#[test]
fn the_first_sighting_of_an_old_workload_only_starts_the_watch() {
    let mut watched = WatchedObjects::default();

    assert_eq!(
        note(
            &mut watched,
            &deployment("ADDED", 4, "registry:5000/shop/checkout:1.9.0", 3, OLD)
        ),
        None
    );
}

#[test]
fn a_new_image_is_a_deployment_of_its_tag() {
    let mut watched = WatchedObjects::default();
    note(
        &mut watched,
        &deployment("ADDED", 4, "registry:5000/shop/checkout:1.9.0", 3, OLD),
    );

    let noted = note(
        &mut watched,
        &deployment("MODIFIED", 5, "registry:5000/shop/checkout:2.0.0", 3, OLD),
    )
    .expect("a change");

    assert_eq!(noted.kind, ChangeKind::Deployment);
    assert_eq!(noted.service, "checkout");
    assert_eq!(noted.namespace, "shop");
    assert_eq!(noted.environment, "prod-eu");
    assert_eq!(noted.version, "2.0.0");
    assert!(
        noted
            .description
            .contains("checkout:1.9.0 → registry:5000/shop/checkout:2.0.0"),
        "{}",
        noted.description
    );
}

#[test]
fn new_replicas_are_a_scaling_and_a_status_update_is_nothing() {
    let mut watched = WatchedObjects::default();
    note(
        &mut watched,
        &deployment("ADDED", 4, "checkout:1.9.0", 3, OLD),
    );

    // The status moved; the generation did not.
    assert_eq!(
        note(
            &mut watched,
            &deployment("MODIFIED", 4, "checkout:1.9.0", 3, OLD)
        ),
        None
    );

    let scaled = note(
        &mut watched,
        &deployment("MODIFIED", 5, "checkout:1.9.0", 6, OLD),
    )
    .expect("a change");

    assert_eq!(scaled.kind, ChangeKind::Scaling);
    assert!(scaled.description.ends_with("3 → 6 replicas"));

    let tuned = note(
        &mut watched,
        &deployment("MODIFIED", 6, "checkout:1.9.0", 6, OLD),
    )
    .expect("a change");

    assert_eq!(tuned.kind, ChangeKind::Configuration);
}

#[test]
fn a_workload_made_just_now_is_a_deployment_and_one_deleted_an_infrastructure_change() {
    let mut watched = WatchedObjects::default();
    let created = chrono::DateTime::from_timestamp_micros(NOW - MINUTE)
        .expect("a time")
        .to_rfc3339();

    let made = note(
        &mut watched,
        &deployment("ADDED", 1, "checkout@sha256:abc", 1, &created),
    )
    .expect("a change");

    assert_eq!(made.kind, ChangeKind::Deployment);
    assert_eq!(made.version, "sha256:abc");

    let gone = note(
        &mut watched,
        &deployment("DELETED", 1, "checkout@sha256:abc", 1, &created),
    )
    .expect("a change");

    assert_eq!(gone.kind, ChangeKind::Infrastructure);
    assert_eq!(gone.description, "Deployment checkout-api deleted");
}

#[test]
fn a_config_map_whose_data_moved_is_a_configuration_change() {
    let mut watched = WatchedObjects::default();
    let config = |data: Value| {
        json!({"body": serde_json::to_string(&json!({
            "type": "MODIFIED",
            "object": {"kind": "ConfigMap", "metadata": {"name": "checkout-config", "namespace": "shop", "labels": {"app": "checkout"}}, "data": data}
        })).unwrap()})
    };

    assert_eq!(note(&mut watched, &config(json!({"pool": "10"}))), None);
    assert_eq!(note(&mut watched, &config(json!({"pool": "10"}))), None);

    let changed = note(&mut watched, &config(json!({"pool": "50"}))).expect("a change");

    assert_eq!(changed.kind, ChangeKind::Configuration);
    assert_eq!(changed.service, "checkout");
}

#[test]
fn objects_of_other_kinds_and_other_events_are_left_alone() {
    let mut watched = WatchedObjects::default();

    assert_eq!(
        note(
            &mut watched,
            &json!({"body": {"type": "ADDED", "object": {"kind": "Pod", "metadata": {"name": "p"}}}})
        ),
        None
    );
    assert_eq!(
        note(&mut watched, &json!({"message": "GET /orders 200"})),
        None
    );
}

#[test]
fn a_set_environment_wins_over_the_field() {
    let mut watched = WatchedObjects::default();
    let spec = ChangeSpec {
        environment: "production".into(),
        environment_field: "k8s.cluster.name".into(),
    };
    let at = TimestampMicros(NOW);

    watched.note(
        &org(),
        &deployment("ADDED", 1, "checkout:1", 1, OLD),
        &spec,
        at,
    );

    let noted = watched
        .note(
            &org(),
            &deployment("MODIFIED", 2, "checkout:2", 1, OLD),
            &spec,
            at,
        )
        .expect("a change");

    assert_eq!(noted.environment, "production");
}
