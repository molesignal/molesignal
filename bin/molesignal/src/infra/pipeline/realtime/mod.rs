// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tenant-scoped realtime projection of pipeline configurations.
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use crate::{
    domain::pipeline::realtime::{RealtimePipeline, RealtimePipelineRepository, validate_config},
    shared::{Error, Result, ids::Id},
};

pub struct PgRealtimePipelineRepository {
    pool: PgPool,
}
impl PgRealtimePipelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RealtimePipelineRepository for PgRealtimePipelineRepository {
    async fn for_source(&self, org: &Id, source: &str) -> Result<Option<RealtimePipeline>> {
        let rows = sqlx::query("SELECT id, target_stream, function_steps, updated_at_micros FROM scheduled_pipelines WHERE org_id = $1 AND source_stream = $2 AND enabled = TRUE AND function_steps->>'mode' = 'realtime' LIMIT 2")
            .bind(org.as_str()).bind(source).fetch_all(&self.pool).await.map_err(crate::infra::persistence::sqlx_err)?;
        if rows.len() > 1 {
            return Err(Error::conflict(
                "only one realtime pipeline may be enabled per source",
            ));
        }
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let target: String = row.get("target_stream");
        let config: Json<Value> = row.get("function_steps");
        let routing = validate_config(&config.0, source, &target)?
            .ok_or_else(|| Error::internal("invalid realtime projection"))?;
        let steps = crate::domain::pipeline::processing::parse_processing_steps(&config.0)?;
        Ok(Some(RealtimePipeline {
            id: Id(row.get("id")),
            org_id: org.clone(),
            target,
            routing,
            steps,
            updated_at: crate::shared::time::TimestampMicros(row.get("updated_at_micros")),
        }))
    }
}
