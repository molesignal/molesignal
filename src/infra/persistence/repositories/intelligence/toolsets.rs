// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `intelligence_toolsets` 表 Pg 实装。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolset {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// JSON schema 描述 tool 入口，供 Intelligence dispatcher 透传使用。
    pub schema: Value,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait AgentToolsetRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<AgentToolset>>;
    async fn create(&self, t: AgentToolset) -> Result<AgentToolset>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgAgentToolsetRepository {
    pool: PgPool,
}

impl PgAgentToolsetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, schema, enabled, created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> AgentToolset {
    let schema: Json<Value> = r
        .try_get("schema")
        .unwrap_or(Json(Value::Object(Default::default())));
    AgentToolset {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        schema: schema.0,
        enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl AgentToolsetRepository for PgAgentToolsetRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<AgentToolset>> {
        let sql = format!(
            "SELECT {COLS} FROM intelligence_toolsets WHERE org_id = $1 ORDER BY updated_at_micros DESC, name ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn create(&self, t: AgentToolset) -> Result<AgentToolset> {
        sqlx::query(
            "INSERT INTO intelligence_toolsets
                (id, org_id, name, schema, enabled, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&t.id.0)
        .bind(&t.org_id.0)
        .bind(&t.name)
        .bind(Json(&t.schema))
        .bind(t.enabled)
        .bind(t.created_at.0)
        .bind(t.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(t)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM intelligence_toolsets WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

/// 测试和极简嵌入场景可继续使用空仓储；正常 bootstrap 阶段使用 Pg 实装。
pub struct EmptyAgentToolsetRepository;

impl EmptyAgentToolsetRepository {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmptyAgentToolsetRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentToolsetRepository for EmptyAgentToolsetRepository {
    async fn list(&self, _org_id: &Id) -> Result<Vec<AgentToolset>> {
        Ok(Vec::new())
    }
    async fn create(&self, _t: AgentToolset) -> Result<AgentToolset> {
        Err(Error::forbidden(
            "intelligence_toolsets repository is not configured",
        ))
    }
    async fn delete(&self, _org_id: &Id, _id: &Id) -> Result<()> {
        Err(Error::forbidden(
            "intelligence_toolsets repository is not configured",
        ))
    }
}
