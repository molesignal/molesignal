// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则跨集群汇总的拉取端：汇总集群是本集群的规则，每秒向联邦里映射了这个组织的各集群拉计数
//! （`cluster.v1.EventService/RealtimeCounts`），用这个组织在那个集群上的 per-org token（没有就用
//! 集群级 token），与事件推送相同。
//!
//! 每个集群最多等 [`COUNTS_TIMEOUT`]，各集群同时拉。拉不到的集群用它上一次答的计数顶上，最多
//! [`LAST_COUNTS_TTL`]，之后按 0 算：偶尔一秒没答上不会让进行中的事件解决。拉不到记指标，状态变了
//! 才记日志（每秒一条会淹掉日志）。本集群的 id、各组织要问的集群与 token 缓存 [`TARGETS_TTL`]。

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::future::join_all;
use parking_lot::Mutex;
use tonic::transport::Channel;

use super::{ClusterSecretRepository, RemoteClustersRepository, grpc_channel};
use crate::{
    app::alerting::{realtime::FederatedCounts, sources::FederationCluster},
    infra::{
        persistence::repositories::cluster::events::ClusterOrgLinkRepository,
        secret::resolve_secret_ref,
    },
    protocol::cluster::v1::{RealtimeCountsRequest, event_service_client::EventServiceClient},
    shared::ids::Id,
};

/// 每个集群最多等多久：汇总每秒一轮。
const COUNTS_TIMEOUT: Duration = Duration::from_millis(800);

/// 本集群的 id、各组织要问的集群与 token 多久重新读一次。
const TARGETS_TTL: Duration = Duration::from_secs(30);

/// 拉不到时，一个集群上一次答的计数最多顶多久。
const LAST_COUNTS_TTL: Duration = Duration::from_secs(30);

static FAILED: OnceLock<prometheus::IntCounterVec> = OnceLock::new();

/// `realtime_federated_counts_failed_total{cluster}`：拉不到别的集群计数的次数。
fn failed_vec() -> &'static prometheus::IntCounterVec {
    FAILED.get_or_init(|| {
        crate::shared::metrics::register_int_counter_vec(
            "realtime_federated_counts_failed_total",
            "real-time rule counts that could not be read from a federated cluster, by cluster",
            &["cluster"],
        )
    })
}

/// 读的时间和那时要问的集群。
type Targets = (Instant, Arc<Vec<Target>>);

/// 一个集群答的时间和它答的计数。
type Answer = (Instant, Vec<(Id, u64)>);

/// 要问的一个集群。
struct Target {
    cluster_id: String,
    address: String,
    tls_verify: bool,
    token: String,
}

pub struct GrpcFederatedCounts {
    cluster: Arc<dyn FederationCluster>,
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    org_links: Arc<dyn ClusterOrgLinkRepository>,
    secrets: Arc<dyn ClusterSecretRepository>,
    local: Mutex<Option<(Instant, Option<String>)>>,
    targets: Mutex<HashMap<Id, Targets>>,
    /// 每个地址一条复用的连接；拉失败就丢掉，下一轮重连。
    channels: Mutex<HashMap<String, Channel>>,
    /// 正拉不到的集群：状态变了才记日志。
    failing: Mutex<HashSet<String>>,
    /// 每个集群、组织上一次答的计数。
    last: Mutex<HashMap<(String, Id), Answer>>,
}

impl GrpcFederatedCounts {
    pub fn new(
        cluster: Arc<dyn FederationCluster>,
        remote_clusters: Arc<dyn RemoteClustersRepository>,
        org_links: Arc<dyn ClusterOrgLinkRepository>,
        secrets: Arc<dyn ClusterSecretRepository>,
    ) -> Self {
        Self {
            cluster,
            remote_clusters,
            org_links,
            secrets,
            local: Mutex::new(None),
            targets: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
            failing: Mutex::new(HashSet::new()),
            last: Mutex::new(HashMap::new()),
        }
    }

    /// 映射了这个组织的启用的远端集群，带上这个组织在那边的 token。
    async fn targets(&self, org_id: &Id, local: &str) -> Arc<Vec<Target>> {
        if let Some((read_at, targets)) = self.targets.lock().get(org_id)
            && read_at.elapsed() < TARGETS_TTL
        {
            return targets.clone();
        }

        let clusters = match self.remote_clusters.list_enabled().await {
            Ok(clusters) => clusters,
            Err(error) => {
                tracing::debug!(%error, "remote clusters could not be read for real-time counts");
                Vec::new()
            }
        };

        let mut targets = Vec::new();

        for cluster in clusters.into_iter().filter(|cluster| cluster.id.0 != local) {
            let link = match self.org_links.get_for_local(&cluster.id, org_id).await {
                Ok(Some(link)) => link,
                Ok(None) => continue,
                Err(error) => {
                    tracing::debug!(cluster = %cluster.id, %error, "organization link could not be read");
                    continue;
                }
            };

            // per-org token 优先；缺省回退集群级 token。
            let token_ref = link
                .token_secret_ref
                .filter(|reference| !reference.trim().is_empty())
                .unwrap_or_else(|| cluster.token_secret_ref.clone());

            match resolve_secret_ref(&token_ref, org_id, Some(self.secrets.as_ref())).await {
                Ok(token) => targets.push(Target {
                    cluster_id: cluster.id.0,
                    address: cluster.advertise_addr,
                    tls_verify: cluster.tls_verify,
                    token,
                }),
                Err(_) => tracing::debug!(
                    cluster = %cluster.id,
                    "the token for a federated cluster cannot be resolved"
                ),
            }
        }

        let targets = Arc::new(targets);

        self.targets
            .lock()
            .insert(org_id.clone(), (Instant::now(), targets.clone()));

        targets
    }

    async fn channel(&self, target: &Target) -> Result<Channel, String> {
        if let Some(channel) = self.channels.lock().get(&target.address) {
            return Ok(channel.clone());
        }

        let channel = grpc_channel::connect(&target.address, target.tls_verify).await?;

        self.channels
            .lock()
            .insert(target.address.clone(), channel.clone());

        Ok(channel)
    }

    async fn cluster_counts(
        &self,
        target: &Target,
        local: &str,
        org_id: &Id,
        rule_ids: &[Id],
    ) -> Result<Vec<(Id, u64)>, String> {
        let channel = self.channel(target).await?;

        let mut request = tonic::Request::new(RealtimeCountsRequest {
            source_cluster_id: local.to_string(),
            org_id: org_id.0.clone(),
            rule_ids: rule_ids.iter().map(|id| id.0.clone()).collect(),
        });
        request.set_timeout(COUNTS_TIMEOUT);
        grpc_channel::with_bearer(&mut request, &target.token)?;

        let reply = tokio::time::timeout(
            COUNTS_TIMEOUT,
            EventServiceClient::new(channel).realtime_counts(request),
        )
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|status| status.message().to_string())?;

        Ok(reply
            .into_inner()
            .counts
            .into_iter()
            .map(|count| (Id(count.rule_id), count.count))
            .collect())
    }

    /// 这一秒算进去的计数：答了就是它答的（记下来）；没答上就用上一次答的，过了
    /// [`LAST_COUNTS_TTL`] 就不算。
    fn answer_of(
        &self,
        target: &Target,
        org_id: &Id,
        reply: Result<Vec<(Id, u64)>, String>,
    ) -> Vec<(Id, u64)> {
        let key = (target.cluster_id.clone(), org_id.clone());

        match reply {
            Ok(counts) => {
                self.last
                    .lock()
                    .insert(key, (Instant::now(), counts.clone()));
                counts
            }
            Err(_) => self
                .last
                .lock()
                .get(&key)
                .filter(|(answered_at, _)| answered_at.elapsed() < LAST_COUNTS_TTL)
                .map(|(_, counts)| counts.clone())
                .unwrap_or_default(),
        }
    }

    /// 记下一个集群这一秒拉没拉到；状态变了才记日志。
    fn note(&self, target: &Target, outcome: &Result<Vec<(Id, u64)>, String>) {
        match outcome {
            Ok(_) => {
                if self.failing.lock().remove(&target.cluster_id) {
                    tracing::info!(
                        cluster = %target.cluster_id,
                        "real-time rule counts of a federated cluster are read again"
                    );
                }
            }
            Err(error) => {
                self.channels.lock().remove(&target.address);
                failed_vec().with_label_values(&[&target.cluster_id]).inc();

                if self.failing.lock().insert(target.cluster_id.clone()) {
                    tracing::warn!(
                        cluster = %target.cluster_id,
                        %error,
                        "real-time rule counts of a federated cluster could not be read; \
                         its last counts stand in for up to 30 s, then its share counts as 0"
                    );
                }
            }
        }
    }
}

#[async_trait]
impl FederatedCounts for GrpcFederatedCounts {
    async fn local_cluster(&self) -> Option<String> {
        if let Some((read_at, local)) = self.local.lock().as_ref()
            && read_at.elapsed() < TARGETS_TTL
        {
            return local.clone();
        }

        let local = match self.cluster.cluster_id().await {
            Ok(id) => Some(id.trim().to_string()).filter(|id| !id.is_empty()),
            Err(error) => {
                tracing::debug!(%error, "the cluster id could not be read");
                None
            }
        };

        *self.local.lock() = Some((Instant::now(), local.clone()));
        local
    }

    async fn remote_counts(&self, org_id: &Id, rule_ids: &[Id]) -> HashMap<Id, u64> {
        let mut totals: HashMap<Id, u64> = HashMap::new();

        let Some(local) = self.local_cluster().await else {
            return totals;
        };

        let targets = self.targets(org_id, &local).await;

        let replies = join_all(
            targets
                .iter()
                .map(|target| self.cluster_counts(target, &local, org_id, rule_ids)),
        )
        .await;

        for (target, reply) in targets.iter().zip(replies) {
            self.note(target, &reply);

            for (id, count) in self.answer_of(target, org_id, reply) {
                *totals.entry(id).or_default() += count;
            }
        }

        totals
    }
}
