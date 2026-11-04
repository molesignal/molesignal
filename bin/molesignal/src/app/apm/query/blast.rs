// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a service reaches along the map, as an incident on it does: the
//! services that call it, a hop at a time, whose requests it may fail or
//! slow; and what it calls or publishes to, where the cause may lie, the
//! unhealthy first. Calls through a topic do not wait on their consumers, so
//! nothing reaches a producer from them.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use service_graph::ServiceGraphRepository;

use super::{
    ApmQueryContext, ApmQueryService, ApmResponseMeta, MapChange, MapEdge, MapEdgeKind, MapNode,
    MapNodeKind, MapRed, MapResponse, ServiceHealth, map::service_node,
};
use crate::{
    domain::apm::{DependencyCategory, HealthLines},
    shared::Result,
};

/// How far up the callers go, and how far down what is called.
pub const UPSTREAM_HOPS: usize = 3;
pub const DOWNSTREAM_HOPS: usize = 2;
/// The most nodes each side lists.
const MOST_NODES: usize = 30;

#[derive(Debug, Clone, Serialize)]
pub struct BlastRadius {
    pub meta: ApmResponseMeta,
    /// The service itself; absent when the map has it nowhere in the range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<BlastNode>,
    /// Who calls it, and who calls them: what its trouble reaches.
    pub upstream: Vec<BlastNode>,
    /// What it calls or publishes to: where its trouble may come from.
    pub downstream: Vec<BlastNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlastNode {
    pub id: String,
    pub kind: MapNodeKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<DependencyCategory>,
    /// Calls between it and the service: 1 for a direct caller or callee.
    pub hops: usize,
    /// The node it is reached through, the one a hop nearer the service.
    pub via: String,
    /// A service's own health; for what is called, read from how its calls fail.
    pub health: ServiceHealth,
    /// What it served or was asked.
    pub red: MapRed,
    /// The calls between it and the node it is reached through.
    pub call: MapRed,
}

impl ApmQueryService {
    /// What `service` reaches along the map of `context`, its calls between
    /// services read from `graph`.
    pub async fn blast_radius(
        &self,
        context: &ApmQueryContext,
        graph: &dyn ServiceGraphRepository,
        service: &str,
    ) -> Result<BlastRadius> {
        let map = self.map_of(context, graph).await?;
        let lines = self.org_settings(&context.org_id).await.health;

        Ok(reach(map, service, &lines))
    }
}

/// What `service` reaches along `map`, a node's health read against `lines`
/// when the map gives none.
pub fn reach(map: MapResponse, service: &str, lines: &HealthLines) -> BlastRadius {
    let start = service_node(service);
    let nodes = map
        .nodes
        .iter()
        .filter(|node| node.change != Some(MapChange::Gone))
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let edges = map
        .edges
        .iter()
        .filter(|edge| edge.change != Some(MapChange::Gone))
        .collect::<Vec<_>>();

    let callers = |id: &str| {
        edges
            .iter()
            .filter(|edge| edge.kind == MapEdgeKind::Call && edge.target == id)
            .map(|edge| (edge.source.as_str(), *edge))
            .collect::<Vec<_>>()
    };
    let called = |id: &str| {
        edges
            .iter()
            .filter(|edge| edge.kind != MapEdgeKind::Consume && edge.source == id)
            .map(|edge| (edge.target.as_str(), *edge))
            .collect::<Vec<_>>()
    };

    let mut upstream = walk(&start, UPSTREAM_HOPS, callers, &nodes, lines);
    let mut downstream = walk(&start, DOWNSTREAM_HOPS, called, &nodes, lines);

    upstream.sort_by(|left, right| {
        left.hops
            .cmp(&right.hops)
            .then_with(|| right.call.request_count.cmp(&left.call.request_count))
            .then_with(|| left.id.cmp(&right.id))
    });
    downstream.sort_by(|left, right| {
        health_rank(left.health)
            .cmp(&health_rank(right.health))
            .then_with(|| left.hops.cmp(&right.hops))
            .then_with(|| right.call.error_rate.total_cmp(&left.call.error_rate))
            .then_with(|| left.id.cmp(&right.id))
    });
    upstream.truncate(MOST_NODES);
    downstream.truncate(MOST_NODES);

    BlastRadius {
        service: nodes
            .get(start.as_str())
            .map(|node| blast_node(node, 0, &start, MapRed::default(), lines)),
        meta: map.meta,
        upstream,
        downstream,
    }
}

/// The nodes `next` leads to from `start`, each once, at most `hops` away.
fn walk<'a>(
    start: &str,
    hops: usize,
    next: impl Fn(&str) -> Vec<(&'a str, &'a MapEdge)>,
    nodes: &HashMap<&str, &'a MapNode>,
    lines: &HealthLines,
) -> Vec<BlastNode> {
    let mut seen = HashSet::from([start.to_owned()]);
    let mut queue = VecDeque::from([(start.to_owned(), 0)]);
    let mut reached = Vec::new();

    while let Some((id, depth)) = queue.pop_front() {
        if depth == hops {
            continue;
        }

        for (other, edge) in next(&id) {
            if !seen.insert(other.to_owned()) {
                continue;
            }

            if let Some(node) = nodes.get(other) {
                reached.push(blast_node(node, depth + 1, &id, edge.red.clone(), lines));

                // Past a topic or what services call, nothing more is known.
                if node.kind == MapNodeKind::Service {
                    queue.push_back((other.to_owned(), depth + 1));
                }
            }
        }
    }

    reached
}

fn blast_node(
    node: &MapNode,
    hops: usize,
    via: &str,
    call: MapRed,
    lines: &HealthLines,
) -> BlastNode {
    BlastNode {
        id: node.id.clone(),
        kind: node.kind,
        name: node.name.clone(),
        namespace: node.namespace.clone(),
        category: node.category,
        hops,
        via: via.to_owned(),
        health: node
            .health
            .unwrap_or_else(|| called_health(&node.red, lines)),
        red: node.red.clone(),
        call,
    }
}

/// The health of what is called, from how its calls fail.
fn called_health(red: &MapRed, lines: &HealthLines) -> ServiceHealth {
    if red.request_count == 0 {
        ServiceHealth::NoTraffic
    } else if red.error_rate >= lines.error_rate_critical {
        ServiceHealth::Critical
    } else if red.error_rate >= lines.error_rate_warning {
        ServiceHealth::Warning
    } else {
        ServiceHealth::Healthy
    }
}

fn health_rank(health: ServiceHealth) -> u8 {
    match health {
        ServiceHealth::Critical => 0,
        ServiceHealth::Warning => 1,
        ServiceHealth::Healthy => 2,
        ServiceHealth::NoTraffic => 3,
    }
}

#[cfg(test)]
mod tests;
