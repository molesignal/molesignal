// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cluster discovery port and peer metadata.

use std::sync::Arc;

use async_trait::async_trait;

use crate::shared::ids::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerRole {
    Standalone,
    Router,
    Intake,
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

#[async_trait]
pub trait ClusterRegistry: Send + Sync {
    async fn list_role(&self, role: PeerRole) -> Vec<PeerInfo>;

    async fn list_all(&self) -> Vec<PeerInfo> {
        self.list_role(PeerRole::Standalone).await
    }

    async fn pick_intake(&self, _org_id: &Id, _stream: &str) -> Option<PeerInfo> {
        self.list_role(PeerRole::Intake).await.into_iter().next()
    }

    async fn pick_querier(&self) -> Option<PeerInfo> {
        self.list_role(PeerRole::Querier).await.into_iter().next()
    }
}

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
                    PeerRole::Intake,
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
