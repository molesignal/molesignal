// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `incident_rca` 表 Pg 实装：AI 根因分析持久化（每 incident 一条，upsert）。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::alerting::{incident::IncidentRca, repositories::IncidentRcaRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgIncidentRcaRepository {
    pool: PgPool,
}

impl PgIncidentRcaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "incident_id, org_id, summary, provider, model, prompt_builtin_key, \
    prompt_hash, prompt_tokens, completion_tokens, finish_reason, \
    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> IncidentRca {
    IncidentRca {
        incident_id: Id(r.try_get::<String, _>("incident_id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        summary: r.try_get("summary").unwrap_or_default(),
        provider: r.try_get("provider").ok(),
        model: r.try_get("model").ok(),
        prompt_builtin_key: r.try_get("prompt_builtin_key").ok(),
        prompt_hash: r.try_get("prompt_hash").ok(),
        prompt_tokens: r.try_get::<i32, _>("prompt_tokens").unwrap_or_default(),
        completion_tokens: r.try_get::<i32, _>("completion_tokens").unwrap_or_default(),
        finish_reason: r.try_get("finish_reason").ok(),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl IncidentRcaRepository for PgIncidentRcaRepository {
    async fn get(&self, incident_id: &Id) -> Result<Option<IncidentRca>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM incident_rca WHERE incident_id = $1"
        ))
        .bind(&incident_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn upsert(&self, rca: IncidentRca) -> Result<IncidentRca> {
        sqlx::query(
            "INSERT INTO incident_rca
                (incident_id, org_id, summary, provider, model, prompt_builtin_key,
                 prompt_hash, prompt_tokens, completion_tokens, finish_reason,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (incident_id) DO UPDATE SET
                summary = EXCLUDED.summary,
                provider = EXCLUDED.provider,
                model = EXCLUDED.model,
                prompt_builtin_key = EXCLUDED.prompt_builtin_key,
                prompt_hash = EXCLUDED.prompt_hash,
                prompt_tokens = EXCLUDED.prompt_tokens,
                completion_tokens = EXCLUDED.completion_tokens,
                finish_reason = EXCLUDED.finish_reason,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&rca.incident_id.0)
        .bind(&rca.org_id.0)
        .bind(&rca.summary)
        .bind(&rca.provider)
        .bind(&rca.model)
        .bind(&rca.prompt_builtin_key)
        .bind(&rca.prompt_hash)
        .bind(rca.prompt_tokens)
        .bind(rca.completion_tokens)
        .bind(&rca.finish_reason)
        .bind(rca.created_at.0)
        .bind(rca.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(self.get(&rca.incident_id).await?.unwrap_or(rca))
    }
}
