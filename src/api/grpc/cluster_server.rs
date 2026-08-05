// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster.v1.NodeService` 的 gRPC server 实装。
//!
//! - Heartbeat：把 `(node_id, role, advertise_addr, ts)` upsert 到 `cluster_nodes`。
//! - List：可选按 roles 过滤，返当前活跃节点（last_heartbeat ≥ now - 15s）。

use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::{
    app::cluster::PeerRole,
    infra::persistence::repositories::cluster::nodes::ClusterNodesRepository,
    protocol::cluster::v1::{
        HeartbeatRequest, HeartbeatResponse, ListNodesRequest, ListNodesResponse, NodeInfo,
        NodeRole,
        node_service_server::{NodeService, NodeServiceServer},
    },
    shared::time::TimestampMicros,
};

pub struct ClusterGrpc {
    repo: Arc<dyn ClusterNodesRepository>,
    alive_window_secs: i64,
}

impl ClusterGrpc {
    pub fn new(repo: Arc<dyn ClusterNodesRepository>, alive_window_secs: i64) -> Self {
        Self {
            repo,
            alive_window_secs,
        }
    }

    pub fn into_server(self) -> NodeServiceServer<Self> {
        NodeServiceServer::new(self)
    }
}

fn proto_role_to_app(v: i32) -> PeerRole {
    match NodeRole::try_from(v).unwrap_or(NodeRole::Unspecified) {
        NodeRole::Router => PeerRole::Router,
        NodeRole::Ingester => PeerRole::Ingester,
        NodeRole::Querier => PeerRole::Querier,
        NodeRole::Compactor => PeerRole::Compactor,
        NodeRole::AlertManager => PeerRole::AlertManager,
        _ => PeerRole::Standalone,
    }
}

fn app_role_to_proto(r: PeerRole) -> NodeRole {
    match r {
        PeerRole::Standalone => NodeRole::Standalone,
        PeerRole::Router => NodeRole::Router,
        PeerRole::Ingester => NodeRole::Ingester,
        PeerRole::Querier => NodeRole::Querier,
        PeerRole::Compactor => NodeRole::Compactor,
        PeerRole::AlertManager => NodeRole::AlertManager,
    }
}

#[tonic::async_trait]
impl NodeService for ClusterGrpc {
    async fn heartbeat(
        &self,
        req: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let HeartbeatRequest { node, ts_micros } = req.into_inner();
        let node = node.ok_or_else(|| Status::invalid_argument("node missing"))?;
        // 一节点可有多个角色：整组存库，list_role 按成员匹配命中。
        let mut roles: Vec<PeerRole> = node.roles.iter().copied().map(proto_role_to_app).collect();
        if roles.is_empty() {
            roles.push(PeerRole::Standalone);
        }
        let ts = if ts_micros == 0 {
            TimestampMicros::now()
        } else {
            TimestampMicros(ts_micros)
        };
        self.repo
            .upsert(&node.node_id, &roles, &node.advertise_addr, ts)
            .await
            .map_err(|e| Status::internal(format!("upsert: {e}")))?;
        Ok(Response::new(HeartbeatResponse {
            server_ts_micros: TimestampMicros::now().0,
        }))
    }

    async fn list(
        &self,
        req: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let ListNodesRequest { roles } = req.into_inner();
        let since = TimestampMicros(TimestampMicros::now().0 - self.alive_window_secs * 1_000_000);
        let rows = self
            .repo
            .list_alive(since)
            .await
            .map_err(|e| Status::internal(format!("list_alive: {e}")))?;
        let role_filter: Option<std::collections::HashSet<PeerRole>> = if roles.is_empty() {
            None
        } else {
            Some(roles.into_iter().map(proto_role_to_app).collect())
        };
        let nodes: Vec<NodeInfo> = rows
            .into_iter()
            .filter(|r| {
                role_filter
                    .as_ref()
                    .map(|s| r.roles.iter().any(|role| s.contains(role)))
                    .unwrap_or(true)
            })
            .map(|r| NodeInfo {
                node_id: r.node_id,
                advertise_addr: r.advertise_addr,
                roles: r
                    .roles
                    .iter()
                    .map(|role| app_role_to_proto(*role) as i32)
                    .collect(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })
            .collect();
        Ok(Response::new(ListNodesResponse { nodes }))
    }
}
