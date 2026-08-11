// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/topology?from=&to=` —— 聚合 service_graph_edges 到 nodes / edges。
//!
//! 走现有 [`crate::api::state::TelemetryState::service_graph`]，按 ISO datetime / RFC3339
//! 解析时间窗，按 (client_service, server_service) 折叠成边，并聚合两端节点指标。

use std::collections::HashMap;

use axum::{
    Extension, Router,
    extract::{Query, State},
    response::Json,
    routing::get,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState, app::iam::IamContext, domain::iam::permission, infra::traces::EdgeSnapshot,
    shared::Error,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/topology", get(topology))
}

#[derive(Debug, Deserialize)]
pub struct TopologyQuery {
    pub from: String, // RFC3339
    pub to: String,
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

fn parse_ts(s: &str) -> Result<i64, Error> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_micros())
        .map_err(|e| Error::invalid(format!("invalid timestamp {s}: {e}")))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn topology(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<TopologyQuery>,
) -> Result<Json<TopologyResponse>, Error> {
    let from = parse_ts(&q.from)?;
    let to = parse_ts(&q.to)?;
    if to < from {
        return Err(Error::invalid("`to` must be >= `from`"));
    }
    let snapshots = state
        .telemetry
        .service_graph
        .query(&ctx.org_id, from, to, None)
        .await?;
    let resp = aggregate(&snapshots, (to - from).max(1));
    Ok(Json(resp))
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

fn aggregate(snapshots: &[EdgeSnapshot], window_micros: i64) -> TopologyResponse {
    let window_secs = (window_micros as f64) / 1_000_000.0;
    let mut edges: HashMap<(String, String), EdgeAcc> = HashMap::new();
    let mut nodes: HashMap<String, NodeAcc> = HashMap::new();

    for s in snapshots {
        let key = (s.client_service.clone(), s.server_service.clone());
        let e = edges.entry(key).or_default();
        e.request_count += s.request_count;
        e.error_count += s.error_count;
        if let Some(p95) = s.p95_us {
            e.p95_us_max = e.p95_us_max.max(p95);
        }

        let n_client = nodes.entry(s.client_service.clone()).or_default();
        n_client.request_count += s.request_count;
        n_client.error_count += s.error_count;
        if let Some(p95) = s.p95_us {
            n_client.p95_us_max = n_client.p95_us_max.max(p95);
        }

        let n_server = nodes.entry(s.server_service.clone()).or_default();
        n_server.request_count += s.request_count;
        n_server.error_count += s.error_count;
        if let Some(p95) = s.p95_us {
            n_server.p95_us_max = n_server.p95_us_max.max(p95);
        }
    }

    let mut node_out: Vec<TopologyNode> = nodes
        .into_iter()
        .map(|(name, acc)| TopologyNode {
            id: name.clone(),
            name,
            error_rate: if acc.request_count == 0 {
                0.0
            } else {
                (acc.error_count as f64) / (acc.request_count as f64)
            },
            p95_ms: (acc.p95_us_max as f64) / 1_000.0,
            rps: (acc.request_count as f64) / window_secs,
            span_count: acc.request_count as i64,
        })
        .collect();
    node_out.sort_by(|a, b| a.name.cmp(&b.name));

    let mut edge_out: Vec<TopologyEdge> = edges
        .into_iter()
        .map(|((src, dst), acc)| TopologyEdge {
            source: src,
            target: dst,
            rps: (acc.request_count as f64) / window_secs,
            err_rate: if acc.request_count == 0 {
                0.0
            } else {
                (acc.error_count as f64) / (acc.request_count as f64)
            },
            p95_ms: (acc.p95_us_max as f64) / 1_000.0,
        })
        .collect();
    edge_out.sort_by(|a, b| a.source.cmp(&b.source).then(a.target.cmp(&b.target)));

    TopologyResponse {
        nodes: node_out,
        edges: edge_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::ids::Id;

    fn snap(client: &str, server: &str, req: u64, err: u64, p95: Option<i64>) -> EdgeSnapshot {
        EdgeSnapshot {
            id: Id::new(),
            org_id: Id("orgA".into()),
            client_service: client.into(),
            server_service: server.into(),
            bucket_at_micros: 0,
            request_count: req,
            error_count: err,
            p50_us: None,
            p95_us: p95,
            p99_us: None,
        }
    }

    #[test]
    fn aggregates_edges_and_nodes() {
        let snaps = vec![
            snap("web", "api", 100, 10, Some(5_000)),
            snap("web", "api", 50, 2, Some(8_000)),
            snap("api", "db", 80, 0, Some(2_000)),
        ];
        let r = aggregate(&snaps, 60_000_000);
        // 3 unique services
        assert_eq!(r.nodes.len(), 3);
        // 2 edges
        assert_eq!(r.edges.len(), 2);
        // web→api 折叠：150 req, 12 err, p95 max=8ms
        let we = r.edges.iter().find(|e| e.source == "web").unwrap();
        assert_eq!(we.target, "api");
        assert!((we.err_rate - 12.0 / 150.0).abs() < 1e-9);
        assert!((we.p95_ms - 8.0).abs() < 1e-9);
        // window=60s → rps = 150/60 = 2.5
        assert!((we.rps - 2.5).abs() < 1e-9);
    }

    #[test]
    fn empty_returns_empty() {
        let r = aggregate(&[], 60_000_000);
        assert!(r.nodes.is_empty());
        assert!(r.edges.is_empty());
    }
}
