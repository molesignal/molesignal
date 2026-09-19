// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `investigation_blobs` 表 Pg 实装（web-investigation-shell）。
//!
//! 投资栈 payload 超 4 KB 时，前端把 JSON 上传到这里，URL 里只放 blob_id。
//! 内容外置到对象存储(S3)：PG 行只存 `object_key` 指针，读时回对象存储取；旧行
//! （payload 在 PG）走 legacy 读路径。7 天自动清理（compactor role 周期 tick，
//! 同时删掉对应 S3 对象）。

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 一条 blob 的存储位置：要么是对象存储 key（新行），要么是 PG 里的旧 payload。
#[derive(Debug, Default)]
pub struct StoredBlob {
    pub object_key: Option<String>,
    pub legacy_payload: Option<Value>,
}

#[async_trait]
pub trait InvestigationBlobRepository: Send + Sync {
    /// 记录指针行：内容已写入对象存储，PG 只存 `object_key`。
    async fn create(&self, id: &Id, org_id: &Id, object_key: &str) -> Result<()>;
    /// 取存储位置（object_key 新行 / legacy_payload 旧行），由调用方决定从哪读。
    async fn fetch(&self, org_id: &Id, id: &Id) -> Result<StoredBlob>;
    /// 删除 `created_at_micros < cutoff_micros` 的行，返回这些行的 object_key（非空者），
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
            "INSERT INTO investigation_blobs (id, org_id, object_key, created_at_micros)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&id.0)
        .bind(&org_id.0)
        .bind(object_key)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn fetch(&self, org_id: &Id, id: &Id) -> Result<StoredBlob> {
        let row = sqlx::query(
            "SELECT object_key, payload FROM investigation_blobs WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let object_key: Option<String> = row.try_get("object_key").ok();
        let legacy_payload: Option<Value> = row
            .try_get::<Option<Json<Value>>, _>("payload")
            .ok()
            .flatten()
            .map(|j| j.0);
        Ok(StoredBlob {
            object_key,
            legacy_payload,
        })
    }

    async fn delete_older_than(&self, cutoff_micros: i64) -> Result<Vec<String>> {
        // 用 RETURNING 一次拿回被删行的 object_key（非空者），供调用方清对象存储。
        let rows = sqlx::query(
            "DELETE FROM investigation_blobs WHERE created_at_micros < $1 RETURNING object_key",
        )
        .bind(cutoff_micros)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<Option<String>, _>("object_key").ok().flatten())
            .collect())
    }
}
