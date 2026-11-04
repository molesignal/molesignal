// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 评审与组织的打分设置。

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{
    super::super::org_settings::{self, sections::AGENT_SCORING},
    is_unique_violation, json, micros, optional_text, sqlx_err, text,
};
use crate::{
    agent::scoring::{Evaluator, GateMode, MAX_EVALUATORS_PER_ORG, ScoringSettings},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

fn evaluator_row(row: PgRow) -> Result<Evaluator> {
    Ok(Evaluator {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        name: text(&row, "name")?,
        rubric: text(&row, "rubric")?,
        targets: json(&row, "targets")?,
        sample_percent: u8::try_from(row.try_get::<i16, _>("sample_percent").map_err(sqlx_err)?)
            .unwrap_or_default(),
        provider_id: optional_text(&row, "provider_id")?.map(Id),
        model: optional_text(&row, "model")?,
        temperature: row.try_get("temperature").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        language: text(&row, "language")?,
        created_by: Id(text(&row, "created_by")?),
        updated_by: Id(text(&row, "updated_by")?),
        created_at: micros(&row, "created_at")?,
        updated_at: micros(&row, "updated_at")?,
    })
}

fn not_found(id: &Id) -> Error {
    Error::not_found(format!("evaluator `{}` not found", id.0))
}

fn name_conflict(error: sqlx::Error, name: &str) -> Error {
    if is_unique_violation(&error) {
        Error::conflict(format!("an evaluator named `{name}` already exists"))
    } else {
        sqlx_err(error)
    }
}

pub(super) async fn list_evaluators(pool: &PgPool, org_id: &Id) -> Result<Vec<Evaluator>> {
    sqlx::query("SELECT * FROM agent_evaluators WHERE org_id = $1 ORDER BY lower(name)")
        .bind(&org_id.0)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(evaluator_row)
        .collect()
}

pub(super) async fn get_evaluator(pool: &PgPool, org_id: &Id, id: &Id) -> Result<Evaluator> {
    sqlx::query("SELECT * FROM agent_evaluators WHERE org_id = $1 AND id = $2")
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .map(evaluator_row)
        .transpose()?
        .ok_or_else(|| not_found(id))
}

pub(super) async fn create_evaluator(pool: &PgPool, evaluator: Evaluator) -> Result<Evaluator> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_evaluators WHERE org_id = $1")
        .bind(&evaluator.org_id.0)
        .fetch_one(pool)
        .await
        .map_err(sqlx_err)?;

    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_EVALUATORS_PER_ORG {
        return Err(Error::invalid(format!(
            "an organization can have at most {MAX_EVALUATORS_PER_ORG} evaluators"
        )));
    }

    sqlx::query(
        "INSERT INTO agent_evaluators
         (id, org_id, name, rubric, targets, sample_percent, provider_id, model, temperature,
          enabled, language, created_by, updated_by, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&evaluator.id.0)
    .bind(&evaluator.org_id.0)
    .bind(&evaluator.name)
    .bind(&evaluator.rubric)
    .bind(Json(&evaluator.targets))
    .bind(i16::from(evaluator.sample_percent))
    .bind(evaluator.provider_id.as_ref().map(|id| id.0.as_str()))
    .bind(evaluator.model.as_deref())
    .bind(evaluator.temperature)
    .bind(evaluator.enabled)
    .bind(&evaluator.language)
    .bind(&evaluator.created_by.0)
    .bind(&evaluator.updated_by.0)
    .bind(evaluator.created_at)
    .bind(evaluator.updated_at)
    .execute(pool)
    .await
    .map_err(|error| name_conflict(error, &evaluator.name))?;

    get_evaluator(pool, &evaluator.org_id, &evaluator.id).await
}

pub(super) async fn update_evaluator(pool: &PgPool, evaluator: Evaluator) -> Result<Evaluator> {
    let result = sqlx::query(
        "UPDATE agent_evaluators SET
           name = $3, rubric = $4, targets = $5, sample_percent = $6, provider_id = $7,
           model = $8, temperature = $9, enabled = $10, language = $11, updated_by = $12,
           updated_at = $13
         WHERE org_id = $1 AND id = $2",
    )
    .bind(&evaluator.org_id.0)
    .bind(&evaluator.id.0)
    .bind(&evaluator.name)
    .bind(&evaluator.rubric)
    .bind(Json(&evaluator.targets))
    .bind(i16::from(evaluator.sample_percent))
    .bind(evaluator.provider_id.as_ref().map(|id| id.0.as_str()))
    .bind(evaluator.model.as_deref())
    .bind(evaluator.temperature)
    .bind(evaluator.enabled)
    .bind(&evaluator.language)
    .bind(&evaluator.updated_by.0)
    .bind(evaluator.updated_at)
    .execute(pool)
    .await
    .map_err(|error| name_conflict(error, &evaluator.name))?;

    if result.rows_affected() == 0 {
        return Err(not_found(&evaluator.id));
    }

    get_evaluator(pool, &evaluator.org_id, &evaluator.id).await
}

/// 删除评审；它给过的分数留着，排着的打分作废。
pub(super) async fn delete_evaluator(pool: &PgPool, org_id: &Id, id: &Id) -> Result<()> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    let deleted = sqlx::query("DELETE FROM agent_evaluators WHERE org_id = $1 AND id = $2")
        .bind(&org_id.0)
        .bind(&id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    if deleted.rows_affected() == 0 {
        return Err(not_found(id));
    }

    sqlx::query(
        "UPDATE agent_score_jobs
         SET status = 'skipped', error = 'the evaluator was deleted', finished_at = $3
         WHERE org_id = $1 AND scorer = $2 AND status = 'queued'",
    )
    .bind(&org_id.0)
    .bind(&id.0)
    .bind(TimestampMicros::now())
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tx.commit().await.map_err(sqlx_err)
}

/// `agent.scoring` 一节的值。取值范围由 [`ScoringSettings::validated`] 把关。
#[derive(Serialize, Deserialize)]
struct StoredScoringSettings {
    review_threshold: f64,
    review_sample_percent: u8,
    gate: String,
}

pub(super) async fn settings(pool: &PgPool, org_id: &Id) -> Result<ScoringSettings> {
    let section =
        org_settings::read::<StoredScoringSettings>(pool, &org_id.0, AGENT_SCORING).await?;

    let Some(section) = section else {
        return Ok(ScoringSettings::default());
    };

    Ok(ScoringSettings {
        review_threshold: section.value.review_threshold,
        review_sample_percent: section.value.review_sample_percent,
        gate: GateMode::parse(&section.value.gate)?,
        updated_by: section.updated_by.map(Id),
        updated_at: Some(section.updated_at),
    })
}

pub(super) async fn put_settings(
    pool: &PgPool,
    org_id: &Id,
    settings: &ScoringSettings,
) -> Result<ScoringSettings> {
    let stored = StoredScoringSettings {
        review_threshold: settings.review_threshold,
        review_sample_percent: settings.review_sample_percent,
        gate: settings.gate.as_str().to_owned(),
    };

    org_settings::write(
        pool,
        &org_id.0,
        AGENT_SCORING,
        &stored,
        settings.updated_by.as_ref().map(|id| id.0.as_str()),
        settings.updated_at.unwrap_or_else(TimestampMicros::now),
    )
    .await?;

    self::settings(pool, org_id).await
}
