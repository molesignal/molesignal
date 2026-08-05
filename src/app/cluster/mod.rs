// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集群上下文：节点发现 + 路由。
//!
//! 这是 app 层的端口（trait）。具体存储（cluster_nodes 表）+ gRPC 客户端实现在 infra。
//! 当前提供：
//! - [`PeerRole`] / [`PeerInfo`]：节点元信息（与 protocol::cluster::v1 同构但不直接依赖 proto）
//! - [`ClusterRegistry`] trait：`list_role` / `pick_querier` / `pick_ingester`
//! - [`StandaloneRegistry`]：standalone 模式的 trivial 实现，仅返回 self（permits Distributed
//!   engine 在单进程下的 fallback 路径）

use std::sync::Arc;

use async_trait::async_trait;

use crate::shared::ids::Id;

pub mod registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerRole {
    Standalone,
    Router,
    Ingester,
    Querier,
    Compactor,
    AlertManager,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: String,
    pub advertise_addr: String,
    pub roles: Vec<PeerRole>,
}

/// 集群成员注册表。
#[async_trait]
pub trait ClusterRegistry: Send + Sync {
    async fn list_role(&self, role: PeerRole) -> Vec<PeerInfo>;

    /// 返回全部活跃节点。旧实现可继续把 `Standalone` 视为“不按角色过滤”；
    /// Trace sampler ownership 使用此视图后再筛选真正暴露内部 gRPC listener 的节点。
    async fn list_all(&self) -> Vec<PeerInfo> {
        self.list_role(PeerRole::Standalone).await
    }

    /// 一致性哈希挑选 ingester（spec 8 / 11）。当前 standalone 直接返回 self。
    async fn pick_ingester(&self, _org_id: &Id, _stream: &str) -> Option<PeerInfo> {
        self.list_role(PeerRole::Ingester).await.into_iter().next()
    }

    /// 轮询挑选 querier；当前同 pick_ingester 简化。
    async fn pick_querier(&self) -> Option<PeerInfo> {
        self.list_role(PeerRole::Querier).await.into_iter().next()
    }
}

/// Standalone 模式的注册表：永远只返回自己（advertise_addr 由配置传入）。
pub struct StandaloneRegistry {
    pub self_info: PeerInfo,
}

impl StandaloneRegistry {
    pub fn new(advertise_addr: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            self_info: PeerInfo {
                node_id: "standalone".into(),
                advertise_addr: advertise_addr.into(),
                roles: vec![
                    PeerRole::Standalone,
                    PeerRole::Router,
                    PeerRole::Ingester,
                    PeerRole::Querier,
                    PeerRole::Compactor,
                    PeerRole::AlertManager,
                ],
            },
        })
    }
}

#[async_trait]
impl ClusterRegistry for StandaloneRegistry {
    async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
        vec![self.self_info.clone()]
    }
}
