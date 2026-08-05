// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `connectors` 表 CRUD。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connector {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    pub enabled: bool,
    pub last_run_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ConnectorRepository: Send + Sync {
    async fn create(&self, c: Connector) -> Result<Connector>;
    async fn update(&self, c: Connector) -> Result<Connector>;
    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Connector>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Connector>>;
    async fn list_enabled(&self) -> Result<Vec<Connector>>;
    /// 按 kind + `config_json.push_token` 查 enabled 的 push 型 connector（推送接入端点鉴权）。
    async fn find_for_push(&self, kind: &str, push_token: &str) -> Result<Option<Connector>>;
}

pub struct PgConnectorRepository {
    pool: PgPool,
}

impl PgConnectorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, kind, config_json, enabled, last_run_at_micros,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Connector {
    let cfg: Json<Value> = r.try_get("config_json").unwrap_or(Json(Value::Null));
    Connector {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        kind: r.try_get::<String, _>("kind").unwrap_or_default(),
        config_json: cfg.0,
        enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
        last_run_at: r
            .try_get::<Option<i64>, _>("last_run_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl ConnectorRepository for PgConnectorRepository {
    async fn create(&self, c: Connector) -> Result<Connector> {
        sqlx::query(
            "INSERT INTO connectors
                (id, org_id, name, kind, config_json, enabled, last_run_at_micros,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8)",
        )
        .bind(&c.id.0)
        .bind(&c.org_id.0)
        .bind(&c.name)
        .bind(&c.kind)
        .bind(Json(&c.config_json))
        .bind(c.enabled)
        .bind(c.created_at.0)
        .bind(c.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(c)
    }

    async fn update(&self, c: Connector) -> Result<Connector> {
        sqlx::query(
            "UPDATE connectors SET
                name = $3,
                kind = $4,
                config_json = $5,
                enabled = $6,
                updated_at_micros = $7
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&c.id.0)
        .bind(&c.org_id.0)
        .bind(&c.name)
        .bind(&c.kind)
        .bind(Json(&c.config_json))
        .bind(c.enabled)
        .bind(c.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(c)
    }

    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE connectors SET last_run_at_micros = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(ts.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Connector> {
        let sql = format!("SELECT {COLS} FROM connectors WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(row_to(row))
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM connectors WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Connector>> {
        let sql = format!("SELECT {COLS} FROM connectors WHERE org_id = $1 ORDER BY name");
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_enabled(&self) -> Result<Vec<Connector>> {
        let sql =
            format!("SELECT {COLS} FROM connectors WHERE enabled = TRUE ORDER BY org_id, name");
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn find_for_push(&self, kind: &str, push_token: &str) -> Result<Option<Connector>> {
        let sql = format!(
            "SELECT {COLS} FROM connectors
             WHERE kind = $1 AND enabled = TRUE AND config_json->>'push_token' = $2
             LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(kind)
            .bind(push_token)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::super::persistence::sqlx_err)?;
        Ok(row.map(row_to))
    }
}
