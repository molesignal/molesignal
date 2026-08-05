// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `intelligence_chat_archives` 表 Pg 实装。
//!
//! Mole Intelligence chat transcript 归档元数据：完整 transcript JSON 写到对象存储，PG 只留
//! `object_key` + `sha256` + `bytes` + `status`。归档失败时也写一行（status=failed）
//! 保留失败记录，不抹掉 PG chat history。retention 清理走 [`delete_older_than`]
//! 返回 object_key 供调用方顺带清对象存储孤儿。

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize)]
pub struct ChatArchive {
    pub id: Id,
    pub chat_id: Id,
    pub org_id: Id,
    pub object_key: Option<String>,
    pub sha256: Option<String>,
    pub bytes: i64,
    pub status: String,
    pub error: Option<String>,
    pub created_by: Option<String>,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait ChatArchiveRepository: Send + Sync {
    async fn record(&self, a: ChatArchive) -> Result<ChatArchive>;
    async fn list_for_chat(&self, chat_id: &Id) -> Result<Vec<ChatArchive>>;
    async fn list_for_org(&self, org_id: &Id, limit: i64) -> Result<Vec<ChatArchive>>;
    /// 删除 `created_at_micros < cutoff` 的归档行，返回这些行的 object_key（非空者）。
    async fn delete_older_than(&self, cutoff_micros: i64) -> Result<Vec<String>>;
}

pub struct PgChatArchiveRepository {
    pool: PgPool,
}

impl PgChatArchiveRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, chat_id, org_id, object_key, sha256, bytes, status, error, \
    created_by, created_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Result<ChatArchive> {
    Ok(ChatArchive {
        id: Id(r.try_get::<String, _>("id").map_err(sqlx_err)?),
        chat_id: Id(r.try_get::<String, _>("chat_id").map_err(sqlx_err)?),
        org_id: Id(r.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        object_key: r.try_get("object_key").map_err(sqlx_err)?,
        sha256: r.try_get("sha256").map_err(sqlx_err)?,
        bytes: r.try_get("bytes").map_err(sqlx_err)?,
        status: r.try_get("status").map_err(sqlx_err)?,
        error: r.try_get("error").map_err(sqlx_err)?,
        created_by: r.try_get("created_by").map_err(sqlx_err)?,
        created_at: TimestampMicros(r.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl ChatArchiveRepository for PgChatArchiveRepository {
    async fn record(&self, a: ChatArchive) -> Result<ChatArchive> {
        sqlx::query(
            "INSERT INTO intelligence_chat_archives
                (id, chat_id, org_id, object_key, sha256, bytes, status, error,
                 created_by, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&a.id.0)
        .bind(&a.chat_id.0)
        .bind(&a.org_id.0)
        .bind(&a.object_key)
        .bind(&a.sha256)
        .bind(a.bytes)
        .bind(&a.status)
        .bind(&a.error)
        .bind(&a.created_by)
        .bind(a.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(a)
    }

    async fn list_for_chat(&self, chat_id: &Id) -> Result<Vec<ChatArchive>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_chat_archives
             WHERE chat_id = $1 ORDER BY created_at_micros DESC"
        ))
        .bind(&chat_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn list_for_org(&self, org_id: &Id, limit: i64) -> Result<Vec<ChatArchive>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_chat_archives
             WHERE org_id = $1 ORDER BY created_at_micros DESC LIMIT $2"
        ))
        .bind(&org_id.0)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn delete_older_than(&self, cutoff_micros: i64) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "DELETE FROM intelligence_chat_archives WHERE created_at_micros < $1 RETURNING object_key",
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
