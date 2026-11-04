// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The map as it is built: a node for each service that served, then the
//! calls the service graph paired, what services call besides each other and
//! the topics they consume from; then what changed against the stretch
//! before, the namespace kept and the busiest nodes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    GraphCall, MapChange, MapEdge, MapEdgeKind, MapNode, MapNodeKind, MapRed, consumed_topic,
    map_red, service_node, target_node, topic_node,
};
use crate::{
    app::apm::query::{
        aggregate::red_from_measurements, apdex::ApdexThresholds, health::assess_health,
    },
    domain::apm::{
        BucketDimension, BucketMeasurements, DependencyCategory, HealthLines, HistogramSchema,
        MergedBucket, ServiceIdentity, TransactionKind,
    },
    shared::Result,
};

#[derive(Default)]
pub(super) struct Map {
    pub(super) nodes: BTreeMap<String, MapNode>,
    pub(super) edges: BTreeMap<String, MapEdge>,
}

impl Map {
    /// A node for each service that served, its figures read with its Apdex T
    /// and its health against the organization's lines.
    pub(super) fn services(
        &mut self,
        buckets: &[MergedBucket],
        schema: &HistogramSchema,
        (thresholds, lines): (&ApdexThresholds, &HealthLines),
    ) -> Result<()> {
        let mut served = BTreeMap::<&str, (Vec<&ServiceIdentity>, Vec<&BucketMeasurements>)>::new();

        for bucket in buckets {
            if let BucketDimension::Service { service, .. } = &bucket.dimension {
                let entry = served.entry(service.name.as_str()).or_default();
                entry.0.push(service);
                entry.1.push(&bucket.measurements);
            }
        }

        for (name, (identities, measurements)) in served {
            let threshold = thresholds.of(identities[0]);
            let red = red_from_measurements(measurements, schema, threshold)?;
            let (health, _) = assess_health(&red, lines);
            let environments = identities
                .iter()
                .map(|identity| identity.environment.clone())
                .filter(|environment| !environment.is_empty())
                .collect::<BTreeSet<_>>();

            let node = self.node(&service_node(name), MapNodeKind::Service, name);
            node.red = map_red(&red);
            node.health = Some(health);
            node.environments = environments.into_iter().collect();
        }

        Ok(())
    }

    /// The calls the service graph paired, between services.
    pub(super) fn calls(&mut self, calls: &[GraphCall]) {
        for call in calls {
            let source = service_node(&call.client);
            let target = service_node(&call.server);

            self.node(&source, MapNodeKind::Service, &call.client);
            self.node(&target, MapNodeKind::Service, &call.server);

            let edge = self.edge(source, target, MapEdgeKind::Call);
            edge.red.request_count += call.request_count;
            edge.red.error_count += call.error_count;
            edge.red.p95_time = edge.red.p95_time.max(call.p95_micros);
        }
    }

    /// What services call besides each other, and the topics they publish
    /// to. A call to another service the graph already paired is not drawn twice.
    pub(super) fn dependencies(
        &mut self,
        buckets: &[MergedBucket],
        schema: &HistogramSchema,
    ) -> Result<()> {
        let mut called =
            BTreeMap::<(&str, DependencyCategory, &str), Vec<&BucketMeasurements>>::new();

        for bucket in buckets {
            if let BucketDimension::Dependency {
                service,
                dependency,
                ..
            } = &bucket.dimension
            {
                called
                    .entry((
                        service.name.as_str(),
                        dependency.category,
                        dependency.target.as_str(),
                    ))
                    .or_default()
                    .push(&bucket.measurements);
            }
        }

        for ((caller, category, target), measurements) in called {
            let (target_id, kind) = target_node(category, target);
            let source = service_node(caller);
            let edge_id = edge_id(&source, &target_id);

            if category == DependencyCategory::Service && self.edges.contains_key(&edge_id) {
                continue;
            }

            let red = red_from_measurements(measurements, schema, 0)?;

            self.node(&source, MapNodeKind::Service, caller);

            let node = self.node(&target_id, kind, target);
            if kind != MapNodeKind::Service {
                node.category = Some(category);
            }

            let edge_kind = if kind == MapNodeKind::Topic {
                MapEdgeKind::Publish
            } else {
                MapEdgeKind::Call
            };
            let edge = self.edge(source, target_id, edge_kind);
            edge.red = map_red(&red);
        }

        Ok(())
    }

    /// The topics services consume from, read from their messaging entries.
    pub(super) fn consumers(
        &mut self,
        buckets: &[MergedBucket],
        schema: &HistogramSchema,
    ) -> Result<()> {
        let mut consumed = BTreeMap::<(&str, &str), Vec<&BucketMeasurements>>::new();

        for bucket in buckets {
            if let BucketDimension::Transaction {
                service,
                transaction,
                ..
            } = &bucket.dimension
                && transaction.kind == TransactionKind::Messaging
                && let Some(topic) = consumed_topic(&transaction.name)
            {
                consumed
                    .entry((topic, service.name.as_str()))
                    .or_default()
                    .push(&bucket.measurements);
            }
        }

        for ((topic, consumer), measurements) in consumed {
            let red = red_from_measurements(measurements, schema, 0)?;
            let source = topic_node(topic);
            let target = service_node(consumer);

            self.node(&source, MapNodeKind::Topic, topic).category =
                Some(DependencyCategory::Messaging);
            self.node(&target, MapNodeKind::Service, consumer);
            self.edge(source, target, MapEdgeKind::Consume).red = map_red(&red);
        }

        Ok(())
    }

    /// Marks the calls not made before as new and adds those made before
    /// and not now as gone; nothing is marked when nothing was made before.
    pub(super) fn mark_changes(&mut self, before: &Map) {
        if before.edges.is_empty() {
            self.finish_red();
            return;
        }

        for (id, edge) in &mut self.edges {
            if !before.edges.contains_key(id) {
                edge.change = Some(MapChange::New);
            }
        }

        for (id, edge) in &before.edges {
            if self.edges.contains_key(id) {
                continue;
            }

            for end in [&edge.source, &edge.target] {
                if !self.nodes.contains_key(end)
                    && let Some(node) = before.nodes.get(end)
                {
                    let mut gone = node.clone();
                    gone.red = MapRed::default();
                    gone.health = None;
                    gone.change = Some(MapChange::Gone);
                    self.nodes.insert(end.clone(), gone);
                }
            }

            let mut gone = edge.clone();
            gone.red = MapRed::default();
            gone.change = Some(MapChange::Gone);
            self.edges.insert(id.clone(), gone);
        }

        self.finish_red();
    }

    /// External nodes and topics with what they were asked by every caller;
    /// every call's error rate.
    fn finish_red(&mut self) {
        for edge in self.edges.values_mut() {
            edge.red.error_rate = rate(edge.red.error_count, edge.red.request_count);
        }

        for node in self.nodes.values_mut() {
            if node.kind == MapNodeKind::Service && node.health.is_some() {
                continue;
            }

            let incoming = self.edges.values().filter(|edge| edge.target == node.id);
            let mut red = MapRed::default();

            for edge in incoming {
                red.request_count += edge.red.request_count;
                red.error_count += edge.red.error_count;
                red.p95_time = red.p95_time.max(edge.red.p95_time);
            }

            red.error_rate = rate(red.error_count, red.request_count);
            node.red = red;
        }
    }

    /// The services of `namespace`, what they call and what calls them.
    pub(super) fn keep_namespace(&mut self, namespace: &str, namespaces: &HashMap<String, String>) {
        let inside = |id: &str| {
            id.strip_prefix("service:")
                .is_some_and(|name| namespaces.get(name).is_some_and(|value| value == namespace))
        };

        self.edges
            .retain(|_, edge| inside(&edge.source) || inside(&edge.target));

        let ends = self
            .edges
            .values()
            .flat_map(|edge| [edge.source.clone(), edge.target.clone()])
            .collect::<BTreeSet<_>>();

        self.nodes.retain(|id, _| inside(id) || ends.contains(id));
    }

    pub(super) fn name_namespaces(&mut self, namespaces: &HashMap<String, String>) {
        for node in self.nodes.values_mut() {
            if node.kind == MapNodeKind::Service {
                node.namespace = namespaces
                    .get(&node.name)
                    .filter(|namespace| !namespace.is_empty())
                    .cloned();
            }
        }
    }

    /// Keeps the `limit` busiest nodes, and the calls between them.
    pub(super) fn keep_busiest(&mut self, limit: usize) -> bool {
        if self.nodes.len() <= limit {
            return false;
        }

        let mut ranked = self
            .nodes
            .values()
            .map(|node| (node.red.request_count, node.id.clone()))
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| right.cmp(left));

        let kept = ranked
            .into_iter()
            .take(limit)
            .map(|(_, id)| id)
            .collect::<BTreeSet<_>>();

        self.nodes.retain(|id, _| kept.contains(id));
        self.edges
            .retain(|_, edge| kept.contains(&edge.source) && kept.contains(&edge.target));
        true
    }

    fn node(&mut self, id: &str, kind: MapNodeKind, name: &str) -> &mut MapNode {
        self.nodes.entry(id.to_owned()).or_insert_with(|| MapNode {
            id: id.to_owned(),
            kind,
            name: name.to_owned(),
            namespace: None,
            category: None,
            environments: Vec::new(),
            red: MapRed::default(),
            health: None,
            change: None,
        })
    }

    fn edge(&mut self, source: String, target: String, kind: MapEdgeKind) -> &mut MapEdge {
        self.edges
            .entry(edge_id(&source, &target))
            .or_insert_with(|| MapEdge {
                id: edge_id(&source, &target),
                source,
                target,
                kind,
                red: MapRed::default(),
                change: None,
            })
    }
}

fn edge_id(source: &str, target: &str) -> String {
    format!("{source}->{target}")
}

fn rate(errors: u64, requests: u64) -> f64 {
    if requests == 0 {
        0.0
    } else {
        errors as f64 / requests as f64
    }
}
