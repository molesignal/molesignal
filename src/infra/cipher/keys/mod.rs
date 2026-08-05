// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cipher keys CRUD：
//!
//! 行内 `key_material_enc` 是被 `CipherRootKey`（KEK，envelope 包）包过的 AES-256
//! raw key（32B）+ tag。rotation 时插入新 version（旧 version 仍存，便于 decrypt
//! 历史数据）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use self::root::CipherRootKey;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

pub mod root;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CipherKey {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub alg: String,
    pub version: i32,
    /// 解密后的明文 raw key（32B）。**永不出库**——handler 必须 mask。
    #[serde(skip_serializing)]
    pub raw_key: Vec<u8>,
    pub created_at: TimestampMicros,
    pub rotated_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait CipherKeyRepository: Send + Sync {
    async fn create(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey>;
    /// rotation：插入 `version = max+1`，复用同 name。
    async fn rotate(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey>;
    async fn get_latest(&self, org_id: &Id, name: &str) -> Result<CipherKey>;
    async fn get_by_id_version(&self, org_id: &Id, key_id: &str, version: i32)
    -> Result<CipherKey>;
    async fn list(&self, org_id: &Id) -> Result<Vec<CipherKey>>;
    async fn delete(&self, org_id: &Id, name: &str) -> Result<()>;
}

pub struct PgCipherKeyRepository {
    pool: PgPool,
    mk: CipherRootKey,
}

impl PgCipherKeyRepository {
    pub fn new(pool: PgPool, mk: CipherRootKey) -> Self {
        Self { pool, mk }
    }
}

fn row_to(r: sqlx::postgres::PgRow, mk: &CipherRootKey) -> Result<CipherKey> {
    use crate::shared::Error;
    let enc: Vec<u8> = r
        .try_get("key_material_enc")
        .map_err(|e| Error::internal(e.to_string()))?;
    let nonce: Vec<u8> = r
        .try_get("nonce")
        .map_err(|e| Error::internal(e.to_string()))?;
    let raw = mk
        .open(&nonce, &enc)
        .map_err(|e| Error::internal(format!("cipher key open: {e}")))?;
    Ok(CipherKey {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        alg: r.try_get::<String, _>("alg").unwrap_or_default(),
        version: r.try_get::<i32, _>("version").unwrap_or(1),
        raw_key: raw,
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        rotated_at: r
            .try_get::<Option<i64>, _>("rotated_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
    })
}

const COLS: &str = "id, org_id, name, alg, version, key_material_enc, nonce,
                    created_at_micros, rotated_at_micros";

#[async_trait]
impl CipherKeyRepository for PgCipherKeyRepository {
    async fn create(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
        use crate::shared::Error;
        if raw_key.len() != 32 {
            return Err(Error::invalid("cipher key material must be 32 bytes"));
        }
        let (nonce, enc) = self
            .mk
            .seal(raw_key)
            .map_err(|e| Error::internal(format!("cipher seal: {e}")))?;
        let id = Id::new();
        let created = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO cipher_keys
                (id, org_id, name, alg, version, key_material_enc, nonce,
                 created_at_micros, rotated_at_micros)
             VALUES ($1, $2, $3, 'aes-256-gcm', 1, $4, $5, $6, NULL)",
        )
        .bind(&id.0)
        .bind(&org_id.0)
        .bind(name)
        .bind(&enc)
        .bind(&nonce)
        .bind(created.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(CipherKey {
            id,
            org_id: org_id.clone(),
            name: name.to_string(),
            alg: "aes-256-gcm".to_string(),
            version: 1,
            raw_key: raw_key.to_vec(),
            created_at: created,
            rotated_at: None,
        })
    }

    async fn rotate(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
        use crate::shared::Error;
        if raw_key.len() != 32 {
            return Err(Error::invalid("cipher key material must be 32 bytes"));
        }
        // 当前 max version + 1
        let row = sqlx::query(
            "SELECT COALESCE(MAX(version), 0) AS v FROM cipher_keys WHERE org_id = $1 AND name = $2",
        )
        .bind(&org_id.0)
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        let next: i32 = row.try_get("v").unwrap_or(0) + 1;
        let (nonce, enc) = self
            .mk
            .seal(raw_key)
            .map_err(|e| Error::internal(format!("cipher seal: {e}")))?;
        let id = Id::new();
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO cipher_keys
                (id, org_id, name, alg, version, key_material_enc, nonce,
                 created_at_micros, rotated_at_micros)
             VALUES ($1, $2, $3, 'aes-256-gcm', $4, $5, $6, $7, $7)",
        )
        .bind(&id.0)
        .bind(&org_id.0)
        .bind(name)
        .bind(next)
        .bind(&enc)
        .bind(&nonce)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(CipherKey {
            id,
            org_id: org_id.clone(),
            name: name.to_string(),
            alg: "aes-256-gcm".to_string(),
            version: next,
            raw_key: raw_key.to_vec(),
            created_at: now,
            rotated_at: Some(now),
        })
    }

    async fn get_latest(&self, org_id: &Id, name: &str) -> Result<CipherKey> {
        let sql = format!(
            "SELECT {COLS} FROM cipher_keys
             WHERE org_id = $1 AND name = $2
             ORDER BY version DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        row_to(row, &self.mk)
    }

    async fn get_by_id_version(
        &self,
        org_id: &Id,
        key_id: &str,
        version: i32,
    ) -> Result<CipherKey> {
        let sql = format!(
            "SELECT {COLS} FROM cipher_keys
             WHERE org_id = $1 AND id = $2 AND version = $3"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(key_id)
            .bind(version)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        row_to(row, &self.mk)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<CipherKey>> {
        let sql = format!(
            "SELECT {COLS} FROM cipher_keys
             WHERE org_id = $1 ORDER BY name, version DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        rows.into_iter().map(|r| row_to(r, &self.mk)).collect()
    }

    async fn delete(&self, org_id: &Id, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM cipher_keys WHERE org_id = $1 AND name = $2")
            .bind(&org_id.0)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(())
    }
}
