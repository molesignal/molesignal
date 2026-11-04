// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `connectors` 表 CRUD。
//!
//! 内存里的 [`Connector::config_json`] 是完整配置；落库时密钥字段经 [`super::secrets`] 用根密钥
//! 封存到 `secrets_*` 列，推送令牌另存 SHA-256 供推送端点反查。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::secrets;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    pub enabled: bool,
    pub last_run_at: Option<TimestampMicros>,
    /// Where a pull source has read to, and how its last pull went (see [`ReadState`]). Not part of
    /// the settings: saving a connector leaves it.
    #[serde(default)]
    pub read_state: Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ConnectorRepository: Send + Sync {
    async fn create(&self, c: Connector) -> Result<Connector>;
    async fn update(&self, c: Connector) -> Result<Connector>;
    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()>;
    /// Saves what a pull left: its read state, and when it succeeded if it did.
    async fn save_read_state(
        &self,
        id: &Id,
        succeeded_at: Option<TimestampMicros>,
        read_state: &Value,
    ) -> Result<()>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Connector>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Connector>>;
    async fn list_enabled(&self) -> Result<Vec<Connector>>;
    /// 按 kind + 推送令牌（比对其 SHA-256）查 enabled 的 push 型 connector（推送接入端点鉴权）。
    async fn find_for_push(&self, kind: &str, push_token: &str) -> Result<Option<Connector>>;
}

pub struct PgConnectorRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgConnectorRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    fn row_to(&self, r: sqlx::postgres::PgRow) -> Result<Connector> {
        let cfg: Json<Value> = r.try_get("config_json").unwrap_or(Json(Value::Null));
        let nonce: Option<Vec<u8>> = r.try_get("secrets_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = r.try_get("secrets_ciphertext").map_err(sqlx_err)?;
        let envelope = nonce.as_deref().zip(ciphertext.as_deref());

        Ok(Connector {
            id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
            org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
            name: r.try_get::<String, _>("name").unwrap_or_default(),
            kind: r.try_get::<String, _>("kind").unwrap_or_default(),
            config_json: secrets::open(&self.cipher, cfg.0, envelope)?,
            enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
            last_run_at: r
                .try_get::<Option<TimestampMicros>, _>("last_run_at")
                .unwrap_or_default(),
            read_state: r
                .try_get::<Json<Value>, _>("read_state")
                .map(|state| state.0)
                .unwrap_or(Value::Null),
            created_at: r
                .try_get::<TimestampMicros, _>("created_at")
                .unwrap_or_default(),
            updated_at: r
                .try_get::<TimestampMicros, _>("updated_at")
                .unwrap_or_default(),
        })
    }
}

const COLS: &str = "id, org_id, name, kind, config_json, secrets_ciphertext, secrets_nonce,
                    enabled, last_run_at, read_state, created_at, updated_at";

fn sqlx_err(error: sqlx::Error) -> Error {
    super::super::persistence::sqlx_err(error)
}

/// A write refused by the push token's unique index says so, rather than naming the index.
fn write_err(error: sqlx::Error) -> Error {
    match sqlx_err(error) {
        Error::Conflict(message) if message.contains("uq_connector_push_token") => {
            Error::conflict("this push token is already in use: generate a new one")
        }
        other => other,
    }
}

#[async_trait]
impl ConnectorRepository for PgConnectorRepository {
    async fn create(&self, c: Connector) -> Result<Connector> {
        let sealed = secrets::seal(&self.cipher, &c.config_json)?;
        let (nonce, ciphertext) = sealed.envelope.unzip();

        sqlx::query(
            "INSERT INTO connectors
                (id, org_id, name, kind, config_json, secrets_ciphertext, secrets_nonce,
                 push_token_sha256, enabled, last_run_at, created_at,
                 updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11)",
        )
        .bind(&c.id.0)
        .bind(&c.org_id.0)
        .bind(&c.name)
        .bind(&c.kind)
        .bind(Json(&sealed.config_json))
        .bind(ciphertext)
        .bind(nonce)
        .bind(sealed.push_token_sha256)
        .bind(c.enabled)
        .bind(c.created_at)
        .bind(c.updated_at)
        .execute(&self.pool)
        .await
        .map_err(write_err)?;

        Ok(c)
    }

    async fn update(&self, c: Connector) -> Result<Connector> {
        let sealed = secrets::seal(&self.cipher, &c.config_json)?;
        let (nonce, ciphertext) = sealed.envelope.unzip();

        sqlx::query(
            "UPDATE connectors SET
                name = $3,
                kind = $4,
                config_json = $5,
                secrets_ciphertext = $6,
                secrets_nonce = $7,
                push_token_sha256 = $8,
                enabled = $9,
                updated_at = $10
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&c.id.0)
        .bind(&c.org_id.0)
        .bind(&c.name)
        .bind(&c.kind)
        .bind(Json(&sealed.config_json))
        .bind(ciphertext)
        .bind(nonce)
        .bind(sealed.push_token_sha256)
        .bind(c.enabled)
        .bind(c.updated_at)
        .execute(&self.pool)
        .await
        .map_err(write_err)?;

        Ok(c)
    }

    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE connectors SET last_run_at = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(ts)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn save_read_state(
        &self,
        id: &Id,
        succeeded_at: Option<TimestampMicros>,
        read_state: &Value,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE connectors
                SET read_state = $2,
                    last_run_at = COALESCE($3, last_run_at)
              WHERE id = $1",
        )
        .bind(&id.0)
        .bind(Json(read_state))
        .bind(succeeded_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Connector> {
        let sql = format!("SELECT {COLS} FROM connectors WHERE org_id = $1 AND id = $2");

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        self.row_to(row)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM connectors WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Connector>> {
        let sql = format!("SELECT {COLS} FROM connectors WHERE org_id = $1 ORDER BY name");

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(|row| self.row_to(row)).collect()
    }

    async fn list_enabled(&self) -> Result<Vec<Connector>> {
        let sql =
            format!("SELECT {COLS} FROM connectors WHERE enabled = TRUE ORDER BY org_id, name");

        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(|row| self.row_to(row)).collect()
    }

    async fn find_for_push(&self, kind: &str, push_token: &str) -> Result<Option<Connector>> {
        let sql = format!(
            "SELECT {COLS} FROM connectors
             WHERE kind = $1 AND enabled = TRUE AND push_token_sha256 = $2"
        );

        let row = sqlx::query(&sql)
            .bind(kind)
            .bind(secrets::push_token_digest(push_token))
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row.map(|row| self.row_to(row)).transpose()
    }
}
