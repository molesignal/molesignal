// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `pipeline_runs` 表 Pg 实装（pipeline-runs-and-backfill）。
//!
//! 调度器每次运行写一行。状态机：`running` → {`succeeded` | `failed` | `cancelled`}。
//! 一次运行读入的事件，结束时各落在写出、丢弃、失败三者之一。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl PipelineRunState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineRunState::Running => "running",
            PipelineRunState::Succeeded => "succeeded",
            PipelineRunState::Failed => "failed",
            PipelineRunState::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Id,
    pub pipeline_id: Id,
    pub org_id: Id,
    pub state: PipelineRunState,
    pub started_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
    /// Events read from the sources.
    pub scanned_rows: i64,
    /// Events that went everywhere they were meant to.
    pub written_rows: i64,
    /// Events a step or the dedupe dropped on purpose.
    pub dropped_rows: i64,
    /// Events that failed a step, were rejected by a stream or were not delivered.
    pub failed_rows: i64,
    /// 1 for a first attempt at what the run read; more when it retried a failed window.
    pub attempt: i32,
    /// Why the run failed.
    pub error: Option<String>,
    /// What went wrong without failing the run.
    pub warning: Option<String>,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRunFinish {
    pub state: PipelineRunState,
    pub finished_at: TimestampMicros,
    pub scanned_rows: i64,
    pub written_rows: i64,
    pub dropped_rows: i64,
    pub failed_rows: i64,
    pub attempt: i32,
    pub error: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunSummary {
    pub pipeline_id: Id,
    pub last_state: PipelineRunState,
    pub last_started_at: TimestampMicros,
    pub last_finished_at: Option<TimestampMicros>,
    pub last_scanned_rows: i64,
    pub last_error: Option<String>,
    pub runs_in_window: i64,
    pub succeeded_runs_in_window: i64,
    pub failed_runs_in_window: i64,
}

#[async_trait]
pub trait PipelineRunRepository: Send + Sync {
    /// 调度入口 register；写入 `state = running` 的行。
    async fn record_start(&self, run: PipelineRun) -> Result<()>;
    /// 调度退出时更新终态、各项条数与错误。
    async fn record_finish(&self, id: &Id, finish: PipelineRunFinish) -> Result<()>;
    /// HTTP list_runs。可选 `before_micros` 游标，按 `started_at_micros DESC` 排序。
    async fn list(
        &self,
        org_id: &Id,
        pipeline_id: &Id,
        limit: i64,
        before_micros: Option<i64>,
    ) -> Result<Vec<PipelineRun>>;
    /// 每条流水线的最近一次运行与给定窗口内的运行/失败次数。
    async fn summaries(&self, org_id: &Id, since_micros: i64) -> Result<Vec<PipelineRunSummary>>;
}

pub struct PgPipelineRunRepository {
    pool: PgPool,
}

impl PgPipelineRunRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, pipeline_id, org_id, state, started_at_micros, finished_at_micros,
                    scanned_rows, written_rows, dropped_rows, failed_rows, attempt, error, warning";

fn row_to(r: sqlx::postgres::PgRow) -> PipelineRun {
    let state_str: String = r.try_get::<String, _>("state").unwrap_or_default();
    let state = state_from_str(&state_str);

    PipelineRun {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        pipeline_id: Id(r.try_get::<String, _>("pipeline_id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        state,
        started_at: TimestampMicros(r.try_get::<i64, _>("started_at_micros").unwrap_or_default()),
        finished_at: r
            .try_get::<Option<i64>, _>("finished_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        scanned_rows: r.try_get::<i64, _>("scanned_rows").unwrap_or_default(),
        written_rows: r.try_get::<i64, _>("written_rows").unwrap_or_default(),
        dropped_rows: r.try_get::<i64, _>("dropped_rows").unwrap_or_default(),
        failed_rows: r.try_get::<i64, _>("failed_rows").unwrap_or_default(),
        attempt: r.try_get::<i32, _>("attempt").unwrap_or(1),
        error: r.try_get::<Option<String>, _>("error").unwrap_or_default(),
        warning: r
            .try_get::<Option<String>, _>("warning")
            .unwrap_or_default(),
    }
}

fn state_from_str(value: &str) -> PipelineRunState {
    match value {
        "succeeded" => PipelineRunState::Succeeded,
        "failed" => PipelineRunState::Failed,
        "cancelled" => PipelineRunState::Cancelled,
        _ => PipelineRunState::Running,
    }
}

#[async_trait]
impl PipelineRunRepository for PgPipelineRunRepository {
    async fn record_start(&self, run: PipelineRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO pipeline_runs
                (id, pipeline_id, org_id, state, started_at_micros, finished_at_micros,
                 scanned_rows, written_rows, dropped_rows, failed_rows, attempt, error, warning)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&run.id.0)
        .bind(&run.pipeline_id.0)
        .bind(&run.org_id.0)
        .bind(run.state.as_str())
        .bind(run.started_at.0)
        .bind(run.finished_at.map(|t| t.0))
        .bind(run.scanned_rows)
        .bind(run.written_rows)
        .bind(run.dropped_rows)
        .bind(run.failed_rows)
        .bind(run.attempt)
        .bind(run.error)
        .bind(run.warning)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn record_finish(&self, id: &Id, finish: PipelineRunFinish) -> Result<()> {
        sqlx::query(
            "UPDATE pipeline_runs
                SET state = $2, finished_at_micros = $3, scanned_rows = $4, written_rows = $5,
                    dropped_rows = $6, failed_rows = $7, attempt = $8, error = $9, warning = $10
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(finish.state.as_str())
        .bind(finish.finished_at.0)
        .bind(finish.scanned_rows)
        .bind(finish.written_rows)
        .bind(finish.dropped_rows)
        .bind(finish.failed_rows)
        .bind(finish.attempt)
        .bind(finish.error)
        .bind(finish.warning)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list(
        &self,
        org_id: &Id,
        pipeline_id: &Id,
        limit: i64,
        before_micros: Option<i64>,
    ) -> Result<Vec<PipelineRun>> {
        let limit = limit.clamp(1, 500);

        let rows = match before_micros {
            Some(before) => {
                let sql = format!(
                    "SELECT {COLS} FROM pipeline_runs
                       WHERE org_id = $1 AND pipeline_id = $2 AND started_at_micros < $3
                       ORDER BY started_at_micros DESC
                       LIMIT $4"
                );

                sqlx::query(&sql)
                    .bind(&org_id.0)
                    .bind(&pipeline_id.0)
                    .bind(before)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(sqlx_err)?
            }
            None => {
                let sql = format!(
                    "SELECT {COLS} FROM pipeline_runs
                       WHERE org_id = $1 AND pipeline_id = $2
                       ORDER BY started_at_micros DESC
                       LIMIT $3"
                );

                sqlx::query(&sql)
                    .bind(&org_id.0)
                    .bind(&pipeline_id.0)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(sqlx_err)?
            }
        };

        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn summaries(&self, org_id: &Id, since_micros: i64) -> Result<Vec<PipelineRunSummary>> {
        let rows = sqlx::query(
            "WITH latest AS (
                 SELECT DISTINCT ON (pipeline_id)
                    pipeline_id, state, started_at_micros, finished_at_micros,
                    scanned_rows, error
                 FROM pipeline_runs
                 WHERE org_id = $1
                 ORDER BY pipeline_id, started_at_micros DESC
             ),
             recent AS (
                 SELECT pipeline_id,
                    COUNT(*)::BIGINT AS runs_in_window,
                    COUNT(*) FILTER (WHERE state = 'succeeded')::BIGINT AS succeeded_runs_in_window,
                    COUNT(*) FILTER (WHERE state = 'failed')::BIGINT AS failed_runs_in_window
                 FROM pipeline_runs
                 WHERE org_id = $1 AND started_at_micros >= $2
                 GROUP BY pipeline_id
             )
             SELECT
                latest.pipeline_id,
                latest.state,
                latest.started_at_micros,
                latest.finished_at_micros,
                latest.scanned_rows,
                latest.error,
                COALESCE(recent.runs_in_window, 0) AS runs_in_window,
                COALESCE(recent.succeeded_runs_in_window, 0) AS succeeded_runs_in_window,
                COALESCE(recent.failed_runs_in_window, 0) AS failed_runs_in_window
             FROM latest
             LEFT JOIN recent USING (pipeline_id)",
        )
        .bind(&org_id.0)
        .bind(since_micros)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let state = row
                    .try_get::<String, _>("state")
                    .unwrap_or_else(|_| "running".to_string());

                PipelineRunSummary {
                    pipeline_id: Id(row.try_get::<String, _>("pipeline_id").unwrap_or_default()),
                    last_state: state_from_str(&state),
                    last_started_at: TimestampMicros(
                        row.try_get::<i64, _>("started_at_micros")
                            .unwrap_or_default(),
                    ),
                    last_finished_at: row
                        .try_get::<Option<i64>, _>("finished_at_micros")
                        .unwrap_or_default()
                        .map(TimestampMicros),
                    last_scanned_rows: row.try_get::<i64, _>("scanned_rows").unwrap_or_default(),
                    last_error: row
                        .try_get::<Option<String>, _>("error")
                        .unwrap_or_default(),
                    runs_in_window: row.try_get::<i64, _>("runs_in_window").unwrap_or_default(),
                    succeeded_runs_in_window: row
                        .try_get::<i64, _>("succeeded_runs_in_window")
                        .unwrap_or_default(),
                    failed_runs_in_window: row
                        .try_get::<i64, _>("failed_runs_in_window")
                        .unwrap_or_default(),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_as_str_roundtrip() {
        assert_eq!(PipelineRunState::Running.as_str(), "running");
        assert_eq!(PipelineRunState::Succeeded.as_str(), "succeeded");
        assert_eq!(PipelineRunState::Failed.as_str(), "failed");
        assert_eq!(PipelineRunState::Cancelled.as_str(), "cancelled");
    }
}
