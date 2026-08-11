// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群事件总线的存储层：outbox（发送端）+ 投递游标 + org 映射 + Lamport 版本寄存器
//! + 接收端去重。trait + Pg 实装（与 remote_clusters / regex_patterns 一样 trait 落 infra）。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::federation::CloudEvent,
    shared::{Result, ids::Id, time::TimestampMicros},
};

// ===== 发送端 outbox + 投递游标 =====

/// outbox 一行：单调 `seq` + 该行的 CloudEvent。
#[derive(Debug, Clone)]
pub struct OutboxRow {
    pub seq: i64,
    pub event: CloudEvent,
}

#[async_trait]
pub trait ClusterEventOutboxRepository: Send + Sync {
    /// 追加一条事件（按 CloudEvent.id 幂等）。
    async fn append(&self, org_id: &Id, ev: &CloudEvent) -> Result<()>;
    /// 取 `seq > after_seq` 且 org ∈ orgs 的未投递事件（升序，限量）。
    async fn list_undelivered(
        &self,
        after_seq: i64,
        orgs: &[String],
        limit: i64,
    ) -> Result<Vec<OutboxRow>>;
    /// 某远端集群已确认到的 seq（无记录返回 0）。
    async fn acked_seq(&self, remote_cluster_id: &Id) -> Result<i64>;
    /// 推进某远端集群的确认游标（取 max，幂等）。
    async fn ack(&self, remote_cluster_id: &Id, seq: i64) -> Result<()>;
    /// 删除 `seq <= up_to`（全部 enabled 集群都已确认后清理）。
    async fn prune(&self, up_to_seq: i64) -> Result<u64>;
    /// 最大 seq（无事件返回 0），供 lag metric。
    async fn max_seq(&self) -> Result<i64>;
}

pub struct PgClusterEventOutboxRepository {
    pool: PgPool,
}
impl PgClusterEventOutboxRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ClusterEventOutboxRepository for PgClusterEventOutboxRepository {
    async fn append(&self, org_id: &Id, ev: &CloudEvent) -> Result<()> {
        sqlx::query(
            "INSERT INTO cluster_event_outbox
                (id, org_id, event_type, subject, payload, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&ev.id)
        .bind(&org_id.0)
        .bind(&ev.event_type)
        .bind(&ev.subject)
        .bind(Json(ev))
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_undelivered(
        &self,
        after_seq: i64,
        orgs: &[String],
        limit: i64,
    ) -> Result<Vec<OutboxRow>> {
        let rows = sqlx::query(
            "SELECT seq, payload FROM cluster_event_outbox
             WHERE seq > $1 AND org_id = ANY($2)
             ORDER BY seq ASC LIMIT $3",
        )
        .bind(after_seq)
        .bind(orgs)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let seq: i64 = r.try_get("seq").ok()?;
                let ev: Json<CloudEvent> = r.try_get("payload").ok()?;
                Some(OutboxRow { seq, event: ev.0 })
            })
            .collect())
    }

    async fn acked_seq(&self, remote_cluster_id: &Id) -> Result<i64> {
        let row = sqlx::query(
            "SELECT acked_seq FROM cluster_event_delivery WHERE remote_cluster_id = $1",
        )
        .bind(&remote_cluster_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row
            .and_then(|r| r.try_get::<i64, _>("acked_seq").ok())
            .unwrap_or(0))
    }

    async fn ack(&self, remote_cluster_id: &Id, seq: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO cluster_event_delivery (remote_cluster_id, acked_seq, updated_at_micros)
             VALUES ($1, $2, $3)
             ON CONFLICT (remote_cluster_id) DO UPDATE
             SET acked_seq = GREATEST(cluster_event_delivery.acked_seq, EXCLUDED.acked_seq),
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&remote_cluster_id.0)
        .bind(seq)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn prune(&self, up_to_seq: i64) -> Result<u64> {
        let res = sqlx::query("DELETE FROM cluster_event_outbox WHERE seq <= $1")
            .bind(up_to_seq)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(res.rows_affected())
    }

    async fn max_seq(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COALESCE(MAX(seq), 0) AS m FROM cluster_event_outbox")
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.try_get::<i64, _>("m").unwrap_or(0))
    }
}

// ===== org 映射（带 per-org token）=====

#[derive(Debug, Clone)]
pub struct OrgLink {
    pub remote_cluster_id: Id,
    pub local_org_id: Id,
    pub remote_org_id: Id,
    pub token_secret_ref: Option<String>,
}

#[async_trait]
pub trait ClusterOrgLinkRepository: Send + Sync {
    async fn upsert(&self, link: OrgLink) -> Result<()>;
    /// 发送端：本地 org → 该集群上的远端 org（+ per-org token）。
    async fn get_for_local(
        &self,
        remote_cluster_id: &Id,
        local_org: &Id,
    ) -> Result<Option<OrgLink>>;
    /// 接收端：远端 org（事件携带）→ 本地 org。
    async fn get_for_remote(
        &self,
        remote_cluster_id: &Id,
        remote_org: &Id,
    ) -> Result<Option<OrgLink>>;
    async fn list(&self, remote_cluster_id: &Id) -> Result<Vec<OrgLink>>;
    async fn delete(&self, remote_cluster_id: &Id, local_org: &Id) -> Result<()>;
}

pub struct PgClusterOrgLinkRepository {
    pool: PgPool,
}
impl PgClusterOrgLinkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const LINK_COLS: &str = "remote_cluster_id, local_org_id, remote_org_id, token_secret_ref";

fn link_row(r: sqlx::postgres::PgRow) -> OrgLink {
    OrgLink {
        remote_cluster_id: Id(r
            .try_get::<String, _>("remote_cluster_id")
            .unwrap_or_default()),
        local_org_id: Id(r.try_get::<String, _>("local_org_id").unwrap_or_default()),
        remote_org_id: Id(r.try_get::<String, _>("remote_org_id").unwrap_or_default()),
        token_secret_ref: r
            .try_get::<Option<String>, _>("token_secret_ref")
            .unwrap_or(None),
    }
}

#[async_trait]
impl ClusterOrgLinkRepository for PgClusterOrgLinkRepository {
    async fn upsert(&self, link: OrgLink) -> Result<()> {
        sqlx::query(
            "INSERT INTO cluster_org_link (remote_cluster_id, local_org_id, remote_org_id, token_secret_ref)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (remote_cluster_id, local_org_id) DO UPDATE
             SET remote_org_id = EXCLUDED.remote_org_id, token_secret_ref = EXCLUDED.token_secret_ref",
        )
        .bind(&link.remote_cluster_id.0)
        .bind(&link.local_org_id.0)
        .bind(&link.remote_org_id.0)
        .bind(&link.token_secret_ref)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn get_for_local(
        &self,
        remote_cluster_id: &Id,
        local_org: &Id,
    ) -> Result<Option<OrgLink>> {
        let sql = format!(
            "SELECT {LINK_COLS} FROM cluster_org_link WHERE remote_cluster_id = $1 AND local_org_id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&remote_cluster_id.0)
            .bind(&local_org.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(link_row))
    }

    async fn get_for_remote(
        &self,
        remote_cluster_id: &Id,
        remote_org: &Id,
    ) -> Result<Option<OrgLink>> {
        let sql = format!(
            "SELECT {LINK_COLS} FROM cluster_org_link WHERE remote_cluster_id = $1 AND remote_org_id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&remote_cluster_id.0)
            .bind(&remote_org.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(link_row))
    }

    async fn list(&self, remote_cluster_id: &Id) -> Result<Vec<OrgLink>> {
        let sql = format!(
            "SELECT {LINK_COLS} FROM cluster_org_link WHERE remote_cluster_id = $1 ORDER BY local_org_id"
        );
        let rows = sqlx::query(&sql)
            .bind(&remote_cluster_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(link_row).collect())
    }

    async fn delete(&self, remote_cluster_id: &Id, local_org: &Id) -> Result<()> {
        sqlx::query(
            "DELETE FROM cluster_org_link WHERE remote_cluster_id = $1 AND local_org_id = $2",
        )
        .bind(&remote_cluster_id.0)
        .bind(&local_org.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}

// ===== Lamport 版本寄存器 =====

#[async_trait]
pub trait ClusterResourceVersionRepository: Send + Sync {
    /// 本地写：原子 +1 返回新版本（writer=本集群）。
    async fn bump_local(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
        writer: &str,
    ) -> Result<i64>;
    /// 当前 (version, writer)；无记录返回 None。
    async fn get(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
    ) -> Result<Option<(i64, String)>>;
    /// 采纳远端胜出版本（接收端 apply 命中 `wins` 后调用，就地设为 incoming）。
    async fn adopt(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
        version: i64,
        writer: &str,
    ) -> Result<()>;
}

pub struct PgClusterResourceVersionRepository {
    pool: PgPool,
}
impl PgClusterResourceVersionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ClusterResourceVersionRepository for PgClusterResourceVersionRepository {
    async fn bump_local(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
        writer: &str,
    ) -> Result<i64> {
        let row = sqlx::query(
            "INSERT INTO cluster_resource_version
                (resource_kind, org_id, resource_id, version, writer, updated_at_micros)
             VALUES ($1, $2, $3, 1, $4, $5)
             ON CONFLICT (resource_kind, org_id, resource_id) DO UPDATE
             SET version = cluster_resource_version.version + 1,
                 writer = EXCLUDED.writer,
                 updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING version",
        )
        .bind(kind)
        .bind(&org_id.0)
        .bind(resource_id)
        .bind(writer)
        .bind(TimestampMicros::now().0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.try_get::<i64, _>("version").unwrap_or(1))
    }

    async fn get(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
    ) -> Result<Option<(i64, String)>> {
        let row = sqlx::query(
            "SELECT version, writer FROM cluster_resource_version
             WHERE resource_kind = $1 AND org_id = $2 AND resource_id = $3",
        )
        .bind(kind)
        .bind(&org_id.0)
        .bind(resource_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.map(|r| {
            (
                r.try_get::<i64, _>("version").unwrap_or(0),
                r.try_get::<String, _>("writer").unwrap_or_default(),
            )
        }))
    }

    async fn adopt(
        &self,
        kind: &str,
        org_id: &Id,
        resource_id: &str,
        version: i64,
        writer: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO cluster_resource_version
                (resource_kind, org_id, resource_id, version, writer, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (resource_kind, org_id, resource_id) DO UPDATE
             SET version = EXCLUDED.version, writer = EXCLUDED.writer,
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(kind)
        .bind(&org_id.0)
        .bind(resource_id)
        .bind(version)
        .bind(writer)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}

// ===== 接收端去重 =====

#[async_trait]
pub trait SeenEventsRepository: Send + Sync {
    /// 插入 id；返回 true 表示**首次**见到（应处理），false 表示已见过（跳过）。
    async fn insert_if_absent(&self, id: &str) -> Result<bool>;
    /// 撤销一条去重记录：瞬时失败时调用，使该事件下次重投能再被处理（不被误判已见）。
    async fn remove(&self, id: &str) -> Result<()>;
    /// 清理早于 `older_than` 的去重记录。
    async fn sweep(&self, older_than: TimestampMicros) -> Result<u64>;
}

pub struct PgSeenEventsRepository {
    pool: PgPool,
}
impl PgSeenEventsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SeenEventsRepository for PgSeenEventsRepository {
    async fn insert_if_absent(&self, id: &str) -> Result<bool> {
        let res = sqlx::query(
            "INSERT INTO seen_events (id, received_at_micros) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(res.rows_affected() == 1)
    }

    async fn remove(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM seen_events WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn sweep(&self, older_than: TimestampMicros) -> Result<u64> {
        let res = sqlx::query("DELETE FROM seen_events WHERE received_at_micros < $1")
            .bind(older_than.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(res.rows_affected())
    }
}
