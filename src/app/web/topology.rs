// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Topology 聚合 use case（web-investigation-shell）。
//!
//! 把 service_graph_edges 的 `EdgeSnapshot` 折叠成前端要的 `(nodes, edges)`：
//! - 同一 `(client, server)` 边按 request_count / error_count 累加，p95 取 max
//! - nodes 是 client+server 并集，每个 node 同样累加（双向都贡献）
//! - rps = request_count / window_secs

use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct RawEdgeSample<'a> {
    pub client: &'a str,
    pub server: &'a str,
    pub request_count: u64,
    pub error_count: u64,
    pub p95_us: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TopologyNode {
    pub id: String,
    pub name: String,
    pub error_rate: f64,
    pub p95_ms: f64,
    pub rps: f64,
    pub span_count: i64,
}

#[derive(Debug, Serialize)]
pub struct TopologyEdge {
    pub source: String,
    pub target: String,
    pub rps: f64,
    pub err_rate: f64,
    pub p95_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct TopologyResponse {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
}

#[derive(Default, Clone)]
struct EdgeAcc {
    request_count: u64,
    error_count: u64,
    p95_us_max: i64,
}

#[derive(Default, Clone)]
struct NodeAcc {
    request_count: u64,
    error_count: u64,
    p95_us_max: i64,
}

/// 聚合一组 edge 样本到 nodes / edges。`window_micros` 用于计算 RPS。
pub fn aggregate(samples: &[RawEdgeSample<'_>], window_micros: i64) -> TopologyResponse {
    let window_secs = (window_micros as f64) / 1_000_000.0;
    let mut edges: HashMap<(String, String), EdgeAcc> = HashMap::new();
    let mut nodes: HashMap<String, NodeAcc> = HashMap::new();

    for s in samples {
        let key = (s.client.to_string(), s.server.to_string());
        let e = edges.entry(key).or_default();
        e.request_count += s.request_count;
        e.error_count += s.error_count;
        if let Some(p95) = s.p95_us {
            e.p95_us_max = e.p95_us_max.max(p95);
        }

        for svc in [s.client, s.server] {
            let n = nodes.entry(svc.to_string()).or_default();
            n.request_count += s.request_count;
            n.error_count += s.error_count;
            if let Some(p95) = s.p95_us {
                n.p95_us_max = n.p95_us_max.max(p95);
            }
        }
    }

    let mut node_out: Vec<TopologyNode> = nodes
        .into_iter()
        .map(|(name, acc)| TopologyNode {
            id: name.clone(),
            name,
            error_rate: safe_ratio(acc.error_count, acc.request_count),
            p95_ms: (acc.p95_us_max as f64) / 1_000.0,
            rps: (acc.request_count as f64) / window_secs.max(f64::EPSILON),
            span_count: acc.request_count as i64,
        })
        .collect();
    node_out.sort_by(|a, b| a.name.cmp(&b.name));

    let mut edge_out: Vec<TopologyEdge> = edges
        .into_iter()
        .map(|((src, dst), acc)| TopologyEdge {
            source: src,
            target: dst,
            rps: (acc.request_count as f64) / window_secs.max(f64::EPSILON),
            err_rate: safe_ratio(acc.error_count, acc.request_count),
            p95_ms: (acc.p95_us_max as f64) / 1_000.0,
        })
        .collect();
    edge_out.sort_by(|a, b| a.source.cmp(&b.source).then(a.target.cmp(&b.target)));

    TopologyResponse {
        nodes: node_out,
        edges: edge_out,
    }
}

fn safe_ratio(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        (num as f64) / (den as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_folds_same_edge() {
        let s = vec![
            RawEdgeSample {
                client: "web",
                server: "api",
                request_count: 100,
                error_count: 10,
                p95_us: Some(5_000),
            },
            RawEdgeSample {
                client: "web",
                server: "api",
                request_count: 50,
                error_count: 2,
                p95_us: Some(8_000),
            },
        ];
        let r = aggregate(&s, 60_000_000);
        assert_eq!(r.edges.len(), 1);
        let e = &r.edges[0];
        assert!((e.err_rate - 12.0 / 150.0).abs() < 1e-9);
        assert!((e.p95_ms - 8.0).abs() < 1e-9);
        assert!((e.rps - 2.5).abs() < 1e-9);
    }

    #[test]
    fn aggregate_unions_nodes_from_both_sides() {
        let s = vec![
            RawEdgeSample {
                client: "web",
                server: "api",
                request_count: 10,
                error_count: 0,
                p95_us: None,
            },
            RawEdgeSample {
                client: "api",
                server: "db",
                request_count: 5,
                error_count: 0,
                p95_us: None,
            },
        ];
        let r = aggregate(&s, 60_000_000);
        assert_eq!(r.nodes.len(), 3);
        assert!(r.nodes.iter().any(|n| n.name == "web"));
        assert!(r.nodes.iter().any(|n| n.name == "api"));
        assert!(r.nodes.iter().any(|n| n.name == "db"));
    }

    #[test]
    fn aggregate_zero_requests_safe() {
        let r = aggregate(&[], 60_000_000);
        assert!(r.nodes.is_empty() && r.edges.is_empty());
    }
}
