// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Serialize same-organization configuration mutations to avoid competing realtime routes.
use sqlx::{PgPool, TracedTransaction};

use super::repository::ScheduledPipeline;
use crate::{
    domain::pipeline::realtime::validate_config,
    infra::persistence::sqlx_err,
    shared::{Error, Result},
};

pub(super) async fn lock_and_validate(
    pool: &PgPool,
    p: &ScheduledPipeline,
) -> Result<TracedTransaction<'static>> {
    let realtime =
        validate_config(&p.function_steps, &p.source_stream, &p.target_stream)?.is_some();
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("pipeline-config:{}", p.org_id))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    if realtime && p.enabled {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM scheduled_pipelines WHERE org_id = $1 AND source_stream = $2 AND id <> $3 AND enabled = TRUE AND function_steps->>'mode' = 'realtime')")
            .bind(p.org_id.as_str()).bind(&p.source_stream).bind(p.id.as_str()).fetch_one(&mut *tx).await.map_err(sqlx_err)?;
        if exists {
            return Err(Error::conflict(
                "a realtime pipeline is already enabled for this source",
            ));
        }
    }
    Ok(tx)
}
