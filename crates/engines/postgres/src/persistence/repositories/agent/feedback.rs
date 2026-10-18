// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_feedback`：对 Mole Agent 产出的评价，每人对每个对象一行。

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    agent::feedback::{
        AgentFeedback, AgentFeedbackRepository, FeedbackRating, FeedbackReason, FeedbackTarget,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "id, org_id, target_kind, target_id, user_id, rating, reasons, comment, \
    provenance, created_at_micros, updated_at_micros";

pub struct PgAgentFeedbackRepository {
    pool: PgPool,
}

impl PgAgentFeedbackRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn feedback_row(row: sqlx::postgres::PgRow) -> Result<AgentFeedback> {
    let target_kind: String = row.try_get("target_kind").map_err(sqlx_err)?;
    let rating: String = row.try_get("rating").map_err(sqlx_err)?;
    let reasons: Json<Vec<String>> = row.try_get("reasons").map_err(sqlx_err)?;
    let provenance: Json<Value> = row.try_get("provenance").map_err(sqlx_err)?;

    Ok(AgentFeedback {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        target_kind: FeedbackTarget::parse(&target_kind)?,
        target_id: row.try_get("target_id").map_err(sqlx_err)?,
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        rating: FeedbackRating::parse(&rating)?,
        reasons: reasons
            .0
            .iter()
            .map(|reason| FeedbackReason::parse(reason))
            .collect::<Result<_>>()?,
        comment: row.try_get("comment").map_err(sqlx_err)?,
        provenance: provenance.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl AgentFeedbackRepository for PgAgentFeedbackRepository {
    async fn upsert(&self, feedback: AgentFeedback) -> Result<AgentFeedback> {
        let reasons: Vec<&str> = feedback
            .reasons
            .iter()
            .map(|reason| reason.as_str())
            .collect();

        let row = sqlx::query(&format!(
            "INSERT INTO agent_feedback ({COLS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (org_id, target_kind, target_id, user_id) DO UPDATE SET
                rating = EXCLUDED.rating,
                reasons = EXCLUDED.reasons,
                comment = EXCLUDED.comment,
                provenance = EXCLUDED.provenance,
                updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING {COLS}"
        ))
        .bind(&feedback.id.0)
        .bind(&feedback.org_id.0)
        .bind(feedback.target_kind.as_str())
        .bind(&feedback.target_id)
        .bind(&feedback.user_id.0)
        .bind(feedback.rating.as_str())
        .bind(Json(&reasons))
        .bind(&feedback.comment)
        .bind(Json(&feedback.provenance))
        .bind(feedback.created_at.0)
        .bind(feedback.updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        feedback_row(row)
    }

    async fn remove(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_id: &str,
        user_id: &Id,
    ) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM agent_feedback
             WHERE org_id = $1 AND target_kind = $2 AND target_id = $3 AND user_id = $4",
        )
        .bind(&org_id.0)
        .bind(target_kind.as_str())
        .bind(target_id)
        .bind(&user_id.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_for_user(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_ids: &[String],
        user_id: &Id,
    ) -> Result<Vec<AgentFeedback>> {
        if target_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM agent_feedback
             WHERE org_id = $1 AND target_kind = $2 AND user_id = $3
               AND target_id = ANY($4)"
        ))
        .bind(&org_id.0)
        .bind(target_kind.as_str())
        .bind(&user_id.0)
        .bind(target_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(feedback_row).collect()
    }
}
