// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `file_download_tokens` 表 Pg 实装（spec storage 修订）。
//!
//! 异步文件下载：用户 POST 一组 object_key → 系统生成短期 token；用户
//! GET `/api/v1/files/stream/<token>` 流式拉对象（适配 local backend）。
//! S3 backend 直接返 pre-signed URL（不写 token 表）。本 repo 仅承担
//! 非 S3 路径的 token 存储。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDownloadToken {
    pub token: String,
    pub org_id: Id,
    pub user_id: Id,
    pub object_keys: Vec<String>,
    pub expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait FileDownloadTokenRepository: Send + Sync {
    async fn create(&self, t: FileDownloadToken) -> Result<FileDownloadToken>;
    async fn get(&self, token: &str) -> Result<Option<FileDownloadToken>>;
    async fn delete_expired(&self, cutoff: TimestampMicros) -> Result<u64>;
}

pub struct PgFileDownloadTokenRepository {
    pool: PgPool,
}

impl PgFileDownloadTokenRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to(r: sqlx::postgres::PgRow) -> FileDownloadToken {
    let keys: Json<Vec<String>> = r.try_get("object_keys_json").unwrap_or(Json(Vec::new()));
    FileDownloadToken {
        token: r.try_get::<String, _>("token").unwrap_or_default(),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        user_id: Id(r.try_get::<String, _>("user_id").unwrap_or_default()),
        object_keys: keys.0,
        expires_at: TimestampMicros(r.try_get::<i64, _>("expires_at_micros").unwrap_or_default()),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl FileDownloadTokenRepository for PgFileDownloadTokenRepository {
    async fn create(&self, t: FileDownloadToken) -> Result<FileDownloadToken> {
        sqlx::query(
            "INSERT INTO file_download_tokens
                (token, org_id, user_id, object_keys_json, expires_at_micros, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&t.token)
        .bind(&t.org_id.0)
        .bind(&t.user_id.0)
        .bind(Json(&t.object_keys))
        .bind(t.expires_at.0)
        .bind(t.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(t)
    }

    async fn get(&self, token: &str) -> Result<Option<FileDownloadToken>> {
        let row = sqlx::query(
            "SELECT token, org_id, user_id, object_keys_json, expires_at_micros, created_at_micros
             FROM file_download_tokens WHERE token = $1",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn delete_expired(&self, cutoff: TimestampMicros) -> Result<u64> {
        let res = sqlx::query("DELETE FROM file_download_tokens WHERE expires_at_micros < $1")
            .bind(cutoff.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(res.rows_affected())
    }
}

/// 生成下载 token：32 字符 base62 URL-safe。
pub fn generate_token() -> String {
    use rand::TryRng as _;
    const ALPHA: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut buf = [0u8; 32];
    rand::rngs::SysRng.try_fill_bytes(&mut buf).expect("rng");
    buf.iter()
        .map(|b| ALPHA[(*b as usize) % ALPHA.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
