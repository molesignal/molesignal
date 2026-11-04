// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 待审：一个对象一条；采纳过的是回归集。

use sqlx::{PgPool, Row, postgres::PgRow};

use super::{
    micros, optional_micros, optional_text,
    scores::{SUBJECT_COLUMNS, subject_row},
    sources, sqlx_err, text,
};
use crate::{
    agent::scoring::{
        Review, ReviewCause, ReviewDecision, ReviewFilter, ReviewItem, ReviewStatus, ScoreTarget,
        TargetSource,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 待审的列：和对象联查时与对象的同名列区分开。
const REVIEW_COLUMNS: &str = "r.id AS review_id, r.org_id AS review_org_id, \
    r.target_kind AS review_target_kind, r.target_id AS review_target_id, \
    r.source AS review_source, r.cause, r.score, r.status, r.note, r.decided_by, \
    r.decided_at, r.created_at AS review_created_at, \
    r.updated_at AS review_updated_at";

fn review_row(row: &PgRow) -> Result<Review> {
    Ok(Review {
        id: Id(text(row, "review_id")?),
        org_id: Id(text(row, "review_org_id")?),
        target_kind: ScoreTarget::parse(&text(row, "review_target_kind")?)?,
        target_id: text(row, "review_target_id")?,
        source: TargetSource::parse(&text(row, "review_source")?)?,
        cause: ReviewCause::parse(&text(row, "cause")?)?,
        score: row.try_get("score").map_err(sqlx_err)?,
        status: ReviewStatus::parse(&text(row, "status")?)?,
        note: text(row, "note")?,
        decided_by: optional_text(row, "decided_by")?.map(Id),
        decided_at: optional_micros(row, "decided_at")?,
        created_at: micros(row, "review_created_at")?,
        updated_at: micros(row, "review_updated_at")?,
    })
}

/// 待审连同对象：对象不在了时只有待审的列。
fn item_row(row: PgRow) -> Result<ReviewItem> {
    let subject = match optional_text(&row, "target_id")? {
        Some(_) => Some(subject_row(&row)?),
        None => None,
    };

    Ok(ReviewItem {
        review: review_row(&row)?,
        subject,
    })
}

/// 联查对象的 FROM。
const FROM: &str = "agent_score_reviews r LEFT JOIN agent_score_subjects t \
    ON t.org_id = r.org_id AND t.target_kind = r.target_kind AND t.target_id = r.target_id";

fn not_found(id: &Id) -> Error {
    Error::not_found(format!("review `{}` not found", id.0))
}

pub(super) async fn open(pool: &PgPool, review: &Review) -> Result<bool> {
    let result = sqlx::query(
        "INSERT INTO agent_score_reviews
         (id, org_id, target_kind, target_id, source, cause, score, status, note,
          created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', '', $8, $8)
         ON CONFLICT (org_id, target_kind, target_id) DO NOTHING",
    )
    .bind(&review.id.0)
    .bind(&review.org_id.0)
    .bind(review.target_kind.as_str())
    .bind(&review.target_id)
    .bind(review.source.as_str())
    .bind(review.cause.as_str())
    .bind(review.score)
    .bind(review.created_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(result.rows_affected() > 0)
}

pub(super) async fn get(pool: &PgPool, org_id: &Id, id: &Id) -> Result<Review> {
    sqlx::query(&format!(
        "SELECT {REVIEW_COLUMNS} FROM agent_score_reviews r WHERE r.org_id = $1 AND r.id = $2"
    ))
    .bind(&org_id.0)
    .bind(&id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(|row| review_row(&row))
    .transpose()?
    .ok_or_else(|| not_found(id))
}

pub(super) async fn of_target(
    pool: &PgPool,
    org_id: &Id,
    kind: ScoreTarget,
    target_id: &str,
) -> Result<Option<Review>> {
    sqlx::query(&format!(
        "SELECT {REVIEW_COLUMNS} FROM agent_score_reviews r
         WHERE r.org_id = $1 AND r.target_kind = $2 AND r.target_id = $3"
    ))
    .bind(&org_id.0)
    .bind(kind.as_str())
    .bind(target_id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(|row| review_row(&row))
    .transpose()
}

pub(super) async fn list(
    pool: &PgPool,
    org_id: &Id,
    filter: &ReviewFilter,
) -> Result<Vec<ReviewItem>> {
    let (before_at, before_id) = match &filter.before {
        Some((at, id)) => (Some(at.0), Some(id.0.as_str())),
        None => (None, None),
    };

    sqlx::query(&format!(
        "SELECT {REVIEW_COLUMNS}, {SUBJECT_COLUMNS} FROM {FROM}
         WHERE r.org_id = $1 AND ($2::TEXT IS NULL OR r.status = $2)
           AND ($3::TIMESTAMPTZ IS NULL OR (r.created_at, r.id) < ($3, $4::TEXT))
         ORDER BY r.created_at DESC, r.id DESC
         LIMIT $5"
    ))
    .bind(&org_id.0)
    .bind(filter.status.map(ReviewStatus::as_str))
    .bind(before_at.map(TimestampMicros))
    .bind(before_id)
    .bind(filter.limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(item_row)
    .collect()
}

pub(super) async fn decide(
    pool: &PgPool,
    org_id: &Id,
    id: &Id,
    decision: &ReviewDecision,
) -> Result<Review> {
    let result = sqlx::query(
        "UPDATE agent_score_reviews
         SET status = $3, note = $4, decided_by = $5, decided_at = $6,
             updated_at = $6
         WHERE org_id = $1 AND id = $2",
    )
    .bind(&org_id.0)
    .bind(&id.0)
    .bind(decision.status.as_str())
    .bind(&decision.note)
    .bind(&decision.by.0)
    .bind(decision.at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    if result.rows_affected() == 0 {
        return Err(not_found(id));
    }

    get(pool, org_id, id).await
}

pub(super) async fn regression_set(
    pool: &PgPool,
    org_id: &Id,
    wanted: &[TargetSource],
    limit: i64,
) -> Result<Vec<Review>> {
    sqlx::query(&format!(
        "SELECT {REVIEW_COLUMNS} FROM agent_score_reviews r
         WHERE r.org_id = $1 AND r.status = 'accepted' AND r.source = ANY($2)
         ORDER BY r.decided_at DESC NULLS LAST, r.id DESC
         LIMIT $3"
    ))
    .bind(&org_id.0)
    .bind(sources(wanted))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .iter()
    .map(review_row)
    .collect()
}
