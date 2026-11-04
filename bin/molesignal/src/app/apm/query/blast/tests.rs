// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::{
    app::apm::ApmQueryRange,
    domain::apm::{DataQuality, QueryResolution},
    shared::time::TimestampMicros,
};

fn red(requests: u64, errors: u64) -> MapRed {
    MapRed {
        request_count: requests,
        error_count: errors,
        error_rate: errors as f64 / requests.max(1) as f64,
        p95_time: Some(100_000),
    }
}

fn node(id: &str, kind: MapNodeKind, health: Option<ServiceHealth>, red: MapRed) -> MapNode {
    MapNode {
        id: id.into(),
        kind,
        name: id.split_once(':').map_or(id, |(_, name)| name).into(),
        namespace: None,
        category: None,
        environments: Vec::new(),
        red,
        health,
        change: None,
    }
}

fn edge(source: &str, target: &str, kind: MapEdgeKind, red: MapRed) -> MapEdge {
    MapEdge {
        id: format!("{source}->{target}"),
        source: source.into(),
        target: target.into(),
        kind,
        red,
        change: None,
    }
}

fn map(nodes: Vec<MapNode>, edges: Vec<MapEdge>) -> MapResponse {
    MapResponse {
        meta: ApmResponseMeta {
            range: ApmQueryRange {
                from: TimestampMicros(0),
                to: TimestampMicros(1),
            },
            resolution: QueryResolution::Minute,
            projection_started_at: None,
            last_complete_bucket_at: None,
            data_quality: DataQuality::default(),
            activation_boundary: false,
        },
        nodes,
        edges,
        namespaces: Vec::new(),
        environments: Vec::new(),
        truncated: false,
    }
}

#[test]
fn callers_are_reached_up_and_what_is_called_down_the_unhealthy_first() {
    let healthy = Some(ServiceHealth::Healthy);
    let topology = map(
        vec![
            node("service:web", MapNodeKind::Service, healthy, red(500, 0)),
            node(
                "service:gateway",
                MapNodeKind::Service,
                healthy,
                red(900, 1),
            ),
            node(
                "service:checkout",
                MapNodeKind::Service,
                Some(ServiceHealth::Warning),
                red(400, 20),
            ),
            node(
                "service:payments",
                MapNodeKind::Service,
                Some(ServiceHealth::Critical),
                red(300, 60),
            ),
            node("database:orders", MapNodeKind::External, None, red(800, 0)),
            node("topic:orders", MapNodeKind::Topic, None, red(100, 0)),
            node(
                "service:billing",
                MapNodeKind::Service,
                healthy,
                red(100, 0),
            ),
        ],
        vec![
            edge(
                "service:web",
                "service:gateway",
                MapEdgeKind::Call,
                red(500, 0),
            ),
            edge(
                "service:gateway",
                "service:checkout",
                MapEdgeKind::Call,
                red(400, 20),
            ),
            edge(
                "service:checkout",
                "service:payments",
                MapEdgeKind::Call,
                red(300, 60),
            ),
            edge(
                "service:checkout",
                "database:orders",
                MapEdgeKind::Call,
                red(800, 0),
            ),
            edge(
                "service:checkout",
                "topic:orders",
                MapEdgeKind::Publish,
                red(100, 0),
            ),
            edge(
                "topic:orders",
                "service:billing",
                MapEdgeKind::Consume,
                red(100, 0),
            ),
        ],
    );

    let radius = reach(topology, "checkout", &HealthLines::default());
    fn ids(nodes: &[BlastNode]) -> Vec<(&str, usize)> {
        nodes
            .iter()
            .map(|node| (node.id.as_str(), node.hops))
            .collect()
    }

    assert_eq!(
        ids(&radius.upstream),
        [("service:gateway", 1), ("service:web", 2)]
    );
    assert_eq!(radius.upstream[1].via, "service:gateway");
    // The failing service it calls first; a consumer of its topic is not waited on.
    assert_eq!(
        ids(&radius.downstream),
        [
            ("service:payments", 1),
            ("database:orders", 1),
            ("topic:orders", 1)
        ]
    );
    assert_eq!(radius.downstream[0].call.error_count, 60);
    assert_eq!(radius.downstream[1].health, ServiceHealth::Healthy);
    assert_eq!(
        radius.service.map(|service| service.health),
        Some(ServiceHealth::Warning)
    );
}

#[test]
fn a_service_off_the_map_reaches_nothing_and_gone_calls_reach_nothing() {
    let mut gone = edge(
        "service:web",
        "service:checkout",
        MapEdgeKind::Call,
        red(0, 0),
    );
    gone.change = Some(MapChange::Gone);
    let topology = map(
        vec![
            node("service:web", MapNodeKind::Service, None, red(0, 0)),
            node("service:checkout", MapNodeKind::Service, None, red(0, 0)),
        ],
        vec![gone],
    );

    let radius = reach(topology, "checkout", &HealthLines::default());

    assert!(radius.upstream.is_empty());
    assert_eq!(
        radius.service.map(|service| service.health),
        Some(ServiceHealth::NoTraffic)
    );
    assert!(
        reach(
            map(Vec::new(), Vec::new()),
            "checkout",
            &HealthLines::default()
        )
        .service
        .is_none()
    );
}
