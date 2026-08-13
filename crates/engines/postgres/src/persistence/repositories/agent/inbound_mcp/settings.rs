// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::inbound_mcp::InboundMcpSettings,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "org_id,enabled,allowed_origins,max_request_bytes,max_response_bytes,\
    max_concurrent_calls,calls_per_minute,read_timeout_ms,updated_by,\
    created_at_micros,updated_at_micros";

fn row_to_settings(row: sqlx::postgres::PgRow) -> Result<InboundMcpSettings> {
    let origins: Json<Value> = row.try_get("allowed_origins").map_err(sqlx_err)?;
    let allowed_origins = serde_json::from_value(origins.0)
        .map_err(|error| Error::internal(format!("invalid inbound MCP origins: {error}")))?;
    Ok(InboundMcpSettings {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        allowed_origins,
        max_request_bytes: row.try_get("max_request_bytes").map_err(sqlx_err)?,
        max_response_bytes: row.try_get("max_response_bytes").map_err(sqlx_err)?,
        max_concurrent_calls: row.try_get("max_concurrent_calls").map_err(sqlx_err)?,
        calls_per_minute: row.try_get("calls_per_minute").map_err(sqlx_err)?,
        read_timeout_ms: row.try_get("read_timeout_ms").map_err(sqlx_err)?,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn get(pool: &PgPool, org_id: &Id) -> Result<Option<InboundMcpSettings>> {
    sqlx::query(&format!(
        "SELECT {COLS} FROM inbound_mcp_settings WHERE org_id=$1"
    ))
    .bind(&org_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(row_to_settings)
    .transpose()
}

pub(super) async fn upsert(
    pool: &PgPool,
    settings: InboundMcpSettings,
) -> Result<InboundMcpSettings> {
    let row = sqlx::query(&format!(
        "INSERT INTO inbound_mcp_settings ({COLS})
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (org_id) DO UPDATE SET
            enabled=EXCLUDED.enabled,allowed_origins=EXCLUDED.allowed_origins,
            max_request_bytes=EXCLUDED.max_request_bytes,
            max_response_bytes=EXCLUDED.max_response_bytes,
            max_concurrent_calls=EXCLUDED.max_concurrent_calls,
            calls_per_minute=EXCLUDED.calls_per_minute,
            read_timeout_ms=EXCLUDED.read_timeout_ms,updated_by=EXCLUDED.updated_by,
            updated_at_micros=EXCLUDED.updated_at_micros
         RETURNING {COLS}"
    ))
    .bind(&settings.org_id.0)
    .bind(settings.enabled)
    .bind(Json(&settings.allowed_origins))
    .bind(settings.max_request_bytes)
    .bind(settings.max_response_bytes)
    .bind(settings.max_concurrent_calls)
    .bind(settings.calls_per_minute)
    .bind(settings.read_timeout_ms)
    .bind(&settings.updated_by.0)
    .bind(settings.created_at.0)
    .bind(settings.updated_at.0)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;
    row_to_settings(row)
}
