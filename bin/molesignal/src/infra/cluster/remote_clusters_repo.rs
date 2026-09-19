// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `remote_clusters` 表 CRUD。
//!
//! Federated search 用：每个 RemoteCluster 是另一个 molesignal 部署，coordinator
//! 经 Arrow Flight `do_get` 把 SQL fan-out 给 remote，加 bearer token；token 字段
//! 在 list/get 响应中 mask 为 `***`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCluster {
    pub id: Id,
    pub name: String,
    pub advertise_addr: String,
    /// 真实 token / secret_ref；list/get handler 必须 mask。
    pub token_secret_ref: String,
    pub tls_verify: bool,
    pub enabled: bool,
    /// gossip 发现来源标记：未配 token / org_map 的待审集群。discovered 默认 `enabled=false`，
    /// admin 在 clusters 页手动启用 + 填 token + 配 org_map 后才生效。
    #[serde(default)]
    pub discovered: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait RemoteClustersRepository: Send + Sync {
    async fn create(&self, c: RemoteCluster) -> Result<RemoteCluster>;
    async fn update(&self, c: RemoteCluster) -> Result<RemoteCluster>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn get(&self, id: &Id) -> Result<RemoteCluster>;
    async fn list(&self) -> Result<Vec<RemoteCluster>>;
    async fn list_enabled(&self) -> Result<Vec<RemoteCluster>>;
    /// gossip 合并：未知集群入库为 `discovered=true, enabled=false`（按 id 幂等，不覆盖既有）。
    /// 返回真正新插入的条数。
    async fn insert_discovered(&self, id: &str, name: &str, advertise_addr: &str) -> Result<u64>;
}

pub struct PgRemoteClustersRepository {
    pool: PgPool,
}

impl PgRemoteClustersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, name, advertise_addr, token_secret_ref, tls_verify, enabled, discovered,
                    created_at_micros, updated_at_micros";

fn row_to(c: sqlx::postgres::PgRow) -> RemoteCluster {
    RemoteCluster {
        id: Id(c.try_get::<String, _>("id").unwrap_or_default()),
        name: c.try_get::<String, _>("name").unwrap_or_default(),
        advertise_addr: c.try_get::<String, _>("advertise_addr").unwrap_or_default(),
        token_secret_ref: c
            .try_get::<String, _>("token_secret_ref")
            .unwrap_or_default(),
        tls_verify: c.try_get::<bool, _>("tls_verify").unwrap_or(true),
        enabled: c.try_get::<bool, _>("enabled").unwrap_or(true),
        discovered: c.try_get::<bool, _>("discovered").unwrap_or(false),
        created_at: TimestampMicros(c.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(c.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl RemoteClustersRepository for PgRemoteClustersRepository {
    async fn create(&self, c: RemoteCluster) -> Result<RemoteCluster> {
        sqlx::query(
            "INSERT INTO remote_clusters
                (id, name, advertise_addr, token_secret_ref, tls_verify, enabled, discovered,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&c.id.0)
        .bind(&c.name)
        .bind(&c.advertise_addr)
        .bind(&c.token_secret_ref)
        .bind(c.tls_verify)
        .bind(c.enabled)
        .bind(c.discovered)
        .bind(c.created_at.0)
        .bind(c.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(c)
    }

    async fn update(&self, c: RemoteCluster) -> Result<RemoteCluster> {
        // admin 手动启用 discovered 集群即清掉 discovered 标记（升级为正式配置）。
        sqlx::query(
            "UPDATE remote_clusters SET
                name = $2,
                advertise_addr = $3,
                token_secret_ref = $4,
                tls_verify = $5,
                enabled = $6,
                discovered = $7,
                updated_at_micros = $8
             WHERE id = $1",
        )
        .bind(&c.id.0)
        .bind(&c.name)
        .bind(&c.advertise_addr)
        .bind(&c.token_secret_ref)
        .bind(c.tls_verify)
        .bind(c.enabled)
        .bind(c.discovered)
        .bind(c.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(c)
    }

    async fn insert_discovered(&self, id: &str, name: &str, advertise_addr: &str) -> Result<u64> {
        let now = TimestampMicros::now().0;
        let res = sqlx::query(
            "INSERT INTO remote_clusters
                (id, name, advertise_addr, token_secret_ref, tls_verify, enabled, discovered,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, '', TRUE, FALSE, TRUE, $4, $4)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(name)
        .bind(advertise_addr)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(res.rows_affected())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM remote_clusters WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn get(&self, id: &Id) -> Result<RemoteCluster> {
        let sql = format!("SELECT {COLS} FROM remote_clusters WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self) -> Result<Vec<RemoteCluster>> {
        let sql = format!("SELECT {COLS} FROM remote_clusters ORDER BY name");
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_enabled(&self) -> Result<Vec<RemoteCluster>> {
        let sql = format!("SELECT {COLS} FROM remote_clusters WHERE enabled = TRUE ORDER BY name");
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }
}
