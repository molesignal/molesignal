// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What one set of resource attributes says, by the OpenTelemetry semantic conventions: the
//! cluster, node, pod and container a process runs in, or the host it runs on outside
//! Kubernetes, and what runs on or is part of what. Inside a pod `host.name` names the pod, not
//! a machine, so a pod's attributes make no host.

use std::collections::BTreeMap;

use kernel::time::TimestampMicros;

use super::{Entity, EntityKind, EntityLink, EntityRef, Relation};

/// The attributes an entity keeps, of those it was named with.
const KEPT: [&str; 9] = [
    "k8s.cluster.name",
    "k8s.node.name",
    "k8s.namespace.name",
    "k8s.pod.name",
    "k8s.deployment.name",
    "container.name",
    "host.name",
    "cloud.provider",
    "cloud.region",
];

/// The entities and links one set of attributes names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sighting {
    pub entities: Vec<Entity>,
    pub links: Vec<EntityLink>,
}

/// What `attributes` name, seen at `at`; `service` (`namespace/name/environment`) runs on the
/// innermost place they name.
pub fn sighting_of(
    attributes: &BTreeMap<String, String>,
    service: Option<&str>,
    at: TimestampMicros,
) -> Sighting {
    let value = |key: &str| {
        attributes
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    let kept = KEPT
        .iter()
        .filter_map(|key| value(key).map(|value| ((*key).to_owned(), value.to_owned())))
        .collect::<BTreeMap<_, _>>();

    let mut sighting = Sighting::default();
    let mut add = |kind: EntityKind, key: String, name: &str| {
        sighting.entities.push(Entity {
            kind,
            key: key.clone(),
            name: name.to_owned(),
            attributes: kept.clone(),
            first_seen_at: at,
            last_seen_at: at,
        });
        EntityRef::entity(kind, key)
    };

    let cluster = value("k8s.cluster.name");
    let node = value("k8s.node.name");
    let pod = value("k8s.pod.name");
    let in_cluster = |name: &str| format!("{}/{name}", cluster.unwrap_or_default());

    let cluster_ref = cluster.map(|name| add(EntityKind::Cluster, name.to_owned(), name));
    let node_ref = node.map(|name| add(EntityKind::Node, in_cluster(name), name));
    let pod_ref = pod.map(|name| {
        let namespace = value("k8s.namespace.name").unwrap_or_default();
        add(
            EntityKind::Pod,
            in_cluster(&format!("{namespace}/{name}")),
            name,
        )
    });
    let host_ref = (cluster.is_none() && node.is_none() && pod.is_none())
        .then(|| value("host.name"))
        .flatten()
        .map(|name| add(EntityKind::Host, name.to_owned(), name));
    let container_ref = value("container.id").map(|id| {
        let name = value("container.name").unwrap_or(id);
        add(EntityKind::Container, id.to_owned(), name)
    });

    let mut link = |from: Option<&EntityRef>, relation: Relation, to: Option<&EntityRef>| {
        if let (Some(from), Some(to)) = (from, to) {
            sighting.links.push(EntityLink {
                from: from.clone(),
                relation,
                to: to.clone(),
                last_seen_at: at,
            });
        }
    };

    link(node_ref.as_ref(), Relation::PartOf, cluster_ref.as_ref());
    link(pod_ref.as_ref(), Relation::RunsOn, node_ref.as_ref());
    link(container_ref.as_ref(), Relation::PartOf, pod_ref.as_ref());

    // A service runs on the innermost place named: its pod, else its host, else its node.
    let place = pod_ref.or(host_ref).or(node_ref);
    let service = service.map(EntityRef::service);

    link(service.as_ref(), Relation::RunsOn, place.as_ref());
    sighting
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn kinds(sighting: &Sighting) -> Vec<(EntityKind, &str)> {
        sighting
            .entities
            .iter()
            .map(|entity| (entity.kind, entity.key.as_str()))
            .collect()
    }

    #[test]
    fn a_pod_names_its_cluster_node_and_container_and_no_host() {
        let sighting = sighting_of(
            &attributes(&[
                ("k8s.cluster.name", "prod-eu"),
                ("k8s.node.name", "node-7"),
                ("k8s.namespace.name", "shop"),
                ("k8s.pod.name", "checkout-7d9-x2"),
                ("host.name", "checkout-7d9-x2"),
                ("container.id", "abc123"),
                ("container.name", "app"),
            ]),
            Some("shop/checkout/prod"),
            TimestampMicros(1),
        );

        assert_eq!(
            kinds(&sighting),
            vec![
                (EntityKind::Cluster, "prod-eu"),
                (EntityKind::Node, "prod-eu/node-7"),
                (EntityKind::Pod, "prod-eu/shop/checkout-7d9-x2"),
                (EntityKind::Container, "abc123"),
            ]
        );

        let links = sighting
            .links
            .iter()
            .map(|link| {
                (
                    link.from.kind.as_str(),
                    link.relation,
                    link.to.kind.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            links,
            vec![
                ("node", Relation::PartOf, "cluster"),
                ("pod", Relation::RunsOn, "node"),
                ("container", Relation::PartOf, "pod"),
                ("service", Relation::RunsOn, "pod"),
            ]
        );
    }

    #[test]
    fn outside_kubernetes_a_service_runs_on_its_host() {
        let sighting = sighting_of(
            &attributes(&[("host.name", "web-1"), ("cloud.region", "eu-west-1")]),
            Some("shop/search/prod"),
            TimestampMicros(1),
        );

        assert_eq!(kinds(&sighting), vec![(EntityKind::Host, "web-1")]);
        assert_eq!(sighting.entities[0].attributes["cloud.region"], "eu-west-1");
        assert_eq!(
            sighting.links[0].to,
            EntityRef::entity(EntityKind::Host, "web-1")
        );
    }

    #[test]
    fn attributes_that_name_no_place_name_nothing() {
        let sighting = sighting_of(
            &attributes(&[("service.name", "x")]),
            Some("a/b/c"),
            TimestampMicros(1),
        );

        assert_eq!(sighting, Sighting::default());
    }
}
