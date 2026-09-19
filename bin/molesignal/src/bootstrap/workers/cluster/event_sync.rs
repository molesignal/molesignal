// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群事件总线的**发送端** worker：周期把本地 outbox 里未投递的 CloudEvent
//! 推到配了 org 映射的远端集群。
//!
//! 单点周期任务（alert_manager 角色，只起一份）。每 tick：
//! 1. 遍历每个 enabled remote cluster C：按 C 的 org 映射取本地可发 org 的未投递事件
//!    （`acked_seq` 游标之后），**按 org 分组、各用该 org 的 per-org token** 经
//!    [`EventServiceClient`] 推过去；全组成功才推进 C 的投递游标（部分失败下 tick 重投，
//!    接收端按 id 去重，重投幂等）。
//! 2. `prune`：outbox 删到「所有 enabled 集群都已确认」的最小游标。
//! 3. `sweep`：清理接收端去重表里过期记录（TTL 窗口）。
//!
//! 调参（drain 周期 / 批量 / 去重 TTL）从 `instance_settings`（DB）读，**改即热生效**。
//! 联邦未启用（cluster_id 空）→ 每 tick 立即返回，零开销。

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, OnceLock},
    time::Duration,
};

use tokio::task::JoinHandle;

use crate::{
    domain::iam::InstanceSettingsRepository,
    infra::{
        cluster::{
            cluster_secrets_repo::ClusterSecretRepository,
            grpc_channel,
            remote_clusters_repo::{RemoteCluster, RemoteClustersRepository},
        },
        persistence::repositories::cluster::events::{
            ClusterEventOutboxRepository, ClusterOrgLinkRepository, OrgLink, OutboxRow,
            SeenEventsRepository,
        },
        secret::resolve_secret_ref,
    },
    protocol::cluster::v1::{PushEventsRequest, event_service_client::EventServiceClient},
    shared::{Result, ids::Id, time::TimestampMicros},
};

static OUTBOX_LAG: OnceLock<prometheus::IntGaugeVec> = OnceLock::new();
static PUSHED: OnceLock<prometheus::IntCounter> = OnceLock::new();
static PUSH_ERRORS: OnceLock<prometheus::IntCounterVec> = OnceLock::new();

/// `federation_outbox_lag{cluster}`：某集群 `max_seq - acked_seq`（待投递积压）。
fn outbox_lag() -> &'static prometheus::IntGaugeVec {
    OUTBOX_LAG.get_or_init(|| {
        crate::shared::metrics::register_int_gauge_vec(
            "federation_outbox_lag",
            "undelivered cross-cluster events per remote cluster",
            &["cluster"],
        )
    })
}

/// `federation_events_pushed_total`：累计推送出去的事件数。
fn pushed_total() -> &'static prometheus::IntCounter {
    PUSHED.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "federation_events_pushed_total",
            "cross-cluster events pushed to remote clusters",
        )
    })
}

/// `federation_push_errors_total{cluster}`：推送失败次数（软降级，下 tick 重试）。
fn push_errors() -> &'static prometheus::IntCounterVec {
    PUSH_ERRORS.get_or_init(|| {
        crate::shared::metrics::register_int_counter_vec(
            "federation_push_errors_total",
            "cross-cluster event push failures per remote cluster",
            &["cluster"],
        )
    })
}

pub struct ClusterEventSync {
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    org_links: Arc<dyn ClusterOrgLinkRepository>,
    outbox: Arc<dyn ClusterEventOutboxRepository>,
    seen_events: Arc<dyn SeenEventsRepository>,
    secrets: Arc<dyn ClusterSecretRepository>,
    max_message_size: usize,
}

impl ClusterEventSync {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        remote_clusters: Arc<dyn RemoteClustersRepository>,
        org_links: Arc<dyn ClusterOrgLinkRepository>,
        outbox: Arc<dyn ClusterEventOutboxRepository>,
        seen_events: Arc<dyn SeenEventsRepository>,
        secrets: Arc<dyn ClusterSecretRepository>,
        max_message_size: usize,
    ) -> Self {
        Self {
            instance_settings,
            remote_clusters,
            org_links,
            outbox,
            seen_events,
            secrets,
            max_message_size,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            // 初始周期：读一次 DB 设置；读不到退回 10s。
            let mut interval = self
                .instance_settings
                .get()
                .await
                .map(|s| s.federation_drain_interval_secs)
                .unwrap_or(10)
                .max(1) as u64;
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let settings = match self.instance_settings.get().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "federation sync: read settings failed");
                        continue;
                    }
                };
                interval = settings.federation_drain_interval_secs.max(1) as u64;
                if !settings.federation_enabled() {
                    continue; // 未启用 → 零开销。
                }
                let source = settings.federation_cluster_id.trim().to_string();
                if let Err(e) = self
                    .tick(
                        &source,
                        settings.federation_push_batch_size.max(1),
                        settings.federation_seen_events_ttl_secs.max(1),
                    )
                    .await
                {
                    tracing::warn!(error = %e, "federation sync tick failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.cluster_event_sync",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "cluster_event_sync")
    )]
    async fn tick(&self, source_cluster: &str, batch: i64, ttl_secs: i64) -> Result<()> {
        let clusters = self.remote_clusters.list_enabled().await?;
        let max_seq = self.outbox.max_seq().await?;
        let mut min_acked = i64::MAX;
        for c in &clusters {
            let acked = self.outbox.acked_seq(&c.id).await?;
            outbox_lag()
                .with_label_values(&[c.name.as_str()])
                .set((max_seq - acked).max(0));
            let links = self.org_links.list(&c.id).await?;
            if links.is_empty() {
                min_acked = min_acked.min(acked);
                continue; // 无 org 映射 → 不发；不阻塞 prune。
            }
            let orgs: Vec<String> = links.iter().map(|l| l.local_org_id.0.clone()).collect();
            let events = self.outbox.list_undelivered(acked, &orgs, batch).await?;
            if events.is_empty() {
                min_acked = min_acked.min(acked);
                continue;
            }
            match self.push_cluster(source_cluster, c, &links, &events).await {
                Ok(new_acked) => {
                    self.outbox.ack(&c.id, new_acked).await?;
                    min_acked = min_acked.min(new_acked);
                }
                Err(reason) => {
                    push_errors().with_label_values(&[c.name.as_str()]).inc();
                    tracing::warn!(cluster = %c.name, reason = %reason, "federation push failed; retry next tick");
                    min_acked = min_acked.min(acked); // 不推进 → 下 tick 重投。
                }
            }
        }
        // prune：删到所有 enabled 集群都已确认的最小游标（无集群则不删）。
        if min_acked != i64::MAX && min_acked > 0 {
            let _ = self.outbox.prune(min_acked).await?;
        }
        // sweep 接收端去重表过期记录。
        let cutoff = TimestampMicros(TimestampMicros::now().0 - ttl_secs * 1_000_000);
        let _ = self.seen_events.sweep(cutoff).await?;
        Ok(())
    }

    /// 把 cluster C 的一批事件按 org 分组、各用该 org 的 per-org token 推过去。
    /// 返回**安全推进到的游标**：无瞬时重试则到本批最大 seq；有瞬时失败（接收端 `retry_ids`）
    /// 则只推进到首个待重投事件之前（其余已确认的更高 seq 下 tick 重投、接收端按 id 去重幂等）。
    /// 任一组 RPC 整体失败 → Err（不推进游标，整批下 tick 重投）。
    async fn push_cluster(
        &self,
        source_cluster: &str,
        c: &RemoteCluster,
        links: &[OrgLink],
        events: &[OutboxRow],
    ) -> std::result::Result<i64, String> {
        // 按 org 分组，组内保持 seq 升序（events 已按 seq 排序）。
        let mut by_org: BTreeMap<String, Vec<&OutboxRow>> = BTreeMap::new();
        for row in events {
            // outbox 行 org_id 即本地 org（emit 时落的）。subject 也带同一 org。
            let org = row
                .event
                .subject
                .split_once('/')
                .map(|(o, _)| o.to_string())
                .unwrap_or_default();
            by_org.entry(org).or_default().push(row);
        }
        // channel 复用：同一集群的各 org 推送共用一条连接。
        let channel = grpc_channel::connect(&c.advertise_addr, c.tls_verify).await?;
        let mut retry_ids: HashSet<String> = HashSet::new();
        for (org, rows) in by_org {
            // per-org token 优先；缺省回退集群级 token。
            let link = links.iter().find(|l| l.local_org_id.0 == org);
            let token_ref = link
                .and_then(|l| l.token_secret_ref.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| c.token_secret_ref.clone());
            let token =
                resolve_secret_ref(&token_ref, &Id(org.clone()), Some(self.secrets.as_ref()))
                    .await
                    .map_err(|_| "token resolution failed".to_string())?;
            let mut events_json = Vec::with_capacity(rows.len());
            for r in &rows {
                let bytes =
                    serde_json::to_vec(&r.event).map_err(|e| format!("event serialize: {e}"))?;
                events_json.push(bytes.into());
            }
            let mut req = tonic::Request::new(PushEventsRequest {
                source_cluster_id: source_cluster.to_string(),
                events_json,
            });
            grpc_channel::with_bearer(&mut req, &token)?;
            let mut client = EventServiceClient::new(channel.clone())
                .max_encoding_message_size(self.max_message_size);
            let resp = crate::shared::grpc_trace::call(
                req,
                "cluster.v1.EventService",
                "PushEvents",
                crate::shared::grpc_trace::GrpcTarget::Internal,
                |request| client.push_events(request),
            )
            .await
            .map_err(|s| format!("push_events: {}", s.message()))?
            .into_inner();
            pushed_total().inc_by(rows.len() as u64);
            for rid in resp.retry_ids {
                retry_ids.insert(rid);
            }
        }
        // 游标：有瞬时重试 → 推进到首个待重投事件之前；否则全批已确认到最大 seq。
        let max_seq = events.iter().map(|r| r.seq).max().unwrap_or(0);
        let cursor = if retry_ids.is_empty() {
            max_seq
        } else {
            events
                .iter()
                .filter(|r| retry_ids.contains(&r.event.id))
                .map(|r| r.seq)
                .min()
                .map(|min_retry| min_retry - 1)
                .unwrap_or(max_seq)
        };
        Ok(cursor)
    }
}
