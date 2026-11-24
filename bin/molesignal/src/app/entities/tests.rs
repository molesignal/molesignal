// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::entity::{EntityKind, EntityRef, Relation, sighting_of};
use kernel::time::TimestampMicros;

use super::*;

fn pod(name: &str, node: &str, service: &str, at: i64) -> domain::entity::Sighting {
    let attributes = [
        ("k8s.cluster.name", "prod"),
        ("k8s.node.name", node),
        ("k8s.namespace.name", "shop"),
        ("k8s.pod.name", name),
    ]
    .iter()
    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
    .collect::<BTreeMap<_, _>>();

    sighting_of(&attributes, Some(service), TimestampMicros(at))
}

#[test]
fn a_sweep_keeps_each_entity_once_with_its_whole_span_and_latest_attributes() {
    let mut found = Found::default();
    found.absorb(pod("checkout-1", "node-1", "shop/checkout/prod", 10));
    found.absorb(pod("checkout-1", "node-1", "shop/checkout/prod", 30));
    found.absorb(pod("cart-1", "node-1", "shop/cart/prod", 20));

    let (entities, links) = found.into_parts();
    let node = entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Node)
        .expect("the node");

    // A cluster, a node, two pods.
    assert_eq!(entities.len(), 4);
    assert_eq!((node.first_seen_at.0, node.last_seen_at.0), (10, 30));
    // node → cluster, two pods → node, two services → pods.
    assert_eq!(links.len(), 5);
}

#[test]
fn around_an_entity_are_what_it_runs_on_what_runs_on_it_and_its_services() {
    let mut found = Found::default();
    found.absorb(pod("checkout-1", "node-1", "shop/checkout/prod", 10));
    found.absorb(pod("cart-1", "node-1", "shop/cart/prod", 10));

    let (_, links) = found.into_parts();
    let node = EntityRef::entity(EntityKind::Node, "prod/node-1");
    let (parents, children, services) = around(&node, &links);

    assert_eq!(
        parents
            .iter()
            .map(|parent| parent.name.as_str())
            .collect::<Vec<_>>(),
        ["prod"]
    );
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["cart-1", "checkout-1"]
    );
    // Services run on the pods, not on the node itself.
    assert!(services.is_empty());

    let pod = EntityRef::entity(EntityKind::Pod, "prod/shop/checkout-1");
    let (_, _, on_pod) = around(&pod, &links);

    assert_eq!(on_pod, ["shop/checkout/prod"]);
    assert!(links.iter().any(|link| link.relation == Relation::RunsOn));
}

#[test]
fn a_service_key_reads_back_as_its_identity() {
    let service = service_of("shop/checkout/prod").expect("an identity");

    assert_eq!(
        (
            service.namespace.as_str(),
            service.name.as_str(),
            service.environment.as_str()
        ),
        ("shop", "checkout", "prod")
    );
    assert!(service_of("checkout").is_none());
}
