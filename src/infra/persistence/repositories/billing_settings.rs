// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `billing_settings` 表 Pg 实装（billing 控制面）。
//!
//! 平台级单行（singleton id='default'）配置：Stripe webhook secret / api key 用
//! [`CipherRootKey`] AES-256-GCM seal 落库（密文 + nonce 分列）。`get` 解出明文供
//! webhook 验签 / 出站使用；route 层只回 masked（`*_set` 布尔）。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, time::TimestampMicros},
};

/// singleton 行 id。
const SINGLETON_ID: &str = "default";

/// 运行时 billing 配置（secrets 已解密）。
#[derive(Debug, Clone)]
pub struct BillingConfig {
    pub enabled: bool,
    pub signature_tolerance_secs: i64,
    /// 解密后的 Stripe webhook 签名密钥；未设置为空串。
    pub webhook_secret: String,
    /// 解密后的 Stripe api key；未设置为空串。
    pub api_key: String,
    pub webhook_secret_set: bool,
    pub api_key_set: bool,
    pub updated_at: TimestampMicros,
}

impl Default for BillingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            signature_tolerance_secs: 300,
            webhook_secret: String::new(),
            api_key: String::new(),
            webhook_secret_set: false,
            api_key_set: false,
            updated_at: TimestampMicros(0),
        }
    }
}

/// 写入参数。secret 字段 `None` = 保持原值；`Some("")` = 清空；`Some(s)` = 替换。
#[derive(Debug, Clone)]
pub struct BillingSettingsInput {
    pub enabled: bool,
    pub signature_tolerance_secs: i64,
    pub webhook_secret: Option<String>,
    pub api_key: Option<String>,
}

#[async_trait]
pub trait BillingSettingsRepository: Send + Sync {
    /// 读取并解密 secrets；行不存在时返回默认（disabled）。
    async fn get(&self) -> Result<BillingConfig>;
    /// upsert 配置，按 [`BillingSettingsInput`] 语义处理 secrets，返回写后配置。
    async fn set(&self, input: BillingSettingsInput) -> Result<BillingConfig>;
}

pub struct PgBillingSettingsRepository {
    pool: PgPool,
    mk: CipherRootKey,
}

impl PgBillingSettingsRepository {
    pub fn new(pool: PgPool, mk: CipherRootKey) -> Self {
        Self { pool, mk }
    }

    fn seal(&self, plain: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        let (nonce, ct) = self
            .mk
            .seal(plain.as_bytes())
            .map_err(|e| Error::internal(format!("billing secret seal: {e}")))?;
        Ok((nonce, ct))
    }

    fn open(&self, nonce: &[u8], ct: &[u8], what: &str) -> Result<String> {
        let plain = self
            .mk
            .open(nonce, ct)
            .map_err(|_| Error::internal(format!("billing {what} decrypt failed")))?;
        String::from_utf8(plain).map_err(|_| Error::internal(format!("billing {what} not utf-8")))
    }
}

/// DB 行的 sealed secret 列（解密前）。
struct SealedRow {
    enabled: bool,
    tolerance: i64,
    webhook_ct: Option<Vec<u8>>,
    webhook_nonce: Option<Vec<u8>>,
    api_ct: Option<Vec<u8>>,
    api_nonce: Option<Vec<u8>>,
    updated_at: i64,
}

impl PgBillingSettingsRepository {
    async fn fetch_row(&self) -> Result<Option<SealedRow>> {
        let row = sqlx::query(
            "SELECT enabled, signature_tolerance_secs,
                    webhook_secret_ciphertext, webhook_secret_nonce,
                    api_key_ciphertext, api_key_nonce, updated_at_micros
             FROM billing_settings WHERE id = $1",
        )
        .bind(SINGLETON_ID)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(SealedRow {
            enabled: r.try_get("enabled").map_err(sqlx_err)?,
            tolerance: r.try_get("signature_tolerance_secs").map_err(sqlx_err)?,
            webhook_ct: r.try_get("webhook_secret_ciphertext").map_err(sqlx_err)?,
            webhook_nonce: r.try_get("webhook_secret_nonce").map_err(sqlx_err)?,
            api_ct: r.try_get("api_key_ciphertext").map_err(sqlx_err)?,
            api_nonce: r.try_get("api_key_nonce").map_err(sqlx_err)?,
            updated_at: r.try_get("updated_at_micros").map_err(sqlx_err)?,
        }))
    }

    fn row_to_config(&self, row: &SealedRow) -> Result<BillingConfig> {
        let (webhook_secret, webhook_secret_set) = match (&row.webhook_nonce, &row.webhook_ct) {
            (Some(n), Some(c)) => (self.open(n, c, "webhook_secret")?, true),
            _ => (String::new(), false),
        };
        let (api_key, api_key_set) = match (&row.api_nonce, &row.api_ct) {
            (Some(n), Some(c)) => (self.open(n, c, "api_key")?, true),
            _ => (String::new(), false),
        };
        Ok(BillingConfig {
            enabled: row.enabled,
            signature_tolerance_secs: row.tolerance,
            webhook_secret,
            api_key,
            webhook_secret_set,
            api_key_set,
            updated_at: TimestampMicros(row.updated_at),
        })
    }
}

#[async_trait]
impl BillingSettingsRepository for PgBillingSettingsRepository {
    async fn get(&self) -> Result<BillingConfig> {
        match self.fetch_row().await? {
            Some(row) => self.row_to_config(&row),
            None => Ok(BillingConfig::default()),
        }
    }

    async fn set(&self, input: BillingSettingsInput) -> Result<BillingConfig> {
        let existing = self.fetch_row().await?;
        // 计算每个 secret 的最终 (nonce, ct)：None=保留、Some("")=清空、Some(s)=替换。
        let webhook = resolve_secret(
            input.webhook_secret.as_deref(),
            existing.as_ref().map(|r| (&r.webhook_nonce, &r.webhook_ct)),
            |s| self.seal(s),
        )?;
        let api = resolve_secret(
            input.api_key.as_deref(),
            existing.as_ref().map(|r| (&r.api_nonce, &r.api_ct)),
            |s| self.seal(s),
        )?;
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO billing_settings
                (id, enabled, signature_tolerance_secs,
                 webhook_secret_ciphertext, webhook_secret_nonce,
                 api_key_ciphertext, api_key_nonce, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                signature_tolerance_secs = EXCLUDED.signature_tolerance_secs,
                webhook_secret_ciphertext = EXCLUDED.webhook_secret_ciphertext,
                webhook_secret_nonce = EXCLUDED.webhook_secret_nonce,
                api_key_ciphertext = EXCLUDED.api_key_ciphertext,
                api_key_nonce = EXCLUDED.api_key_nonce,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(SINGLETON_ID)
        .bind(input.enabled)
        .bind(input.signature_tolerance_secs)
        .bind(webhook.as_ref().map(|(_, ct)| ct.clone()))
        .bind(webhook.as_ref().map(|(nonce, _)| nonce.clone()))
        .bind(api.as_ref().map(|(_, ct)| ct.clone()))
        .bind(api.as_ref().map(|(nonce, _)| nonce.clone()))
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get().await
    }
}

/// 一条已封存 secret 的 `(nonce, ciphertext)`。
type SealedSecret = (Vec<u8>, Vec<u8>);
/// 既有行里某 secret 的两列引用 `(nonce, ciphertext)`，各列可空。
type ExistingSecretCols<'a> = (&'a Option<Vec<u8>>, &'a Option<Vec<u8>>);

/// 决定某 secret 最终落库的 `(nonce, ciphertext)`：
/// - `desired = None` → 保留 `existing`（若有）；
/// - `desired = Some("")` → 清空（返回 None）；
/// - `desired = Some(s)` → seal s。
fn resolve_secret(
    desired: Option<&str>,
    existing: Option<ExistingSecretCols<'_>>,
    seal: impl Fn(&str) -> Result<SealedSecret>,
) -> Result<Option<SealedSecret>> {
    match desired {
        None => Ok(existing.and_then(|(nonce, ct)| match (nonce, ct) {
            (Some(n), Some(c)) => Some((n.clone(), c.clone())),
            _ => None,
        })),
        Some("") => Ok(None),
        Some(s) => Ok(Some(seal(s)?)),
    }
}
