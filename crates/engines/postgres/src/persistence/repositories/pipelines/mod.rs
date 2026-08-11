// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `pipelines` 表 Pg 实装。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::{
    sqlx_err,
    streams::{stream_type_from_str, stream_type_to_str},
};
use crate::{
    domain::{
        pipeline::{Pipeline, PipelineRepository, PipelineStep},
        stream::{StreamType, is_reserved_system_stream},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod runs;

pub struct PgPipelineRepository {
    pool: PgPool,
}

impl PgPipelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn validate_pipeline_target(stream: &str) -> Result<()> {
    if is_reserved_system_stream(stream) {
        return Err(Error::forbidden(
            "`_molesignal` cannot be a pipeline target",
        ));
    }
    Ok(())
}

/// `stream_target_hash` 列约定：`org:stream:type` 字符串原文（DB 端只看唯一约束，
/// 不在 SQL 层做 hash 函数；保留单向可读）。
fn target_hash(org_id: &Id, stream: &str, st: StreamType) -> String {
    format!("{}:{}:{}", org_id.0, stream, stream_type_to_str(st))
}

const COLS: &str = "id, org_id, name, stream_target_hash, steps, enabled,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Result<Pipeline> {
    let steps: Json<Vec<PipelineStep>> = r.try_get("steps").unwrap_or(Json(Vec::new()));
    // stream_target_hash 反解 `org:stream:type` → (_org, stream, st)
    let target: String = r.try_get("stream_target_hash").unwrap_or_default();
    let mut parts = target.splitn(3, ':');
    let _org = parts.next().unwrap_or_default();
    let stream_name = parts.next().unwrap_or_default().to_string();
    let st_str = parts.next().unwrap_or("logs");
    let st = stream_type_from_str(st_str).unwrap_or(StreamType::Logs);
    Ok(Pipeline {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        stream_name,
        stream_type: st,
        steps: steps.0,
        enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    })
}

#[async_trait]
impl PipelineRepository for PgPipelineRepository {
    async fn create(&self, p: Pipeline) -> Result<Pipeline> {
        validate_pipeline_target(&p.stream_name)?;
        let hash = target_hash(&p.org_id, &p.stream_name, p.stream_type);
        sqlx::query(
            "INSERT INTO pipelines
                (id, org_id, name, stream_target_hash, steps, enabled,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&hash)
        .bind(Json(&p.steps))
        .bind(p.enabled)
        .bind(p.created_at.0)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn update(&self, p: Pipeline) -> Result<Pipeline> {
        validate_pipeline_target(&p.stream_name)?;
        let hash = target_hash(&p.org_id, &p.stream_name, p.stream_type);
        sqlx::query(
            "UPDATE pipelines SET
                name = $3, stream_target_hash = $4, steps = $5, enabled = $6,
                updated_at_micros = $7
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&hash)
        .bind(Json(&p.steps))
        .bind(p.enabled)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Pipeline> {
        let sql = format!("SELECT {COLS} FROM pipelines WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Pipeline>> {
        let sql = format!("SELECT {COLS} FROM pipelines WHERE org_id = $1 ORDER BY name");
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn list_for_stream(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
    ) -> Result<Vec<Pipeline>> {
        let hash = target_hash(org_id, stream, stream_type);
        let sql = format!(
            "SELECT {COLS} FROM pipelines
             WHERE org_id = $1 AND stream_target_hash = $2 AND enabled = TRUE
             ORDER BY name"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&hash)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM pipelines WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_system_stream_cannot_be_a_pipeline_target() {
        assert!(matches!(
            validate_pipeline_target("_molesignal"),
            Err(Error::Forbidden(_))
        ));
        assert!(validate_pipeline_target("_custom").is_ok());
    }
}
