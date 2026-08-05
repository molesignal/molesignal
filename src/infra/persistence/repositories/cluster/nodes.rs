// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster_nodes` 表的 Pg 实装。
//!
//! 用法：每个 role 启动期挂 HeartbeatTask 周期 `upsert`；router / querier
//! 经 `list_alive` 取最近 15s 内仍活跃的节点；后台 sweeper 每 60s `sweep_stale`
//! 把超 5min 的脏数据清掉。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use tokio::sync::Mutex;

use super::super::sqlx_err;
use crate::{
    app::cluster::{ClusterRegistry, PeerInfo, PeerRole},
    shared::{Result, ids::Id, time::TimestampMicros},
};

const NODE_DISCOVERY_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct ClusterNodeRow {
    pub node_id: String,
    /// 节点承担的角色集（逗号分隔存库；标准单角色节点即一个元素）。
    pub roles: Vec<PeerRole>,
    pub advertise_addr: String,
    pub started_at: TimestampMicros,
    pub last_heartbeat_at: TimestampMicros,
}

#[async_trait]
pub trait ClusterNodesRepository: Send + Sync {
    async fn upsert(
        &self,
        node_id: &str,
        roles: &[PeerRole],
        advertise_addr: &str,
        ts: TimestampMicros,
    ) -> Result<()>;
    async fn list_alive(&self, since: TimestampMicros) -> Result<Vec<ClusterNodeRow>>;
    async fn sweep_stale(&self, older_than: TimestampMicros) -> Result<u64>;
    /// 主动注销本节点行（drain 退役时调用）；各进程最迟在 2 秒节点发现缓存过期后摘除。
    async fn delete(&self, node_id: &str) -> Result<()>;
}

pub struct PgClusterNodesRepository {
    pool: PgPool,
}

impl PgClusterNodesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn peer_role_to_str(r: PeerRole) -> &'static str {
    match r {
        PeerRole::Standalone => "standalone",
        PeerRole::Router => "router",
        PeerRole::Ingester => "ingester",
        PeerRole::Querier => "querier",
        PeerRole::Compactor => "compactor",
        PeerRole::AlertManager => "alert_manager",
    }
}

fn peer_role_from_str(s: &str) -> PeerRole {
    match s {
        "router" => PeerRole::Router,
        "ingester" => PeerRole::Ingester,
        "querier" => PeerRole::Querier,
        "compactor" => PeerRole::Compactor,
        "alert_manager" => PeerRole::AlertManager,
        _ => PeerRole::Standalone,
    }
}

/// 角色集 → 逗号分隔串入库。
fn roles_to_csv(roles: &[PeerRole]) -> String {
    roles
        .iter()
        .map(|r| peer_role_to_str(*r))
        .collect::<Vec<_>>()
        .join(",")
}

/// 逗号分隔串 → 角色集；忽略空段，空集兜底成 `[Standalone]`。
fn roles_from_csv(s: &str) -> Vec<PeerRole> {
    let roles: Vec<PeerRole> = s
        .split(',')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .map(peer_role_from_str)
        .collect();
    if roles.is_empty() {
        vec![PeerRole::Standalone]
    } else {
        roles
    }
}

#[async_trait]
impl ClusterNodesRepository for PgClusterNodesRepository {
    async fn upsert(
        &self,
        node_id: &str,
        roles: &[PeerRole],
        advertise_addr: &str,
        ts: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO cluster_nodes
                (node_id, role, advertise_addr, started_at_micros, last_heartbeat_at_micros)
             VALUES ($1, $2, $3, $4, $4)
             ON CONFLICT (node_id) DO UPDATE
             SET role = EXCLUDED.role,
                 advertise_addr = EXCLUDED.advertise_addr,
                 last_heartbeat_at_micros = EXCLUDED.last_heartbeat_at_micros",
        )
        .bind(node_id)
        .bind(roles_to_csv(roles))
        .bind(advertise_addr)
        .bind(ts.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_alive(&self, since: TimestampMicros) -> Result<Vec<ClusterNodeRow>> {
        let rows = sqlx::query(
            "SELECT node_id, role, advertise_addr, started_at_micros, last_heartbeat_at_micros
             FROM cluster_nodes
             WHERE last_heartbeat_at_micros >= $1",
        )
        .bind(since.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ClusterNodeRow {
                node_id: r.try_get::<String, _>("node_id").unwrap_or_default(),
                roles: roles_from_csv(&r.try_get::<String, _>("role").unwrap_or_default()),
                advertise_addr: r.try_get::<String, _>("advertise_addr").unwrap_or_default(),
                started_at: TimestampMicros(r.try_get("started_at_micros").unwrap_or_default()),
                last_heartbeat_at: TimestampMicros(
                    r.try_get("last_heartbeat_at_micros").unwrap_or_default(),
                ),
            })
            .collect())
    }

    async fn sweep_stale(&self, older_than: TimestampMicros) -> Result<u64> {
        let res = sqlx::query("DELETE FROM cluster_nodes WHERE last_heartbeat_at_micros < $1")
            .bind(older_than.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(res.rows_affected())
    }

    async fn delete(&self, node_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM cluster_nodes WHERE node_id = $1")
            .bind(node_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

// ----- 基于 cluster_nodes 表的 ClusterRegistry 实装 -----

pub struct PgClusterRegistry {
    repo: Arc<dyn ClusterNodesRepository>,
    self_advertise: String,
    alive_window_secs: i64,
    cache_ttl: Duration,
    cache: Mutex<AliveNodesCache>,
}

#[derive(Default)]
struct AliveNodesCache {
    refreshed_at: Option<Instant>,
    rows: Vec<ClusterNodeRow>,
}

impl PgClusterRegistry {
    pub fn new(
        repo: Arc<dyn ClusterNodesRepository>,
        self_advertise: String,
        alive_window_secs: i64,
    ) -> Self {
        Self::with_cache_ttl(
            repo,
            self_advertise,
            alive_window_secs,
            NODE_DISCOVERY_CACHE_TTL,
        )
    }

    fn with_cache_ttl(
        repo: Arc<dyn ClusterNodesRepository>,
        self_advertise: String,
        alive_window_secs: i64,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            repo,
            self_advertise,
            alive_window_secs,
            cache_ttl,
            cache: Mutex::new(AliveNodesCache::default()),
        }
    }

    async fn list_alive_cached(&self) -> Vec<ClusterNodeRow> {
        // One async mutex provides both the short-lived cache and single-flight refresh. On
        // expiry, concurrent router requests wait for the same SELECT instead of fanning out one
        // cluster_nodes query per request.
        let mut cache = self.cache.lock().await;
        if cache
            .refreshed_at
            .is_some_and(|refreshed_at| refreshed_at.elapsed() < self.cache_ttl)
        {
            return cache.rows.clone();
        }

        let since = TimestampMicros(TimestampMicros::now().0 - self.alive_window_secs * 1_000_000);
        let rows = match self.repo.list_alive(since).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(error = %error, "cluster node discovery refresh failed");
                Vec::new()
            }
        };
        cache.refreshed_at = Some(Instant::now());
        cache.rows = rows.clone();
        rows
    }
}

#[async_trait]
impl ClusterRegistry for PgClusterRegistry {
    async fn list_role(&self, role: PeerRole) -> Vec<PeerInfo> {
        self.list_alive_cached()
            .await
            .into_iter()
            .filter(|r| r.roles.contains(&role) || role == PeerRole::Standalone)
            .map(|r| PeerInfo {
                node_id: r.node_id,
                advertise_addr: r.advertise_addr,
                roles: r.roles,
            })
            .collect()
    }

    async fn pick_ingester(&self, org_id: &Id, stream: &str) -> Option<PeerInfo> {
        // FNV-1a 一致性哈希按 (org_id, stream) 选 ingester
        let peers = self.list_role(PeerRole::Ingester).await;
        if peers.is_empty() {
            return None;
        }
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in org_id
            .0
            .as_bytes()
            .iter()
            .chain(b"|".iter())
            .chain(stream.as_bytes())
        {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let idx = (h as usize) % peers.len();
        peers.into_iter().nth(idx)
    }

    async fn pick_querier(&self) -> Option<PeerInfo> {
        // 朴素轮询：用当前时间纳秒做 mod；当前即可
        let peers = self.list_role(PeerRole::Querier).await;
        if peers.is_empty() {
            return None;
        }
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let idx = (now_ns as usize) % peers.len();
        peers.into_iter().nth(idx)
    }
}

impl PgClusterRegistry {
    pub fn self_advertise(&self) -> &str {
        &self.self_advertise
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn roles_csv_round_trip() {
        assert_eq!(
            roles_to_csv(&[PeerRole::Ingester, PeerRole::Querier]),
            "ingester,querier"
        );
        assert_eq!(
            roles_from_csv("ingester,querier"),
            vec![PeerRole::Ingester, PeerRole::Querier]
        );
        // 空段忽略；空集兜底 standalone。
        assert_eq!(
            roles_from_csv(" ingester , , querier "),
            vec![PeerRole::Ingester, PeerRole::Querier]
        );
        assert_eq!(roles_from_csv(""), vec![PeerRole::Standalone]);
    }

    struct FixedRepo(Vec<ClusterNodeRow>);
    #[async_trait]
    impl ClusterNodesRepository for FixedRepo {
        async fn upsert(
            &self,
            _n: &str,
            _r: &[PeerRole],
            _a: &str,
            _t: TimestampMicros,
        ) -> Result<()> {
            Ok(())
        }
        async fn list_alive(&self, _since: TimestampMicros) -> Result<Vec<ClusterNodeRow>> {
            Ok(self.0.clone())
        }
        async fn sweep_stale(&self, _older: TimestampMicros) -> Result<u64> {
            Ok(0)
        }
        async fn delete(&self, _node_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct CountingRepo {
        rows: Vec<ClusterNodeRow>,
        list_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ClusterNodesRepository for CountingRepo {
        async fn upsert(
            &self,
            _node_id: &str,
            _roles: &[PeerRole],
            _advertise_addr: &str,
            _ts: TimestampMicros,
        ) -> Result<()> {
            Ok(())
        }

        async fn list_alive(&self, _since: TimestampMicros) -> Result<Vec<ClusterNodeRow>> {
            self.list_calls.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            Ok(self.rows.clone())
        }

        async fn sweep_stale(&self, _older_than: TimestampMicros) -> Result<u64> {
            Ok(0)
        }

        async fn delete(&self, _node_id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn row(node: &str, roles: Vec<PeerRole>) -> ClusterNodeRow {
        let now = TimestampMicros::now();
        ClusterNodeRow {
            node_id: node.into(),
            roles,
            advertise_addr: format!("{node}:1"),
            started_at: now,
            last_heartbeat_at: now,
        }
    }

    #[tokio::test]
    async fn list_role_matches_multi_role_membership() {
        let rows = vec![
            row("a", vec![PeerRole::Ingester, PeerRole::Querier]),
            row("b", vec![PeerRole::Compactor]),
        ];
        let reg = PgClusterRegistry::new(std::sync::Arc::new(FixedRepo(rows)), "self:1".into(), 15);

        let queriers = reg.list_role(PeerRole::Querier).await;
        assert_eq!(queriers.len(), 1);
        assert_eq!(queriers[0].node_id, "a");
        assert_eq!(
            queriers[0].roles,
            vec![PeerRole::Ingester, PeerRole::Querier]
        );

        // 同一多角色节点也作为 ingester 被发现。
        let ingesters = reg.list_role(PeerRole::Ingester).await;
        assert_eq!(ingesters.len(), 1);
        assert_eq!(ingesters[0].node_id, "a");

        let compactors = reg.list_role(PeerRole::Compactor).await;
        assert_eq!(compactors.len(), 1);
        assert_eq!(compactors[0].node_id, "b");
    }

    #[tokio::test]
    async fn discovery_cache_is_shared_across_roles_and_concurrent_callers() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let repo = Arc::new(CountingRepo {
            rows: vec![row(
                "a",
                vec![PeerRole::Ingester, PeerRole::Querier, PeerRole::Compactor],
            )],
            list_calls: Arc::clone(&list_calls),
        });
        let registry =
            PgClusterRegistry::with_cache_ttl(repo, "self:1".into(), 15, Duration::from_secs(2));

        let (ingesters, queriers, compactors) = tokio::join!(
            registry.list_role(PeerRole::Ingester),
            registry.list_role(PeerRole::Querier),
            registry.list_role(PeerRole::Compactor),
        );

        assert_eq!(ingesters.len(), 1);
        assert_eq!(queriers.len(), 1);
        assert_eq!(compactors.len(), 1);
        assert_eq!(list_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn expired_discovery_cache_refreshes_again() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let repo = Arc::new(CountingRepo {
            rows: vec![row("a", vec![PeerRole::Ingester])],
            list_calls: Arc::clone(&list_calls),
        });
        let registry = PgClusterRegistry::with_cache_ttl(repo, "self:1".into(), 15, Duration::ZERO);

        registry.list_role(PeerRole::Ingester).await;
        registry.list_role(PeerRole::Ingester).await;

        assert_eq!(list_calls.load(Ordering::Relaxed), 2);
    }
}
