// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 文件下载令牌（spec storage 修订），存在 `one_time_tokens` 里（`kind = 'file_download'`），只存令牌的
//! SHA-256 摘要。
//!
//! 异步文件下载：用户 POST 一组 object_key → 系统生成短期 token；用户
//! GET `/api/v1/files/stream/<token>` 流式拉对象（适配 local backend）。
//! S3 backend 直接返 pre-signed URL（不写令牌）。本 repo 仅承担
//! 非 S3 路径的令牌存储。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// 库里没有令牌原文：读出来的令牌就是调用方拿来查的那个。
fn row_to(r: sqlx::postgres::PgRow, token: &str) -> Result<FileDownloadToken> {
    let keys: Json<Vec<String>> = r.try_get("payload").map_err(sqlx_err)?;

    Ok(FileDownloadToken {
        token: token.to_owned(),
        org_id: Id(r.try_get("org_id").map_err(sqlx_err)?),
        user_id: Id(r.try_get("user_id").map_err(sqlx_err)?),
        object_keys: keys.0,
        expires_at: r.try_get("expires_at").map_err(sqlx_err)?,
        created_at: r.try_get("created_at").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl FileDownloadTokenRepository for PgFileDownloadTokenRepository {
    async fn create(&self, t: FileDownloadToken) -> Result<FileDownloadToken> {
        sqlx::query(
            "INSERT INTO one_time_tokens
                (token_hash, kind, org_id, user_id, payload, expires_at, created_at)
             VALUES ($1, 'file_download', $2, $3, $4, $5, $6)",
        )
        .bind(digest(&t.token))
        .bind(&t.org_id.0)
        .bind(&t.user_id.0)
        .bind(Json(&t.object_keys))
        .bind(t.expires_at)
        .bind(t.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(t)
    }

    async fn get(&self, token: &str) -> Result<Option<FileDownloadToken>> {
        let row = sqlx::query(
            "SELECT org_id, user_id, payload, expires_at, created_at
             FROM one_time_tokens WHERE token_hash = $1 AND kind = 'file_download'",
        )
        .bind(digest(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| row_to(row, token)).transpose()
    }

    async fn delete_expired(&self, cutoff: TimestampMicros) -> Result<u64> {
        let res = sqlx::query(
            "DELETE FROM one_time_tokens WHERE kind = 'file_download' AND expires_at < $1",
        )
        .bind(cutoff)
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
