// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster_secrets` 表 CRUD —— 联邦 token 的 DB 后端（spec federated-search）。
//!
//! `remote_clusters.token_secret_ref` 形如 `cipher_keys:<ref_id>` 时即指向本表一行。
//! 明文 token 用 [`CipherRootKey`]（envelope KEK）AES-256-GCM `seal` 后存
//! `ciphertext` + `nonce`，读取时 `open` 还原。
//!
//! 为什么不复用 `cipher_keys` 表：`cipher_keys` 行按语义只放固定 32B 字段级 DEK
//! （`create` 强校验 `raw_key.len() == 32`），承接不了任意长度的 bearer token。
//! `cipher_keys:` 这个 scheme 名沿用 spec 指针写法，落地在这张专用表。
//!
//! **明文 token 绝不能进日志或任何序列化响应**——本模块不实现 `Serialize`，
//! `get_plaintext` 的返回值只在 [`crate::infra::secret::resolve_secret_ref`] 内即用即弃。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
pub trait ClusterSecretRepository: Send + Sync {
    /// 写入/覆盖一条 cluster secret（明文经 root key seal 后落库）。
    async fn put(&self, org_id: &Id, ref_id: &str, plaintext: &str) -> Result<()>;
    /// 解出明文 token；不存在返 [`Error::not_found`]。明文绝不入日志/响应。
    async fn get_plaintext(&self, org_id: &Id, ref_id: &str) -> Result<String>;
    async fn delete(&self, org_id: &Id, ref_id: &str) -> Result<()>;
}

pub struct PgClusterSecretRepository {
    pool: PgPool,
    mk: CipherRootKey,
}

impl PgClusterSecretRepository {
    pub fn new(pool: PgPool, mk: CipherRootKey) -> Self {
        Self { pool, mk }
    }
}

#[async_trait]
impl ClusterSecretRepository for PgClusterSecretRepository {
    async fn put(&self, org_id: &Id, ref_id: &str, plaintext: &str) -> Result<()> {
        if ref_id.is_empty() {
            return Err(Error::invalid("cluster secret ref_id 不能为空"));
        }
        let (nonce, enc) = self
            .mk
            .seal(plaintext.as_bytes())
            .map_err(|e| Error::internal(format!("cluster secret seal: {e}")))?;
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO cluster_secrets
                (org_id, ref_id, ciphertext, nonce, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $5)
             ON CONFLICT (org_id, ref_id) DO UPDATE SET
                ciphertext = EXCLUDED.ciphertext,
                nonce = EXCLUDED.nonce,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(ref_id)
        .bind(&enc)
        .bind(&nonce)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(())
    }

    async fn get_plaintext(&self, org_id: &Id, ref_id: &str) -> Result<String> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce FROM cluster_secrets WHERE org_id = $1 AND ref_id = $2",
        )
        .bind(&org_id.0)
        .bind(ref_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("cluster secret `{ref_id}` 未配置")))?;

        let enc: Vec<u8> = row
            .try_get("ciphertext")
            .map_err(|e| Error::internal(e.to_string()))?;
        let nonce: Vec<u8> = row
            .try_get("nonce")
            .map_err(|e| Error::internal(e.to_string()))?;
        let plain = self
            .mk
            .open(&nonce, &enc)
            // 错误信息只提 ref_id，绝不带明文 / 密文片段。
            .map_err(|_| Error::internal(format!("cluster secret `{ref_id}` 解密失败")))?;
        String::from_utf8(plain)
            .map_err(|_| Error::internal(format!("cluster secret `{ref_id}` 非 UTF-8 token")))
    }

    async fn delete(&self, org_id: &Id, ref_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM cluster_secrets WHERE org_id = $1 AND ref_id = $2")
            .bind(&org_id.0)
            .bind(ref_id)
            .execute(&self.pool)
            .await
            .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(())
    }
}
