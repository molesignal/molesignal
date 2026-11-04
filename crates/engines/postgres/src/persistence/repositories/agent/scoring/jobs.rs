// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 排队的打分：排进去、领走、结束、过一会儿再试。

use sqlx::{PgPool, Row, postgres::PgRow};

use super::{is_unique_violation, micros, optional_text, sqlx_err, text};
use crate::{
    agent::scoring::{JobReason, JobStatus, NewJob, RULES_SCORER, ScoreJob, ScoreTarget},
    shared::{Result, ids::Id, time::TimestampMicros},
};

fn job_row(row: PgRow) -> Result<ScoreJob> {
    Ok(ScoreJob {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        target_kind: ScoreTarget::parse(&text(&row, "target_kind")?)?,
        target_id: text(&row, "target_id")?,
        chat_id: optional_text(&row, "chat_id")?.map(Id),
        scorer: text(&row, "scorer")?,
        reason: JobReason::parse(&text(&row, "reason")?)?,
        status: JobStatus::parse(&text(&row, "status")?)?,
        attempts: row.try_get("attempts").map_err(sqlx_err)?,
        created_at: micros(&row, "created_at")?,
    })
}

pub(super) async fn enqueue(pool: &PgPool, jobs: &[NewJob]) -> Result<u64> {
    let now = TimestampMicros::now().0;
    let mut added = 0;

    for job in jobs {
        let result = sqlx::query(
            "INSERT INTO agent_score_jobs
             (id, org_id, target_kind, target_id, chat_id, scorer, reason, status, attempts,
              not_before, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued', 0, $8, $8)
             ON CONFLICT (org_id, target_kind, target_id, scorer) WHERE status = 'queued'
             DO NOTHING",
        )
        .bind(Id::new().0)
        .bind(&job.org_id.0)
        .bind(job.target_kind.as_str())
        .bind(&job.target_id)
        .bind(job.chat_id.as_ref().map(|id| id.0.as_str()))
        .bind(&job.scorer)
        .bind(job.reason.as_str())
        .bind(TimestampMicros(now))
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

        added += result.rows_affected();
    }

    Ok(added)
}

/// 领走到点的打分，以及领走后节点没了、租约过期的。
pub(super) async fn claim(
    pool: &PgPool,
    now: TimestampMicros,
    lease_until: TimestampMicros,
    limit: i64,
) -> Result<Vec<ScoreJob>> {
    sqlx::query(
        "UPDATE agent_score_jobs j
         SET status = 'running', attempts = j.attempts + 1, lease_expires_at = $2
         FROM (
           SELECT id FROM agent_score_jobs
           WHERE (status = 'queued' AND not_before <= $1)
              OR (status = 'running' AND lease_expires_at < $1)
           ORDER BY not_before
           LIMIT $3
           FOR UPDATE SKIP LOCKED
         ) due
         WHERE j.id = due.id
         RETURNING j.*",
    )
    .bind(now)
    .bind(lease_until)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(job_row)
    .collect()
}

pub(super) async fn finish(
    pool: &PgPool,
    id: &Id,
    status: JobStatus,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_score_jobs
         SET status = $2, error = $3, finished_at = $4, lease_expires_at = NULL
         WHERE id = $1",
    )
    .bind(&id.0)
    .bind(status.as_str())
    .bind(error)
    .bind(TimestampMicros::now())
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

/// 过一会儿再试。同一个对象、同一个打分器已经又排了一次时，这一次不再排。
pub(super) async fn retry(
    pool: &PgPool,
    id: &Id,
    not_before: TimestampMicros,
    error: &str,
) -> Result<()> {
    let requeued = sqlx::query(
        "UPDATE agent_score_jobs
         SET status = 'queued', not_before = $2, error = $3, lease_expires_at = NULL
         WHERE id = $1",
    )
    .bind(&id.0)
    .bind(not_before)
    .bind(error)
    .execute(pool)
    .await;

    match requeued {
        Ok(_) => Ok(()),
        Err(error) if is_unique_violation(&error) => {
            finish(pool, id, JobStatus::Skipped, Some("queued again")).await
        }
        Err(error) => Err(sqlx_err(error)),
    }
}

pub(super) async fn evaluator_planned(
    pool: &PgPool,
    org_id: &Id,
    kind: ScoreTarget,
    target_id: &str,
) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM agent_score_jobs
           WHERE org_id = $1 AND target_kind = $2 AND target_id = $3 AND scorer <> $4
             AND status IN ('queued', 'running', 'done')
         ) OR EXISTS (
           SELECT 1 FROM agent_scores
           WHERE org_id = $1 AND target_kind = $2 AND target_id = $3 AND scorer <> $4
         )",
    )
    .bind(&org_id.0)
    .bind(kind.as_str())
    .bind(target_id)
    .bind(RULES_SCORER)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)
}

pub(super) async fn purge(pool: &PgPool, before: TimestampMicros) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM agent_score_jobs
         WHERE status IN ('done', 'skipped', 'failed') AND finished_at < $1",
    )
    .bind(before)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(result.rows_affected())
}
