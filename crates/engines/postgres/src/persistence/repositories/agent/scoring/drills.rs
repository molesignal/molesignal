// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Drills and their runs: `agent_drills`, one per incident, and `agent_drill_runs`.
//!
//! A run is graded once its Run (`agent_runs`) has ended or is gone: claiming takes those with a
//! lease, and takes back runs whose grading node went away.

use std::collections::HashMap;

use agent::scoring::drills::{
    CandidateRow, Drill, DrillRepository, DrillRun, DrillRunStatus, DrillSource, DrillSummary,
    MAX_DRILLS_PER_ORG,
};
use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{Row, postgres::PgRow};

use super::{PgScoringRepository, micros, optional_micros, optional_text, sqlx_err, text};

const DRILL_COLUMNS: &str = "id, org_id, incident_id, title, reference, source, created_by, \
    created_at, updated_at";

const RUN_COLUMNS: &str = "id, org_id, drill_id, chat_id, run_id, status, score, reasoning, \
    reason, detail, model, prompt_version, grader_model, language, batch_id, attempts, \
    started_by, created_at, finished_at, updated_at, not_before";

/// The most drills a list reads.
const LISTED: i64 = 200;

fn drill_row(row: &PgRow) -> Result<Drill> {
    Ok(Drill {
        id: Id(text(row, "id")?),
        org_id: Id(text(row, "org_id")?),
        incident_id: Id(text(row, "incident_id")?),
        title: text(row, "title")?,
        reference: text(row, "reference")?,
        source: DrillSource::parse(&text(row, "source")?)?,
        created_by: Id(text(row, "created_by")?),
        created_at: micros(row, "created_at")?,
        updated_at: micros(row, "updated_at")?,
    })
}

fn run_row(row: &PgRow) -> Result<DrillRun> {
    let id = |column: &str| -> Result<Option<Id>> { Ok(optional_text(row, column)?.map(Id)) };

    Ok(DrillRun {
        id: Id(text(row, "id")?),
        org_id: Id(text(row, "org_id")?),
        drill_id: Id(text(row, "drill_id")?),
        chat_id: id("chat_id")?,
        run_id: id("run_id")?,
        status: DrillRunStatus::parse(&text(row, "status")?)?,
        score: row.try_get("score").map_err(sqlx_err)?,
        reasoning: text(row, "reasoning")?,
        reason: optional_text(row, "reason")?,
        detail: optional_text(row, "detail")?,
        model: optional_text(row, "model")?,
        prompt_version: row.try_get("prompt_version").map_err(sqlx_err)?,
        grader_model: optional_text(row, "grader_model")?,
        language: text(row, "language")?,
        batch_id: id("batch_id")?,
        attempts: row.try_get("attempts").map_err(sqlx_err)?,
        started_by: Id(text(row, "started_by")?),
        created_at: micros(row, "created_at")?,
        finished_at: optional_micros(row, "finished_at")?,
        updated_at: micros(row, "updated_at")?,
        not_before: optional_micros(row, "not_before")?,
    })
}

#[async_trait]
impl DrillRepository for PgScoringRepository {
    async fn create_drill(&self, drill: &Drill) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_drills WHERE org_id = $1")
            .bind(&drill.org_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        if count >= MAX_DRILLS_PER_ORG as i64 {
            return Err(Error::invalid(format!(
                "an organization keeps at most {MAX_DRILLS_PER_ORG} drills; delete one first"
            )));
        }

        let inserted = sqlx::query(&format!(
            "INSERT INTO agent_drills ({DRILL_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (org_id, incident_id) DO NOTHING"
        ))
        .bind(&drill.id.0)
        .bind(&drill.org_id.0)
        .bind(&drill.incident_id.0)
        .bind(&drill.title)
        .bind(&drill.reference)
        .bind(drill.source.as_str())
        .bind(&drill.created_by.0)
        .bind(drill.created_at)
        .bind(drill.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if inserted.rows_affected() == 0 {
            return Err(Error::conflict("this incident already has a drill"));
        }

        Ok(())
    }

    async fn get_drill(&self, org_id: &Id, id: &Id) -> Result<Drill> {
        let row = sqlx::query(&format!(
            "SELECT {DRILL_COLUMNS} FROM agent_drills WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("drill `{}` not found", id.0)))?;

        drill_row(&row)
    }

    async fn list_drills(&self, org_id: &Id) -> Result<Vec<DrillSummary>> {
        let drills = sqlx::query(&format!(
            "SELECT {DRILL_COLUMNS} FROM agent_drills WHERE org_id = $1
             ORDER BY created_at DESC, id DESC LIMIT $2"
        ))
        .bind(&org_id.0)
        .bind(LISTED)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut last_runs = sqlx::query(&format!(
            "SELECT DISTINCT ON (drill_id) {RUN_COLUMNS} FROM agent_drill_runs
             WHERE org_id = $1
             ORDER BY drill_id, created_at DESC, id DESC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(run_row)
        .map(|run| run.map(|run| (run.drill_id.0.clone(), run)))
        .collect::<Result<HashMap<_, _>>>()?;

        let counts = sqlx::query(
            "SELECT drill_id, count(*) AS runs,
                    avg(score) FILTER (WHERE status = 'done') AS mean_score
             FROM agent_drill_runs WHERE org_id = $1
             GROUP BY drill_id",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(|row| -> Result<(String, (i64, Option<f64>))> {
            Ok((
                text(row, "drill_id")?,
                (
                    row.try_get("runs").map_err(sqlx_err)?,
                    row.try_get("mean_score").map_err(sqlx_err)?,
                ),
            ))
        })
        .collect::<Result<HashMap<_, _>>>()?;

        drills
            .iter()
            .map(|row| {
                let drill = drill_row(row)?;
                let (runs, mean_score) = counts.get(&drill.id.0).copied().unwrap_or((0, None));
                let last_run = last_runs.remove(&drill.id.0);

                Ok(DrillSummary {
                    drill,
                    runs,
                    mean_score,
                    last_run,
                })
            })
            .collect()
    }

    async fn delete_drill(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM agent_drills WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(deleted.rows_affected() > 0)
    }

    async fn drill_candidates(
        &self,
        org_id: &Id,
        since: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<CandidateRow>> {
        let rows = sqlx::query(
            "SELECT i.id, i.summary, i.severity, i.created_at, i.resolved_at, p.body,
                    EXISTS (
                      SELECT 1 FROM agent_drills d
                      WHERE d.org_id = i.org_id AND d.incident_id = i.id
                    ) AS drilled
             FROM incident_postmortems p
             JOIN incidents i ON i.id = p.incident_id AND i.org_id = p.org_id
             WHERE p.org_id = $1 AND i.status IN ('resolved', 'closed')
               AND i.resolved_at >= $2
             ORDER BY i.resolved_at DESC, i.id DESC
             LIMIT $3",
        )
        .bind(&org_id.0)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| {
                Ok(CandidateRow {
                    incident_id: Id(text(row, "id")?),
                    summary: text(row, "summary")?,
                    severity: text(row, "severity")?,
                    created_at: micros(row, "created_at")?,
                    resolved_at: optional_micros(row, "resolved_at")?,
                    body: text(row, "body")?,
                    drilled: row.try_get("drilled").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn create_drill_run(&self, run: &DrillRun) -> Result<()> {
        sqlx::query(&format!(
            "INSERT INTO agent_drill_runs ({RUN_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21)"
        ))
        .bind(&run.id.0)
        .bind(&run.org_id.0)
        .bind(&run.drill_id.0)
        .bind(run.chat_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.run_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.status.as_str())
        .bind(run.score)
        .bind(&run.reasoning)
        .bind(run.reason.as_deref())
        .bind(run.detail.as_deref())
        .bind(run.model.as_deref())
        .bind(run.prompt_version)
        .bind(run.grader_model.as_deref())
        .bind(&run.language)
        .bind(run.batch_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.attempts)
        .bind(&run.started_by.0)
        .bind(run.created_at)
        .bind(run.finished_at)
        .bind(run.updated_at)
        .bind(run.not_before)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn save_drill_run(&self, run: &DrillRun) -> Result<()> {
        sqlx::query(
            "UPDATE agent_drill_runs SET
               chat_id = $3, run_id = $4, status = $5, score = $6, reasoning = $7, reason = $8,
               detail = $9, model = $10, prompt_version = $11, grader_model = $12,
               attempts = $13, finished_at = $14, updated_at = $15, not_before = $16
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&run.org_id.0)
        .bind(&run.id.0)
        .bind(run.chat_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.run_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.status.as_str())
        .bind(run.score)
        .bind(&run.reasoning)
        .bind(run.reason.as_deref())
        .bind(run.detail.as_deref())
        .bind(run.model.as_deref())
        .bind(run.prompt_version)
        .bind(run.grader_model.as_deref())
        .bind(run.attempts)
        .bind(run.finished_at)
        .bind(run.updated_at)
        .bind(run.not_before)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list_drill_runs(
        &self,
        org_id: &Id,
        drill_id: &Id,
        limit: i64,
    ) -> Result<Vec<DrillRun>> {
        sqlx::query(&format!(
            "SELECT {RUN_COLUMNS} FROM agent_drill_runs
             WHERE org_id = $1 AND drill_id = $2
             ORDER BY created_at DESC, id DESC LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&drill_id.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(run_row)
        .collect()
    }

    async fn claim_drill_runs(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<DrillRun>> {
        sqlx::query(&format!(
            "UPDATE agent_drill_runs d
             SET status = 'grading', not_before = $2, attempts = d.attempts + 1,
                 updated_at = clock_timestamp()
             WHERE d.id IN (
               SELECT x.id FROM agent_drill_runs x
               LEFT JOIN agent_runs r ON r.id = x.run_id
               WHERE ((x.status = 'running'
                       AND (r.id IS NULL
                            OR r.status IN ('succeeded', 'failed', 'cancelled', 'interrupted')))
                      OR x.status = 'grading')
                 AND (x.not_before IS NULL OR x.not_before < $1)
               ORDER BY x.updated_at
               LIMIT $3
               FOR UPDATE OF x SKIP LOCKED
             )
             RETURNING {RUN_COLUMNS}"
        ))
        .bind(now)
        .bind(lease_until)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(run_row)
        .collect()
    }
}
