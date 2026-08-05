// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `intelligence_model_providers` + `intelligence_model_provider_secrets` 表 Pg 实装。
//!
//! Provider 元数据 org-scoped；API key 明文经 [`CipherRootKey`] AES-256-GCM `seal`
//! 后存到 `intelligence_model_provider_secrets`（密文 + nonce），元数据表只留 masked
//! `key_last4` + `key_set`。明文 key **绝不**进任何序列化响应 / 日志 / 审计 / 归档。
//!
//! `ModelProvider` 结构不含密钥字段，可安全 `Serialize` 直接回前端。

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// Provider 元数据行。**不含**任何 key 明文/密文字段——可安全序列化回前端。
#[derive(Debug, Clone, Serialize)]
pub struct ModelProvider {
    pub id: Id,
    pub org_id: Id,
    pub provider: String,
    pub name: String,
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    pub timeout_ms: i64,
    pub max_tokens: Option<i64>,
    /// API key 后 4 位（masked 显示用）。
    pub key_last4: Option<String>,
    /// 是否已设置 key。
    pub key_set: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 创建 / 更新元数据的入参（不含 key；key 走单独参数）。
#[derive(Debug, Clone)]
pub struct ModelProviderInput {
    pub id: Id,
    pub org_id: Id,
    pub provider: String,
    pub name: String,
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    pub timeout_ms: i64,
    pub max_tokens: Option<i64>,
}

#[async_trait]
pub trait ModelProviderRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<ModelProvider>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ModelProvider>;
    /// 创建 provider；`api_key` 为 Some 时同时 seal 落库 + 记 masked 元数据。
    async fn create(
        &self,
        input: ModelProviderInput,
        api_key: Option<&str>,
    ) -> Result<ModelProvider>;
    /// 更新元数据（不动 key）。
    async fn update(&self, input: ModelProviderInput) -> Result<ModelProvider>;
    /// 轮换 key：re-seal 覆盖密文，更新 masked 元数据。
    async fn rotate_key(&self, org_id: &Id, id: &Id, api_key: &str) -> Result<ModelProvider>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    /// 解出明文 key 供运行时构造 adapter；未设置返 None。明文即用即弃，绝不入响应。
    async fn get_plaintext_key(&self, org_id: &Id, id: &Id) -> Result<Option<String>>;
}

pub struct PgModelProviderRepository {
    pool: PgPool,
    mk: CipherRootKey,
}

impl PgModelProviderRepository {
    pub fn new(pool: PgPool, mk: CipherRootKey) -> Self {
        Self { pool, mk }
    }
}

const COLS: &str = "id, org_id, provider, name, base_url, default_model, enabled, timeout_ms, \
    max_tokens, key_last4, key_set, created_at_micros, updated_at_micros";

/// 取 key 后 4 位作 masked 显示；短 key 退化为全部。
fn last4(api_key: &str) -> String {
    let chars: Vec<char> = api_key.chars().collect();
    let n = chars.len();
    let start = n.saturating_sub(4);
    chars[start..].iter().collect()
}

fn row_to(r: sqlx::postgres::PgRow) -> Result<ModelProvider> {
    Ok(ModelProvider {
        id: Id(r.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id(r.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        provider: r.try_get("provider").map_err(sqlx_err)?,
        name: r.try_get("name").map_err(sqlx_err)?,
        base_url: r.try_get("base_url").map_err(sqlx_err)?,
        default_model: r.try_get("default_model").map_err(sqlx_err)?,
        enabled: r.try_get("enabled").map_err(sqlx_err)?,
        timeout_ms: r.try_get("timeout_ms").map_err(sqlx_err)?,
        max_tokens: r.try_get("max_tokens").map_err(sqlx_err)?,
        key_last4: r.try_get("key_last4").map_err(sqlx_err)?,
        key_set: r.try_get("key_set").map_err(sqlx_err)?,
        created_at: TimestampMicros(r.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(r.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

impl PgModelProviderRepository {
    /// seal 明文 key 写入 secrets 表（upsert）。
    async fn seal_secret(&self, org_id: &Id, provider_id: &Id, api_key: &str) -> Result<()> {
        let (nonce, ct) = self
            .mk
            .seal(api_key.as_bytes())
            .map_err(|e| Error::internal(format!("ai provider key seal: {e}")))?;
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO intelligence_model_provider_secrets
                (provider_id, org_id, ciphertext, nonce, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $5)
             ON CONFLICT (provider_id) DO UPDATE SET
                ciphertext = EXCLUDED.ciphertext,
                nonce = EXCLUDED.nonce,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&provider_id.0)
        .bind(&org_id.0)
        .bind(&ct)
        .bind(&nonce)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}

#[async_trait]
impl ModelProviderRepository for PgModelProviderRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<ModelProvider>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_model_providers
             WHERE org_id = $1 ORDER BY updated_at_micros DESC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<ModelProvider> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_model_providers WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("ai provider `{}` not found", id.0)))?;
        row_to(row)
    }

    async fn create(
        &self,
        input: ModelProviderInput,
        api_key: Option<&str>,
    ) -> Result<ModelProvider> {
        let now = TimestampMicros::now();
        let key_last4 = api_key.map(last4);
        let key_set = api_key.is_some();
        sqlx::query(
            "INSERT INTO intelligence_model_providers
                (id, org_id, provider, name, base_url, default_model, enabled, timeout_ms,
                 max_tokens, key_last4, key_set, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)",
        )
        .bind(&input.id.0)
        .bind(&input.org_id.0)
        .bind(&input.provider)
        .bind(&input.name)
        .bind(&input.base_url)
        .bind(&input.default_model)
        .bind(input.enabled)
        .bind(input.timeout_ms)
        .bind(input.max_tokens)
        .bind(&key_last4)
        .bind(key_set)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if let Some(k) = api_key {
            self.seal_secret(&input.org_id, &input.id, k).await?;
        }
        self.get(&input.org_id, &input.id).await
    }

    async fn update(&self, input: ModelProviderInput) -> Result<ModelProvider> {
        let now = TimestampMicros::now();
        let res = sqlx::query(
            "UPDATE intelligence_model_providers SET
                provider = $3, name = $4, base_url = $5, default_model = $6,
                enabled = $7, timeout_ms = $8, max_tokens = $9, updated_at_micros = $10
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&input.org_id.0)
        .bind(&input.id.0)
        .bind(&input.provider)
        .bind(&input.name)
        .bind(&input.base_url)
        .bind(&input.default_model)
        .bind(input.enabled)
        .bind(input.timeout_ms)
        .bind(input.max_tokens)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "ai provider `{}` not found",
                input.id.0
            )));
        }
        self.get(&input.org_id, &input.id).await
    }

    async fn rotate_key(&self, org_id: &Id, id: &Id, api_key: &str) -> Result<ModelProvider> {
        // 先确认 provider 存在（org-scoped）。
        let _ = self.get(org_id, id).await?;
        self.seal_secret(org_id, id, api_key).await?;
        let now = TimestampMicros::now();
        sqlx::query(
            "UPDATE intelligence_model_providers SET key_last4 = $3, key_set = TRUE, updated_at_micros = $4
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(last4(api_key))
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get(org_id, id).await
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM intelligence_model_provider_secrets WHERE provider_id = $1 AND org_id = $2")
            .bind(&id.0)
            .bind(&org_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        sqlx::query("DELETE FROM intelligence_model_providers WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn get_plaintext_key(&self, org_id: &Id, id: &Id) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce FROM intelligence_model_provider_secrets
             WHERE provider_id = $1 AND org_id = $2",
        )
        .bind(&id.0)
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let ct: Vec<u8> = row.try_get("ciphertext").map_err(sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("nonce").map_err(sqlx_err)?;
        let plain = self
            .mk
            .open(&nonce, &ct)
            // 错误信息只提 id，绝不带明文/密文片段。
            .map_err(|_| Error::internal(format!("ai provider `{}` key decrypt failed", id.0)))?;
        let s = String::from_utf8(plain)
            .map_err(|_| Error::internal(format!("ai provider `{}` key not utf-8", id.0)))?;
        Ok(Some(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last4_takes_trailing_four_chars() {
        assert_eq!(last4("sk-abcd1234"), "1234");
        assert_eq!(last4("xyz"), "xyz");
        assert_eq!(last4(""), "");
    }
}
