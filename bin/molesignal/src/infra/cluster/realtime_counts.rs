// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的集群计数：向各个接收写入的节点（intake、standalone 角色）拉窗口内的命中数并求和
//! （`alerting.v1.RealtimeService/Counts`）；本节点也接收写入时直接读本地的计数。
//!
//! 拉不到的节点记日志，不让一个节点拖住整轮：每个节点最多等 [`COUNTS_TIMEOUT`]，各节点同时拉。
//! 接上快照仓库时（[`ClusterRealtimeCounts::with_snapshots`]），拉不到的节点、已经不在成员视图里的
//! 节点用它最近存的快照里还在窗口内的格子顶上（快照最多晚 5 秒）；没接时少它那一份。没配集群
//! bearer 时别的节点都只能靠快照。快照过了 [`SNAPSHOTS_TTL`] 先用旧的、后台重读：联邦里的汇总集群
//! 每秒来拉、最多等 800 毫秒，不能等这里查库。

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::future::join_all;
use parking_lot::Mutex;
use tonic::transport::Channel;

use super::grpc_channel;
use crate::{
    app::{
        alerting::realtime::{
            INTERNAL_ORIGIN_HEADER, REALTIME_COUNTS_ORIGIN, RealtimeAlerts, RealtimeCounts,
        },
        cluster::{ClusterRegistry, PeerRole},
    },
    domain::{
        alerting::realtime_snapshot::{CountSnapshot, RealtimeSnapshotRepository},
        cluster::PeerInfo,
    },
    protocol::alerting::v1::{
        RealtimeServiceCountsRequest, realtime_service_client::RealtimeServiceClient,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 每个节点最多等多久：汇总每秒一轮。
const COUNTS_TIMEOUT: Duration = Duration::from_millis(800);

/// 快照多久重新读一次：节点每 5 秒才存一份。
const SNAPSHOTS_TTL: Duration = Duration::from_secs(5);

pub struct ClusterRealtimeCounts {
    registry: Arc<dyn ClusterRegistry>,
    local_node_id: String,
    /// 本节点也接收写入时的本地计数。
    local: Option<Arc<RealtimeAlerts>>,
    cluster_token: Option<Arc<str>>,
    /// 每个节点一条复用的连接；拉失败就丢掉，下一轮重连。
    channels: Mutex<HashMap<String, Channel>>,
    snapshots: Option<Arc<dyn RealtimeSnapshotRepository>>,
    /// 最近读到的快照：读的时间、读了哪些规则。
    saved: Arc<Mutex<Option<Saved>>>,
    /// 正在后台重读快照。
    refreshing: Arc<AtomicBool>,
}

struct Saved {
    read_at: Instant,
    rule_ids: HashSet<Id>,
    snapshots: Arc<Vec<CountSnapshot>>,
}

impl ClusterRealtimeCounts {
    pub fn new(
        registry: Arc<dyn ClusterRegistry>,
        local_node_id: String,
        local: Option<Arc<RealtimeAlerts>>,
        cluster_token: Option<Arc<str>>,
    ) -> Self {
        Self {
            registry,
            local_node_id,
            local,
            cluster_token,
            channels: Mutex::new(HashMap::new()),
            snapshots: None,
            saved: Arc::new(Mutex::new(None)),
            refreshing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 拉不到的节点用它最近存的快照顶上。
    pub fn with_snapshots(mut self, snapshots: Arc<dyn RealtimeSnapshotRepository>) -> Self {
        self.snapshots = Some(snapshots);
        self
    }

    /// 别的接收写入的节点。
    async fn peers(&self) -> Vec<PeerInfo> {
        self.registry
            .list_all()
            .await
            .into_iter()
            .filter(|peer| peer.node_id != self.local_node_id)
            .filter(|peer| {
                peer.roles
                    .iter()
                    .any(|role| matches!(role, PeerRole::Intake | PeerRole::Standalone))
            })
            .collect()
    }

    async fn channel(&self, address: &str) -> Result<Channel, String> {
        if let Some(channel) = self.channels.lock().get(address) {
            return Ok(channel.clone());
        }

        let channel = grpc_channel::connect(address, false).await?;

        self.channels
            .lock()
            .insert(address.to_owned(), channel.clone());

        Ok(channel)
    }

    async fn peer_counts(
        &self,
        peer: &PeerInfo,
        rule_ids: &[Id],
        token: &str,
    ) -> Result<Vec<(Id, u64)>, String> {
        let channel = self.channel(&peer.advertise_addr).await?;

        let mut request = tonic::Request::new(RealtimeServiceCountsRequest {
            rule_ids: rule_ids.iter().map(|id| id.0.clone()).collect(),
        });
        request.set_timeout(COUNTS_TIMEOUT);
        grpc_channel::with_bearer(&mut request, token)?;
        request.metadata_mut().insert(
            INTERNAL_ORIGIN_HEADER,
            REALTIME_COUNTS_ORIGIN
                .parse()
                .map_err(|_| "invalid origin header".to_string())?,
        );

        let reply = tokio::time::timeout(
            COUNTS_TIMEOUT,
            RealtimeServiceClient::new(channel).counts(request),
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
}

#[async_trait]
impl RealtimeCounts for ClusterRealtimeCounts {
    async fn counts(&self, rule_ids: &[Id], now: TimestampMicros) -> HashMap<Id, u64> {
        let mut totals: HashMap<Id, u64> = HashMap::new();
        let mut answered = HashSet::from([self.local_node_id.clone()]);

        if let Some(local) = &self.local {
            for (id, count) in local.counts(rule_ids, now) {
                *totals.entry(id).or_default() += count;
            }
        }

        let peers = self.peers().await;

        if !peers.is_empty() {
            match self.cluster_token.as_deref() {
                Some(token) => {
                    self.add_peers(&mut totals, &mut answered, &peers, rule_ids, token)
                        .await;
                }
                None => tracing::warn!(
                    peers = peers.len(),
                    "real-time rules cannot read the counts of other nodes: the cluster bearer \
                     is not configured"
                ),
            }
        }

        self.add_snapshots(&mut totals, &answered, rule_ids, now)
            .await;

        totals
    }
}

impl ClusterRealtimeCounts {
    /// 各节点同时拉；拉到的记进 `answered`。
    async fn add_peers(
        &self,
        totals: &mut HashMap<Id, u64>,
        answered: &mut HashSet<String>,
        peers: &[PeerInfo],
        rule_ids: &[Id],
        token: &str,
    ) {
        let replies = join_all(
            peers
                .iter()
                .map(|peer| self.peer_counts(peer, rule_ids, token)),
        )
        .await;

        for (peer, reply) in peers.iter().zip(replies) {
            match reply {
                Ok(counts) => {
                    answered.insert(peer.node_id.clone());

                    for (id, count) in counts {
                        *totals.entry(id).or_default() += count;
                    }
                }
                Err(error) => {
                    self.channels.lock().remove(&peer.advertise_addr);
                    tracing::warn!(
                        node_id = %peer.node_id,
                        %error,
                        "real-time rule counts of a node could not be read; its last snapshot \
                         stands in"
                    );
                }
            }
        }
    }

    /// 没答上来的节点（拉不到、已经不在了）用它最近的快照里还在窗口内的格子。
    async fn add_snapshots(
        &self,
        totals: &mut HashMap<Id, u64>,
        answered: &HashSet<String>,
        rule_ids: &[Id],
        now: TimestampMicros,
    ) {
        let Some(saved) = self.saved_snapshots(rule_ids).await else {
            return;
        };

        let wanted: HashSet<&Id> = rule_ids.iter().collect();

        for snapshot in saved.iter().filter(|snapshot| {
            !answered.contains(&snapshot.node_id) && wanted.contains(&snapshot.rule_id)
        }) {
            let count = snapshot.count_at(now);

            if count > 0 {
                *totals.entry(snapshot.rule_id.clone()).or_default() += count;
            }
        }
    }

    async fn saved_snapshots(&self, rule_ids: &[Id]) -> Option<Arc<Vec<CountSnapshot>>> {
        let repository = self.snapshots.as_ref()?;

        // 汇总和联邦里来拉的问的规则不一样：读过这些规则才用缓存；过了期先照旧用，后台重读。
        let cached = self.saved.lock().as_ref().and_then(|saved| {
            rule_ids
                .iter()
                .all(|id| saved.rule_ids.contains(id))
                .then(|| {
                    (
                        saved.read_at,
                        saved.rule_ids.clone(),
                        saved.snapshots.clone(),
                    )
                })
        });

        if let Some((read_at, covered, snapshots)) = cached {
            if read_at.elapsed() >= SNAPSHOTS_TTL {
                self.refresh(repository.clone(), covered);
            }

            return Some(snapshots);
        }

        // 新问到的规则现读，连同读过的一起，两边再来问都用得上。
        let mut wanted: HashSet<Id> = self
            .saved
            .lock()
            .as_ref()
            .map(|saved| saved.rule_ids.clone())
            .unwrap_or_default();
        wanted.extend(rule_ids.iter().cloned());

        match load(repository.as_ref(), &wanted).await {
            Ok(snapshots) => {
                *self.saved.lock() = Some(Saved {
                    read_at: Instant::now(),
                    rule_ids: wanted,
                    snapshots: snapshots.clone(),
                });

                Some(snapshots)
            }
            Err(error) => {
                tracing::debug!(%error, "real-time rule snapshots could not be read");
                None
            }
        }
    }

    /// 后台重读这些规则的快照；正在读时不再起一份。
    fn refresh(&self, repository: Arc<dyn RealtimeSnapshotRepository>, rule_ids: HashSet<Id>) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }

        let (saved, refreshing) = (self.saved.clone(), self.refreshing.clone());

        tokio::spawn(async move {
            match load(repository.as_ref(), &rule_ids).await {
                Ok(snapshots) => {
                    *saved.lock() = Some(Saved {
                        read_at: Instant::now(),
                        rule_ids,
                        snapshots,
                    });
                }
                Err(error) => {
                    tracing::debug!(%error, "real-time rule snapshots could not be read again")
                }
            }

            refreshing.store(false, Ordering::Release);
        });
    }
}

async fn load(
    repository: &dyn RealtimeSnapshotRepository,
    rule_ids: &HashSet<Id>,
) -> crate::shared::Result<Arc<Vec<CountSnapshot>>> {
    let rule_ids: Vec<Id> = rule_ids.iter().cloned().collect();
    Ok(Arc::new(repository.load_rules(&rule_ids).await?))
}
