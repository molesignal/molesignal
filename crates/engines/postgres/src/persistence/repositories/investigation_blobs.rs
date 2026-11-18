// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `investigation_blobs` 表 Pg 实装（web-investigation-shell）。
//!
//! 投资栈 payload 超 4 KB 时，前端把 JSON 上传到这里，URL 里只放 blob_id。
//! The content lives in the object store; a row only points at it with `object_key`.
//! 7 天自动清理（compactor role 周期 tick，同时删掉对应 S3 对象）。

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use sqlx::{PgPool, Row};

use super::sqlx_err;

#[async_trait]
pub trait InvestigationBlobRepository: Send + Sync {
    /// 记录指针行：内容已写入对象存储，PG 只存 `object_key`。
    async fn create(&self, id: &Id, org_id: &Id, object_key: &str) -> Result<()>;
    /// The object store key of the blob.
    async fn fetch(&self, org_id: &Id, id: &Id) -> Result<String>;
    /// 删除 `created_at < cutoff_micros` 的行，返回这些行的 object_key，
    /// 供调用方顺带清理对象存储里的孤儿对象。
    async fn delete_older_than(&self, cutoff_micros: i64) -> Result<Vec<String>>;
}

pub struct PgInvestigationBlobRepository {
    pool: PgPool,
}

impl PgInvestigationBlobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InvestigationBlobRepository for PgInvestigationBlobRepository {
    async fn create(&self, id: &Id, org_id: &Id, object_key: &str) -> Result<()> {
        let now = TimestampMicros::now();

        sqlx::query(
            "INSERT INTO investigation_blobs (id, org_id, object_key, created_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&id.0)
        .bind(&org_id.0)
        .bind(object_key)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn fetch(&self, org_id: &Id, id: &Id) -> Result<String> {
        let row =
            sqlx::query("SELECT object_key FROM investigation_blobs WHERE org_id = $1 AND id = $2")
                .bind(&org_id.0)
                .bind(&id.0)
                .fetch_one(&self.pool)
                .await
                .map_err(sqlx_err)?;

        row.try_get("object_key").map_err(sqlx_err)
    }

    async fn delete_older_than(&self, cutoff_micros: i64) -> Result<Vec<String>> {
        // 用 RETURNING 一次拿回被删行的 object_key，供调用方清对象存储。
        let rows = sqlx::query(
            "DELETE FROM investigation_blobs WHERE created_at < $1 RETURNING object_key",
        )
        .bind(TimestampMicros(cutoff_micros))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| row.try_get("object_key").map_err(sqlx_err))
            .collect()
    }
}
