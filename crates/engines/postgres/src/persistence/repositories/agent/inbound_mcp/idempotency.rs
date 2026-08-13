// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::inbound_mcp::{
        InboundMcpIdempotencyInput, InboundMcpIdempotencyRecord, InboundMcpIdempotencyReservation,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "org_id,principal_type,principal_id,idempotency_key,tool_name,request_hash,\
    status,approval_id,result,lease_expires_at_micros,created_at_micros,updated_at_micros";

fn record_row(row: sqlx::postgres::PgRow) -> Result<InboundMcpIdempotencyRecord> {
    Ok(InboundMcpIdempotencyRecord {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        principal_type: row.try_get("principal_type").map_err(sqlx_err)?,
        principal_id: Id(row.try_get("principal_id").map_err(sqlx_err)?),
        idempotency_key: row.try_get("idempotency_key").map_err(sqlx_err)?,
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        request_hash: row.try_get("request_hash").map_err(sqlx_err)?,
        status: row.try_get("status").map_err(sqlx_err)?,
        approval_id: row
            .try_get::<Option<String>, _>("approval_id")
            .map_err(sqlx_err)?
            .map(Id),
        result: row
            .try_get::<Option<Json<Value>>, _>("result")
            .map_err(sqlx_err)?
            .map(|value| value.0),
        lease_expires_at: TimestampMicros(
            row.try_get("lease_expires_at_micros").map_err(sqlx_err)?,
        ),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn reserve(
    pool: &PgPool,
    input: InboundMcpIdempotencyInput,
) -> Result<InboundMcpIdempotencyReservation> {
    let now = TimestampMicros::now();
    let inserted = sqlx::query(
        "INSERT INTO inbound_mcp_idempotency
            (org_id,principal_type,principal_id,idempotency_key,tool_name,request_hash,status,
             approval_id,result,lease_expires_at_micros,created_at_micros,updated_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,'pending',NULL,NULL,$7,$8,$8)
         ON CONFLICT (org_id,principal_type,principal_id,idempotency_key) DO NOTHING",
    )
    .bind(&input.org_id.0)
    .bind(&input.principal_type)
    .bind(&input.principal_id.0)
    .bind(&input.idempotency_key)
    .bind(&input.tool_name)
    .bind(&input.request_hash)
    .bind(input.lease_expires_at.0)
    .bind(now.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    if inserted.rows_affected() == 1 {
        return Ok(InboundMcpIdempotencyReservation::Acquired);
    }

    let existing = sqlx::query(&format!(
        "SELECT {COLS} FROM inbound_mcp_idempotency
         WHERE org_id=$1 AND principal_type=$2 AND principal_id=$3 AND idempotency_key=$4"
    ))
    .bind(&input.org_id.0)
    .bind(&input.principal_type)
    .bind(&input.principal_id.0)
    .bind(&input.idempotency_key)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)
    .and_then(record_row)?;
    if existing.request_hash != input.request_hash || existing.tool_name != input.tool_name {
        return Err(Error::conflict(
            "idempotency_key was already used for a different managed tool request",
        ));
    }

    // Never reclaim a pending key automatically. A timed-out caller may still
    // be executing a non-transactional external mutation, so retrying it could
    // create a duplicate operation or approval. The lease timestamp is kept as
    // operator diagnostics and for a future explicit recovery workflow.
    Ok(InboundMcpIdempotencyReservation::Existing(Box::new(
        existing,
    )))
}

pub(super) async fn complete(
    pool: &PgPool,
    input: &InboundMcpIdempotencyInput,
    approval_id: Option<&Id>,
    result: &Value,
    status: &str,
) -> Result<()> {
    if !matches!(status, "completed" | "failed") {
        return Err(Error::internal("invalid inbound MCP idempotency status"));
    }
    let now = TimestampMicros::now();
    let updated = sqlx::query(
        "UPDATE inbound_mcp_idempotency
         SET status=$7,approval_id=$8,result=$9,updated_at_micros=$10
         WHERE org_id=$1 AND principal_type=$2 AND principal_id=$3 AND idempotency_key=$4
           AND tool_name=$5 AND request_hash=$6",
    )
    .bind(&input.org_id.0)
    .bind(&input.principal_type)
    .bind(&input.principal_id.0)
    .bind(&input.idempotency_key)
    .bind(&input.tool_name)
    .bind(&input.request_hash)
    .bind(status)
    .bind(approval_id.map(|value| &value.0))
    .bind(Json(result))
    .bind(now.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    if updated.rows_affected() != 1 {
        return Err(Error::conflict(
            "idempotency reservation was lost before completion",
        ));
    }
    Ok(())
}
