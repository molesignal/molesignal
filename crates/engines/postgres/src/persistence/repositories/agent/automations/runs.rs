// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_automation_runs`：每次运行一行，结束时写回状态、原因、回答的开头与输出的结果。

use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::automations::{AutomationRun, AutomationRunStatus, RunEnding, TriggerKind},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

fn run_row(row: PgRow) -> Result<AutomationRun> {
    let trigger: String = row.try_get("trigger").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let id = |column: &str| -> Result<Option<Id>> {
        Ok(row
            .try_get::<Option<String>, _>(column)
            .map_err(sqlx_err)?
            .map(Id))
    };
    let outputs: Json<Value> = row.try_get("outputs").map_err(sqlx_err)?;

    Ok(AutomationRun {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        automation_id: Id(row.try_get("automation_id").map_err(sqlx_err)?),
        trigger: TriggerKind::parse(&trigger)
            .ok_or_else(|| Error::internal(format!("unknown automation trigger `{trigger}`")))?,
        status: AutomationRunStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown automation run status `{status}`")))?,
        reason: row.try_get("reason").map_err(sqlx_err)?,
        chat_id: id("chat_id")?,
        run_id: id("run_id")?,
        incident_id: id("incident_id")?,
        group_key: row.try_get("group_key").map_err(sqlx_err)?,
        input: row.try_get("input").map_err(sqlx_err)?,
        summary: row.try_get("summary").map_err(sqlx_err)?,
        outputs: outputs.0,
        started_by: id("started_by")?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        finished_at: row
            .try_get::<Option<TimestampMicros>, _>("finished_at")
            .map_err(sqlx_err)?,
    })
}

pub(super) async fn create(pool: &PgPool, run: AutomationRun) -> Result<AutomationRun> {
    sqlx::query(
        "INSERT INTO agent_automation_runs
         (id, org_id, automation_id, trigger, status, reason, chat_id, run_id, incident_id,
          group_key, input, summary, outputs, started_by, created_at, finished_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(&run.id.0)
    .bind(&run.org_id.0)
    .bind(&run.automation_id.0)
    .bind(run.trigger.as_str())
    .bind(run.status.as_str())
    .bind(run.reason.as_deref())
    .bind(run.chat_id.as_ref().map(|id| id.0.as_str()))
    .bind(run.run_id.as_ref().map(|id| id.0.as_str()))
    .bind(run.incident_id.as_ref().map(|id| id.0.as_str()))
    .bind(run.group_key.as_deref())
    .bind(run.input.as_deref())
    .bind(run.summary.as_deref())
    .bind(Json(&run.outputs))
    .bind(run.started_by.as_ref().map(|id| id.0.as_str()))
    .bind(run.created_at)
    .bind(run.finished_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(run)
}

pub(super) async fn attach(pool: &PgPool, id: &Id, chat_id: &Id, run_id: &Id) -> Result<()> {
    sqlx::query("UPDATE agent_automation_runs SET chat_id = $2, run_id = $3 WHERE id = $1")
        .bind(&id.0)
        .bind(&chat_id.0)
        .bind(&run_id.0)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}

/// 只结束还在运行的记录：两个节点同时收尾时只有一个写得进去。
pub(super) async fn finish(
    pool: &PgPool,
    id: &Id,
    ending: &RunEnding,
) -> Result<Option<AutomationRun>> {
    sqlx::query(
        "UPDATE agent_automation_runs
         SET status = $2, reason = $3, summary = $4, outputs = $5, finished_at = $6
         WHERE id = $1 AND status = 'running'
         RETURNING *",
    )
    .bind(&id.0)
    .bind(ending.status.as_str())
    .bind(ending.reason.as_deref())
    .bind(ending.summary.as_deref())
    .bind(Json(&ending.outputs))
    .bind(TimestampMicros::now())
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(run_row)
    .transpose()
}

pub(super) async fn get(pool: &PgPool, org_id: &Id, id: &Id) -> Result<AutomationRun> {
    sqlx::query("SELECT * FROM agent_automation_runs WHERE org_id = $1 AND id = $2")
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("automation run `{}` not found", id.0)))
        .and_then(run_row)
}

pub(super) async fn for_agent_run(pool: &PgPool, run_id: &Id) -> Result<Option<AutomationRun>> {
    sqlx::query("SELECT * FROM agent_automation_runs WHERE run_id = $1")
        .bind(&run_id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .map(run_row)
        .transpose()
}

pub(super) async fn list(
    pool: &PgPool,
    org_id: &Id,
    automation_id: &Id,
    limit: i64,
) -> Result<Vec<AutomationRun>> {
    sqlx::query(
        "SELECT * FROM agent_automation_runs
         WHERE org_id = $1 AND automation_id = $2
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(&org_id.0)
    .bind(&automation_id.0)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(run_row)
    .collect()
}

pub(super) async fn running(pool: &PgPool, limit: i64) -> Result<Vec<AutomationRun>> {
    sqlx::query(
        "SELECT * FROM agent_automation_runs WHERE status = 'running'
         ORDER BY created_at LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(run_row)
    .collect()
}

pub(super) async fn analyzed(
    pool: &PgPool,
    automation_id: &Id,
    incident_id: &Id,
    group_key: &str,
    since: TimestampMicros,
) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM agent_automation_runs
           WHERE automation_id = $1 AND trigger = 'incident' AND status <> 'skipped'
             AND (incident_id = $2 OR (group_key = $3 AND created_at >= $4))
         )",
    )
    .bind(&automation_id.0)
    .bind(&incident_id.0)
    .bind(group_key)
    .bind(since)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)
}

pub(super) async fn purge(pool: &PgPool, keep: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM agent_automation_runs WHERE id IN (
           SELECT id FROM (
             SELECT id, ROW_NUMBER() OVER (
               PARTITION BY automation_id ORDER BY created_at DESC
             ) AS position
             FROM agent_automation_runs WHERE status <> 'running'
           ) ranked WHERE position > $1
         )",
    )
    .bind(keep)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(result.rows_affected())
}
