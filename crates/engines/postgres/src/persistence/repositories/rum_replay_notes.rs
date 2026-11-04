// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapters for what is kept about a recorded session besides its events: the comments
//! people leave on its replay and the summaries a model writes of it.

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::rum::{
        MAX_COMMENTS_PER_SESSION, RumReplayComment, RumReplayCommentRepository, RumSessionSummary,
        RumSessionSummaryRepository, RumSummaryChapter,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgRumReplayCommentRepository {
    pool: PgPool,
}

impl PgRumReplayCommentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const SELECT_COMMENT: &str = "SELECT c.org_id, c.id, c.application_id, c.session_id, c.at, c.body,
        c.author_id, u.display_name AS author_name, c.created_at, c.updated_at
   FROM rum_replay_comments c
   LEFT JOIN users u ON u.id = c.author_id";

fn comment_of(row: sqlx::postgres::PgRow) -> RumReplayComment {
    RumReplayComment {
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        id: row.try_get::<String, _>("id").unwrap_or_default(),
        application_id: row
            .try_get::<String, _>("application_id")
            .unwrap_or_default(),
        session_id: row.try_get::<String, _>("session_id").unwrap_or_default(),
        at_time: row
            .try_get::<TimestampMicros, _>("at")
            .unwrap_or_default()
            .0,
        body: row.try_get::<String, _>("body").unwrap_or_default(),
        author_id: row
            .try_get::<Option<String>, _>("author_id")
            .unwrap_or_default()
            .map(Id),
        author_name: row
            .try_get::<Option<String>, _>("author_name")
            .unwrap_or_default(),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .unwrap_or_default(),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .unwrap_or_default(),
    }
}

fn not_found(id: &str) -> Error {
    Error::not_found(format!("RUM replay comment `{id}` does not exist"))
}

#[async_trait]
impl RumReplayCommentRepository for PgRumReplayCommentRepository {
    async fn create(&self, comment: RumReplayComment) -> Result<RumReplayComment> {
        let held: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rum_replay_comments WHERE org_id = $1 AND session_id = $2",
        )
        .bind(&comment.org_id.0)
        .bind(&comment.session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if held >= MAX_COMMENTS_PER_SESSION as i64 {
            return Err(Error::invalid(format!(
                "a session holds at most {MAX_COMMENTS_PER_SESSION} comments"
            )));
        }

        sqlx::query(
            "INSERT INTO rum_replay_comments
                (org_id, id, application_id, session_id, at, body, author_id,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&comment.org_id.0)
        .bind(&comment.id)
        .bind(&comment.application_id)
        .bind(&comment.session_id)
        .bind(TimestampMicros(comment.at_time))
        .bind(&comment.body)
        .bind(comment.author_id.as_ref().map(|id| id.0.as_str()))
        .bind(comment.created_at)
        .bind(comment.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.get(&comment.org_id, &comment.session_id, &comment.id)
            .await
    }

    async fn list(&self, org_id: &Id, session_id: &str) -> Result<Vec<RumReplayComment>> {
        let rows = sqlx::query(&format!(
            "{SELECT_COMMENT} WHERE c.org_id = $1 AND c.session_id = $2
             ORDER BY c.at, c.created_at, c.id"
        ))
        .bind(&org_id.0)
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(comment_of).collect())
    }

    async fn get(&self, org_id: &Id, session_id: &str, id: &str) -> Result<RumReplayComment> {
        sqlx::query(&format!(
            "{SELECT_COMMENT} WHERE c.org_id = $1 AND c.session_id = $2 AND c.id = $3"
        ))
        .bind(&org_id.0)
        .bind(session_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(comment_of)
        .ok_or_else(|| not_found(id))
    }

    async fn update_body(
        &self,
        org_id: &Id,
        session_id: &str,
        id: &str,
        body: &str,
        now: TimestampMicros,
    ) -> Result<RumReplayComment> {
        let changed = sqlx::query(
            "UPDATE rum_replay_comments SET body = $4, updated_at = $5
              WHERE org_id = $1 AND session_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(session_id)
        .bind(id)
        .bind(body)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if changed.rows_affected() == 0 {
            return Err(not_found(id));
        }

        self.get(org_id, session_id, id).await
    }

    async fn delete(&self, org_id: &Id, session_id: &str, id: &str) -> Result<()> {
        let removed = sqlx::query(
            "DELETE FROM rum_replay_comments WHERE org_id = $1 AND session_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(session_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if removed.rows_affected() == 0 {
            return Err(not_found(id));
        }

        Ok(())
    }

    async fn delete_older_than(&self, cutoff: TimestampMicros, limit: usize) -> Result<usize> {
        let removed = sqlx::query(
            "DELETE FROM rum_replay_comments
              WHERE (org_id, id) IN (
                  SELECT org_id, id FROM rum_replay_comments
                   WHERE at < $1
                   LIMIT $2)",
        )
        .bind(cutoff)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(usize::try_from(removed.rows_affected()).unwrap_or(usize::MAX))
    }
}

pub struct PgRumSessionSummaryRepository {
    pool: PgPool,
}

impl PgRumSessionSummaryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn summary_of(row: sqlx::postgres::PgRow) -> RumSessionSummary {
    RumSessionSummary {
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        application_id: row
            .try_get::<String, _>("application_id")
            .unwrap_or_default(),
        session_id: row.try_get::<String, _>("session_id").unwrap_or_default(),
        language: row.try_get::<String, _>("language").unwrap_or_default(),
        summary: row.try_get::<String, _>("summary").unwrap_or_default(),
        chapters: row
            .try_get::<Json<Vec<RumSummaryChapter>>, _>("chapters")
            .map(|chapters| chapters.0)
            .unwrap_or_default(),
        model: row.try_get::<String, _>("model").unwrap_or_default(),
        prompt_tokens: row.try_get::<i64, _>("prompt_tokens").unwrap_or_default(),
        completion_tokens: row
            .try_get::<i64, _>("completion_tokens")
            .unwrap_or_default(),
        input_digest: row.try_get::<String, _>("input_digest").unwrap_or_default(),
        generated_by: row
            .try_get::<Option<String>, _>("generated_by")
            .unwrap_or_default()
            .map(Id),
        generated_at: row
            .try_get::<TimestampMicros, _>("generated_at")
            .unwrap_or_default(),
    }
}

#[async_trait]
impl RumSessionSummaryRepository for PgRumSessionSummaryRepository {
    async fn get(
        &self,
        org_id: &Id,
        session_id: &str,
        language: &str,
    ) -> Result<Option<RumSessionSummary>> {
        let found = sqlx::query(
            "SELECT org_id, application_id, session_id, language, summary, chapters, model,
                    prompt_tokens, completion_tokens, input_digest, generated_by,
                    generated_at
               FROM rum_session_summaries
              WHERE org_id = $1 AND session_id = $2 AND language = $3",
        )
        .bind(&org_id.0)
        .bind(session_id)
        .bind(language)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(found.map(summary_of))
    }

    async fn latest(&self, org_id: &Id, session_id: &str) -> Result<Option<RumSessionSummary>> {
        let found = sqlx::query(
            "SELECT org_id, application_id, session_id, language, summary, chapters, model,
                    prompt_tokens, completion_tokens, input_digest, generated_by,
                    generated_at
               FROM rum_session_summaries
              WHERE org_id = $1 AND session_id = $2
              ORDER BY generated_at DESC
              LIMIT 1",
        )
        .bind(&org_id.0)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(found.map(summary_of))
    }

    async fn put(&self, summary: &RumSessionSummary) -> Result<()> {
        sqlx::query(
            "INSERT INTO rum_session_summaries
                (org_id, application_id, session_id, language, summary, chapters, model,
                 prompt_tokens, completion_tokens, input_digest, generated_by,
                 generated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (org_id, session_id, language) DO UPDATE
             SET application_id = EXCLUDED.application_id,
                 summary = EXCLUDED.summary,
                 chapters = EXCLUDED.chapters,
                 model = EXCLUDED.model,
                 prompt_tokens = EXCLUDED.prompt_tokens,
                 completion_tokens = EXCLUDED.completion_tokens,
                 input_digest = EXCLUDED.input_digest,
                 generated_by = EXCLUDED.generated_by,
                 generated_at = EXCLUDED.generated_at",
        )
        .bind(&summary.org_id.0)
        .bind(&summary.application_id)
        .bind(&summary.session_id)
        .bind(&summary.language)
        .bind(&summary.summary)
        .bind(Json(&summary.chapters))
        .bind(&summary.model)
        .bind(summary.prompt_tokens)
        .bind(summary.completion_tokens)
        .bind(&summary.input_digest)
        .bind(summary.generated_by.as_ref().map(|id| id.0.as_str()))
        .bind(summary.generated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn delete_older_than(&self, cutoff: TimestampMicros, limit: usize) -> Result<usize> {
        let removed = sqlx::query(
            "DELETE FROM rum_session_summaries
              WHERE (org_id, session_id, language) IN (
                  SELECT org_id, session_id, language FROM rum_session_summaries
                   WHERE generated_at < $1
                   LIMIT $2)",
        )
        .bind(cutoff)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(usize::try_from(removed.rows_affected()).unwrap_or(usize::MAX))
    }
}
