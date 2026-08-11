// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `scheduled_pipelines` 表 CRUD。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledPipeline {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub source_stream: String,
    pub target_stream: String,
    pub function_steps: Value,
    pub cron: String,
    pub lookback_secs: i32,
    pub last_run_at: Option<TimestampMicros>,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ScheduledPipelineRepository: Send + Sync {
    async fn create(&self, p: ScheduledPipeline) -> Result<ScheduledPipeline>;
    async fn update(&self, p: ScheduledPipeline) -> Result<ScheduledPipeline>;
    async fn delete(&self, org: &Id, id: &Id) -> Result<()>;
    async fn get_by_id(&self, id: &Id) -> Result<ScheduledPipeline>;
    async fn get(&self, org: &Id, id: &Id) -> Result<ScheduledPipeline>;
    async fn list(&self, org: &Id) -> Result<Vec<ScheduledPipeline>>;
    async fn list_enabled_all(&self) -> Result<Vec<ScheduledPipeline>>;
    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()>;
}

pub struct PgScheduledPipelineRepository {
    pool: PgPool,
}

impl PgScheduledPipelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, source_stream, target_stream, function_steps, cron,
                    lookback_secs, last_run_at_micros, enabled,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> ScheduledPipeline {
    let steps: Json<Value> = r.try_get("function_steps").unwrap_or(Json(Value::Null));
    ScheduledPipeline {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        source_stream: r.try_get::<String, _>("source_stream").unwrap_or_default(),
        target_stream: r.try_get::<String, _>("target_stream").unwrap_or_default(),
        function_steps: steps.0,
        cron: r.try_get::<String, _>("cron").unwrap_or_default(),
        lookback_secs: r.try_get::<i32, _>("lookback_secs").unwrap_or(300),
        last_run_at: r
            .try_get::<Option<i64>, _>("last_run_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl ScheduledPipelineRepository for PgScheduledPipelineRepository {
    async fn create(&self, p: ScheduledPipeline) -> Result<ScheduledPipeline> {
        sqlx::query(
            "INSERT INTO scheduled_pipelines
                (id, org_id, name, source_stream, target_stream, function_steps, cron,
                 lookback_secs, last_run_at_micros, enabled,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $10, $11)",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&p.source_stream)
        .bind(&p.target_stream)
        .bind(Json(&p.function_steps))
        .bind(&p.cron)
        .bind(p.lookback_secs)
        .bind(p.enabled)
        .bind(p.created_at.0)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(p)
    }

    async fn update(&self, p: ScheduledPipeline) -> Result<ScheduledPipeline> {
        sqlx::query(
            "UPDATE scheduled_pipelines SET
                name = $3,
                source_stream = $4,
                target_stream = $5,
                function_steps = $6,
                cron = $7,
                lookback_secs = $8,
                enabled = $9,
                updated_at_micros = $10
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&p.source_stream)
        .bind(&p.target_stream)
        .bind(Json(&p.function_steps))
        .bind(&p.cron)
        .bind(p.lookback_secs)
        .bind(p.enabled)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(p)
    }

    async fn delete(&self, org: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM scheduled_pipelines WHERE org_id = $1 AND id = $2")
            .bind(&org.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn get_by_id(&self, id: &Id) -> Result<ScheduledPipeline> {
        let sql = format!("SELECT {COLS} FROM scheduled_pipelines WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(row_to(row))
    }

    async fn get(&self, org: &Id, id: &Id) -> Result<ScheduledPipeline> {
        let sql = format!("SELECT {COLS} FROM scheduled_pipelines WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org: &Id) -> Result<Vec<ScheduledPipeline>> {
        let sql = format!("SELECT {COLS} FROM scheduled_pipelines WHERE org_id = $1 ORDER BY name");
        let rows = sqlx::query(&sql)
            .bind(&org.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_enabled_all(&self) -> Result<Vec<ScheduledPipeline>> {
        let sql = format!(
            "SELECT {COLS} FROM scheduled_pipelines WHERE enabled = TRUE ORDER BY org_id, name"
        );
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE scheduled_pipelines SET last_run_at_micros = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(ts.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(())
    }
}
