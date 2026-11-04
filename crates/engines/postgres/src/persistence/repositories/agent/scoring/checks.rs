// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prompt 晋级检查：排着的由一个节点领走，做的时候按样本记下进度并续上租约。

use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{json, micros, optional_micros, optional_text, sqlx_err, text};
use crate::{
    agent::scoring::check::{CheckStatus, CheckVerdict, PromptCheck},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

fn check_row(row: PgRow) -> Result<PromptCheck> {
    Ok(PromptCheck {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        prompt_id: Id(text(&row, "prompt_id")?),
        prompt_version: row.try_get("prompt_version").map_err(sqlx_err)?,
        purpose: text(&row, "purpose")?,
        baseline_prompt_id: Id(text(&row, "baseline_prompt_id")?),
        baseline_version: row.try_get("baseline_version").map_err(sqlx_err)?,
        evaluator_id: Id(text(&row, "evaluator_id")?),
        status: CheckStatus::parse(&text(&row, "status")?)?,
        samples: json(&row, "samples")?,
        baseline_mean: row.try_get("baseline_mean").map_err(sqlx_err)?,
        candidate_mean: row.try_get("candidate_mean").map_err(sqlx_err)?,
        verdict: optional_text(&row, "verdict")?
            .map(|verdict| CheckVerdict::parse(&verdict))
            .transpose()?,
        error: optional_text(&row, "error")?,
        tokens: row.try_get("tokens").map_err(sqlx_err)?,
        requested_by: Id(text(&row, "requested_by")?),
        created_at: micros(&row, "created_at")?,
        finished_at: optional_micros(&row, "finished_at")?,
    })
}

pub(super) async fn create(pool: &PgPool, check: &PromptCheck) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_prompt_checks
         (id, org_id, prompt_id, prompt_version, purpose, baseline_prompt_id, baseline_version,
          evaluator_id, status, samples, requested_by, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(&check.id.0)
    .bind(&check.org_id.0)
    .bind(&check.prompt_id.0)
    .bind(check.prompt_version)
    .bind(&check.purpose)
    .bind(&check.baseline_prompt_id.0)
    .bind(check.baseline_version)
    .bind(&check.evaluator_id.0)
    .bind(check.status.as_str())
    .bind(Json(&check.samples))
    .bind(&check.requested_by.0)
    .bind(check.created_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn get(pool: &PgPool, org_id: &Id, id: &Id) -> Result<PromptCheck> {
    sqlx::query("SELECT * FROM agent_prompt_checks WHERE org_id = $1 AND id = $2")
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .map(check_row)
        .transpose()?
        .ok_or_else(|| Error::not_found(format!("prompt check `{}` not found", id.0)))
}

pub(super) async fn latest(
    pool: &PgPool,
    org_id: &Id,
    prompt_id: &Id,
) -> Result<Option<PromptCheck>> {
    sqlx::query(
        "SELECT * FROM agent_prompt_checks
         WHERE org_id = $1 AND prompt_id = $2
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&org_id.0)
    .bind(&prompt_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(check_row)
    .transpose()
}

pub(super) async fn claim(
    pool: &PgPool,
    now: TimestampMicros,
    lease_until: TimestampMicros,
) -> Result<Option<PromptCheck>> {
    sqlx::query(
        "UPDATE agent_prompt_checks c
         SET status = 'running', lease_expires_at = $2
         FROM (
           SELECT id FROM agent_prompt_checks
           WHERE status = 'queued'
              OR (status = 'running' AND lease_expires_at < $1)
           ORDER BY created_at
           LIMIT 1
           FOR UPDATE SKIP LOCKED
         ) due
         WHERE c.id = due.id
         RETURNING c.*",
    )
    .bind(now)
    .bind(lease_until)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(check_row)
    .transpose()
}

pub(super) async fn save(
    pool: &PgPool,
    check: &PromptCheck,
    lease_until: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_prompt_checks
         SET status = $2, samples = $3, baseline_mean = $4, candidate_mean = $5, verdict = $6,
             error = $7, tokens = $8, finished_at = $9, lease_expires_at = $10
         WHERE id = $1",
    )
    .bind(&check.id.0)
    .bind(check.status.as_str())
    .bind(Json(&check.samples))
    .bind(check.baseline_mean)
    .bind(check.candidate_mean)
    .bind(check.verdict.map(CheckVerdict::as_str))
    .bind(check.error.as_deref())
    .bind(check.tokens)
    .bind(check.finished_at)
    .bind(lease_until)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}
