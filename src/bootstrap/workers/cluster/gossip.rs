// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! gossip 节点发现 worker（#12 super-cluster leader）。
//!
//! 单点周期任务（alert_manager 角色）。每 tick 与每个 **enabled** peer 交换"已知集群"
//! 拓扑（仅 id/name/addr，**绝不**传 token / org 映射）：对端回传的未知集群入库为
//! `discovered=true, enabled=false` —— admin 在 clusters 页看到后手动启用 + 填 token +
//! 配 org_map 才生效（自动发现拓扑、不自动信任）。
//!
//! 周期 `federation_gossip_interval_secs` 从 `instance_settings`（DB）热读；联邦未启用
//! （cluster_id 空）→ 每 tick 立即返回，零开销。token best-effort（`env:` 即用；
//! `cipher_keys:` 因 worker 无 org 上下文解析失败则跳过该 peer，拓扑仍经其它路径传播）。

use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::iam::InstanceSettingsRepository,
    infra::{
        cluster::{
            cluster_secrets_repo::ClusterSecretRepository, grpc_channel,
            remote_clusters_repo::RemoteClustersRepository,
        },
        persistence::repositories::cluster::events::ClusterOrgLinkRepository,
        secret::resolve_cluster_control_token,
    },
    protocol::cluster::v1::{ClusterRef, GossipRequest, event_service_client::EventServiceClient},
    shared::ids::Id,
};

pub struct ClusterGossip {
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    org_links: Arc<dyn ClusterOrgLinkRepository>,
    secrets: Arc<dyn ClusterSecretRepository>,
}

impl ClusterGossip {
    pub fn new(
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        remote_clusters: Arc<dyn RemoteClustersRepository>,
        org_links: Arc<dyn ClusterOrgLinkRepository>,
        secrets: Arc<dyn ClusterSecretRepository>,
    ) -> Self {
        Self {
            instance_settings,
            remote_clusters,
            org_links,
            secrets,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = self
                .instance_settings
                .get()
                .await
                .map(|s| s.federation_gossip_interval_secs)
                .unwrap_or(60)
                .max(1) as u64;
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let settings = match self.instance_settings.get().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "gossip: read settings failed");
                        continue;
                    }
                };
                interval = settings.federation_gossip_interval_secs.max(1) as u64;
                if !settings.federation_enabled() {
                    continue;
                }
                let self_id = settings.federation_cluster_id.trim().to_string();
                self.gossip_round(&self_id).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.cluster_gossip",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "cluster_gossip")
    )]
    async fn gossip_round(&self, self_id: &str) {
        let mine = self.remote_clusters.list().await.unwrap_or_default();
        // 本端已知拓扑（仅 id/name/addr）。
        let known_refs: Vec<ClusterRef> = mine
            .iter()
            .filter(|c| !c.id.0.is_empty())
            .map(|c| ClusterRef {
                id: c.id.0.clone(),
                name: c.name.clone(),
                advertise_addr: c.advertise_addr.clone(),
            })
            .collect();
        let known_ids: HashSet<String> = mine.iter().map(|c| c.id.0.clone()).collect();

        for peer in mine.iter().filter(|c| c.enabled) {
            // token 经 per-org link 解析（cluster 控制 RPC 用任一有 org 上下文的 token；
            // cipher_keys: 因此可解，不再被无脑跳过）。
            let per_org: Vec<(Id, Option<String>)> = self
                .org_links
                .list(&peer.id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|l| (l.local_org_id, l.token_secret_ref))
                .collect();
            let Some(token) = resolve_cluster_control_token(
                &peer.token_secret_ref,
                &per_org,
                Some(self.secrets.as_ref()),
            )
            .await
            else {
                continue; // 无任何可解析 token → 跳过该 peer。
            };
            let Ok(channel) = grpc_channel::connect(&peer.advertise_addr, peer.tls_verify).await
            else {
                continue;
            };
            let mut req = tonic::Request::new(GossipRequest {
                known: known_refs.clone(),
            });
            if grpc_channel::with_bearer(&mut req, &token).is_err() {
                continue;
            }
            let mut client = EventServiceClient::new(channel);
            let resp = match crate::shared::grpc_trace::call(
                req,
                "cluster.v1.EventService",
                "GossipClusters",
                crate::shared::grpc_trace::GrpcTarget::Internal,
                |request| client.gossip_clusters(request),
            )
            .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    tracing::debug!(peer = %peer.name, error = %e.message(), "gossip exchange failed");
                    continue;
                }
            };
            // 合并对端拓扑：未知集群（排除自身）入库为 discovered。
            for r in resp.known {
                if r.id.is_empty() || r.id == self_id || known_ids.contains(&r.id) {
                    continue;
                }
                if let Ok(n) = self
                    .remote_clusters
                    .insert_discovered(&r.id, &r.name, &r.advertise_addr)
                    .await
                    && n > 0
                {
                    tracing::info!(discovered = %r.id, name = %r.name, "gossip discovered new cluster (disabled until configured)");
                }
            }
        }
    }
}
