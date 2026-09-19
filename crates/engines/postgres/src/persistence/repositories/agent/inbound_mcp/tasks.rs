// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::inbound_mcp::{InboundMcpTask, InboundMcpTaskStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "id,org_id,principal_type,principal_id,tool_name,request,status,status_message,\
    input_requests,input_responses,result,error,cancel_requested,expires_at_micros,\
    created_at_micros,updated_at_micros";

fn parse_status(value: &str) -> Result<InboundMcpTaskStatus> {
    match value {
        "working" => Ok(InboundMcpTaskStatus::Working),
        "input_required" => Ok(InboundMcpTaskStatus::InputRequired),
        "completed" => Ok(InboundMcpTaskStatus::Completed),
        "failed" => Ok(InboundMcpTaskStatus::Failed),
        "cancelled" => Ok(InboundMcpTaskStatus::Cancelled),
        other => Err(Error::internal(format!(
            "invalid inbound MCP task status `{other}`"
        ))),
    }
}

fn optional_json(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Value>> {
    Ok(row
        .try_get::<Option<Json<Value>>, _>(column)
        .map_err(sqlx_err)?
        .map(|value| value.0))
}

fn task_row(row: sqlx::postgres::PgRow) -> Result<InboundMcpTask> {
    let request: Json<Value> = row.try_get("request").map_err(sqlx_err)?;
    Ok(InboundMcpTask {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        principal_type: row.try_get("principal_type").map_err(sqlx_err)?,
        principal_id: Id(row.try_get("principal_id").map_err(sqlx_err)?),
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        request: request.0,
        status: parse_status(&row.try_get::<String, _>("status").map_err(sqlx_err)?)?,
        status_message: row.try_get("status_message").map_err(sqlx_err)?,
        input_requests: optional_json(&row, "input_requests")?,
        input_responses: optional_json(&row, "input_responses")?,
        result: optional_json(&row, "result")?,
        error: optional_json(&row, "error")?,
        cancel_requested: row.try_get("cancel_requested").map_err(sqlx_err)?,
        expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn create(pool: &PgPool, task: InboundMcpTask) -> Result<InboundMcpTask> {
    let row = sqlx::query(&format!(
        "INSERT INTO inbound_mcp_tasks ({COLS})
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
         RETURNING {COLS}"
    ))
    .bind(task.id.0)
    .bind(task.org_id.0)
    .bind(task.principal_type)
    .bind(task.principal_id.0)
    .bind(task.tool_name)
    .bind(Json(task.request))
    .bind(task.status.as_str())
    .bind(task.status_message)
    .bind(task.input_requests.map(Json))
    .bind(task.input_responses.map(Json))
    .bind(task.result.map(Json))
    .bind(task.error.map(Json))
    .bind(task.cancel_requested)
    .bind(task.expires_at.0)
    .bind(task.created_at.0)
    .bind(task.updated_at.0)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;
    task_row(row)
}

pub(super) async fn get(
    pool: &PgPool,
    org_id: &Id,
    principal_type: &str,
    principal_id: &Id,
    task_id: &Id,
) -> Result<InboundMcpTask> {
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM inbound_mcp_tasks
         WHERE id=$1 AND org_id=$2 AND principal_type=$3 AND principal_id=$4"
    ))
    .bind(&task_id.0)
    .bind(&org_id.0)
    .bind(principal_type)
    .bind(&principal_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| Error::not_found("inbound MCP task"))?;
    task_row(row)
}

pub(super) async fn update(pool: &PgPool, task: InboundMcpTask) -> Result<InboundMcpTask> {
    let org_id = task.org_id.clone();
    let principal_type = task.principal_type.clone();
    let principal_id = task.principal_id.clone();
    let task_id = task.id.clone();
    let row = sqlx::query(&format!(
        "UPDATE inbound_mcp_tasks SET status=$6,status_message=$7,input_requests=$8,
            input_responses=$9,result=$10,error=$11,cancel_requested=$12,
            expires_at_micros=$13,updated_at_micros=$14
         WHERE id=$1 AND org_id=$2 AND principal_type=$3 AND principal_id=$4 AND tool_name=$5
           AND CASE
             WHEN $6 IN ('completed','failed') THEN status='working'
             WHEN $6='cancelled' THEN status IN ('working','input_required')
             WHEN $6='input_required' THEN status IN ('working','input_required')
             WHEN $6='working' THEN status='working'
             ELSE FALSE
           END
         RETURNING {COLS}"
    ))
    .bind(&task.id.0)
    .bind(&task.org_id.0)
    .bind(&task.principal_type)
    .bind(&task.principal_id.0)
    .bind(&task.tool_name)
    .bind(task.status.as_str())
    .bind(task.status_message)
    .bind(task.input_requests.map(Json))
    .bind(task.input_responses.map(Json))
    .bind(task.result.map(Json))
    .bind(task.error.map(Json))
    .bind(task.cancel_requested)
    .bind(task.expires_at.0)
    .bind(task.updated_at.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;
    match row {
        Some(row) => task_row(row),
        None => get(pool, &org_id, &principal_type, &principal_id, &task_id).await,
    }
}

pub(super) async fn request_cancellation(
    pool: &PgPool,
    org_id: &Id,
    principal_type: &str,
    principal_id: &Id,
    task_id: &Id,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE inbound_mcp_tasks
         SET cancel_requested=TRUE,status='cancelled',
             status_message='Task cancellation requested',updated_at_micros=$5
         WHERE id=$1 AND org_id=$2 AND principal_type=$3 AND principal_id=$4
           AND status IN ('working','input_required')",
    )
    .bind(&task_id.0)
    .bind(&org_id.0)
    .bind(principal_type)
    .bind(&principal_id.0)
    .bind(TimestampMicros::now().0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    if updated.rows_affected() == 0 {
        let _ = get(pool, org_id, principal_type, principal_id, task_id).await?;
    }
    Ok(())
}
