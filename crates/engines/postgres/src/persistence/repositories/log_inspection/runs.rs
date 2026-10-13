// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `log_inspection_runs`：巡检的执行记录。结束一次执行时同时改写巡检行上的「最近一次执行」，
//! 两处在一条语句或一个事务里完成，列表不会读到只改了一半的状态。

use async_trait::async_trait;
use sqlx::{Row, TracedQuery, postgres::PgRow, types::Json};

use super::{
    super::sqlx_err, PgLogInspectionRepository, id, optional_id, optional_level,
    optional_timestamp, timestamp,
};
use crate::{
    domain::log_inspection::{
        InspectionNotification, InspectionToolCall, LogInspectionRun, RunCompletion, RunFailure,
        RunStatus, RunTrigger, repositories::LogInspectionRunRepository,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "id, inspection_id, org_id, trigger_kind, status, level, window_start_micros,
    window_end_micros, summary, report, failure, error, provider_id, model, run_as_user_id,
    requested_by, tool_calls, prompt_tokens, completion_tokens, notifications,
    created_at_micros, started_at_micros, finished_at_micros";

/// 列表用：报告正文可能有几千字，一页几十条时不取。
const COLS_WITHOUT_REPORT: &str = "id, inspection_id, org_id, trigger_kind, status, level,
    window_start_micros, window_end_micros, summary, NULL::TEXT AS report, failure, error,
    provider_id, model, run_as_user_id, requested_by, tool_calls, prompt_tokens,
    completion_tokens, notifications, created_at_micros, started_at_micros, finished_at_micros";

fn row_to_run(row: PgRow) -> Result<LogInspectionRun> {
    let trigger: String = row.try_get("trigger_kind").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let failure: Option<String> = row.try_get("failure").map_err(sqlx_err)?;
    let tool_calls: Json<Vec<InspectionToolCall>> = row.try_get("tool_calls").map_err(sqlx_err)?;
    let notifications: Json<Vec<InspectionNotification>> =
        row.try_get("notifications").map_err(sqlx_err)?;

    Ok(LogInspectionRun {
        id: id(&row, "id")?,
        inspection_id: id(&row, "inspection_id")?,
        org_id: id(&row, "org_id")?,
        trigger: RunTrigger::parse(&trigger)?,
        status: RunStatus::parse(&status)?,
        level: optional_level(&row, "level")?,
        window_start: timestamp(&row, "window_start_micros")?,
        window_end: timestamp(&row, "window_end_micros")?,
        summary: row.try_get("summary").map_err(sqlx_err)?,
        report: row.try_get("report").map_err(sqlx_err)?,
        failure: failure
            .map(|failure| RunFailure::parse(&failure))
            .transpose()?,
        error: row.try_get("error").map_err(sqlx_err)?,
        provider_id: optional_id(&row, "provider_id")?,
        model: row.try_get("model").map_err(sqlx_err)?,
        run_as_user_id: optional_id(&row, "run_as_user_id")?,
        requested_by: optional_id(&row, "requested_by")?,
        tool_calls: tool_calls.0,
        prompt_tokens: row.try_get("prompt_tokens").map_err(sqlx_err)?,
        completion_tokens: row.try_get("completion_tokens").map_err(sqlx_err)?,
        notifications: notifications.0,
        created_at: timestamp(&row, "created_at_micros")?,
        started_at: optional_timestamp(&row, "started_at_micros")?,
        finished_at: optional_timestamp(&row, "finished_at_micros")?,
    })
}

fn insert_sql() -> String {
    format!(
        "INSERT INTO log_inspection_runs (
             id, inspection_id, org_id, trigger_kind, status, window_start_micros,
             window_end_micros, provider_id, model, run_as_user_id, requested_by,
             created_at_micros, started_at_micros
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         RETURNING {COLS}"
    )
}

/// 新记录只带「谁、何时、查哪段时间」；报告、失败原因和 token 数在结束时才有。
fn bind_new_run<'q>(query: TracedQuery<'q>, run: &'q LogInspectionRun) -> TracedQuery<'q> {
    query
        .bind(&run.id.0)
        .bind(&run.inspection_id.0)
        .bind(&run.org_id.0)
        .bind(run.trigger.as_str())
        .bind(run.status.as_str())
        .bind(run.window_start.0)
        .bind(run.window_end.0)
        .bind(run.provider_id.as_ref().map(|value| value.0.as_str()))
        .bind(&run.model)
        .bind(run.run_as_user_id.as_ref().map(|value| value.0.as_str()))
        .bind(run.requested_by.as_ref().map(|value| value.0.as_str()))
        .bind(run.created_at.0)
        .bind(run.started_at.map(|at| at.0))
}

#[async_trait]
impl LogInspectionRunRepository for PgLogInspectionRepository {
    async fn begin_scheduled(
        &self,
        run: LogInspectionRun,
        next_run_at: Option<TimestampMicros>,
    ) -> Result<LogInspectionRun> {
        let sql = insert_sql();
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let row = bind_new_run(sqlx::query(&sql), &run)
            .fetch_one(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        sqlx::query("UPDATE log_inspections SET next_run_at_micros = $2 WHERE id = $1")
            .bind(&run.inspection_id.0)
            .bind(next_run_at.map(|at| at.0))
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        transaction.commit().await.map_err(sqlx_err)?;
        row_to_run(row)
    }

    async fn enqueue(&self, run: LogInspectionRun) -> Result<LogInspectionRun> {
        let sql = insert_sql();

        let row = bind_new_run(sqlx::query(&sql), &run)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row_to_run(row)
    }

    async fn list_queued(&self, limit: i64) -> Result<Vec<LogInspectionRun>> {
        let sql = format!(
            "SELECT {COLS} FROM log_inspection_runs
              WHERE status = 'queued'
              ORDER BY created_at_micros, id
              LIMIT $1"
        );

        sqlx::query(&sql)
            .bind(limit.max(1))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_run)
            .collect()
    }

    async fn start_queued(
        &self,
        run_id: &Id,
        started_at: TimestampMicros,
        window_start: TimestampMicros,
        window_end: TimestampMicros,
    ) -> Result<bool> {
        let started = sqlx::query(
            "UPDATE log_inspection_runs
                SET status = 'running', started_at_micros = $2,
                    window_start_micros = $3, window_end_micros = $4
              WHERE id = $1 AND status = 'queued'",
        )
        .bind(&run_id.0)
        .bind(started_at.0)
        .bind(window_start.0)
        .bind(window_end.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(started == 1)
    }

    async fn finish(&self, run_id: &Id, completion: RunCompletion) -> Result<bool> {
        // 记录与巡检行在一条语句里改：记录已经被别处结束时两处都不动。
        let finished: Option<String> = sqlx::query_scalar(
            "WITH finished AS (
                 UPDATE log_inspection_runs
                    SET status = $2, level = $3, summary = $4, report = $5, failure = $6,
                        error = $7, provider_id = $8, model = $9, run_as_user_id = $10,
                        tool_calls = $11, prompt_tokens = $12, completion_tokens = $13,
                        finished_at_micros = $14
                  WHERE id = $1 AND status IN ('queued', 'running')
              RETURNING inspection_id
             )
             UPDATE log_inspections AS inspection
                SET last_run_at_micros = $14, last_run_status = $2, last_run_level = $3
               FROM finished
              WHERE inspection.id = finished.inspection_id
          RETURNING inspection.id",
        )
        .bind(&run_id.0)
        .bind(completion.status.as_str())
        .bind(completion.level.map(|level| level.as_str()))
        .bind(&completion.summary)
        .bind(&completion.report)
        .bind(completion.failure.map(|failure| failure.as_str()))
        .bind(&completion.error)
        .bind(
            completion
                .provider_id
                .as_ref()
                .map(|value| value.0.as_str()),
        )
        .bind(&completion.model)
        .bind(
            completion
                .run_as_user_id
                .as_ref()
                .map(|value| value.0.as_str()),
        )
        .bind(Json(&completion.tool_calls))
        .bind(completion.prompt_tokens)
        .bind(completion.completion_tokens)
        .bind(completion.finished_at.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(finished.is_some())
    }

    async fn record_notifications(
        &self,
        run_id: &Id,
        notifications: &[InspectionNotification],
    ) -> Result<()> {
        sqlx::query("UPDATE log_inspection_runs SET notifications = $2 WHERE id = $1")
            .bind(&run_id.0)
            .bind(Json(notifications))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list_stale(
        &self,
        running_before: TimestampMicros,
        queued_before: TimestampMicros,
    ) -> Result<Vec<LogInspectionRun>> {
        let sql = format!(
            "SELECT {COLS_WITHOUT_REPORT} FROM log_inspection_runs
              WHERE (status = 'running' AND started_at_micros < $1)
                 OR (status = 'queued' AND created_at_micros < $2)
              ORDER BY created_at_micros, id
              LIMIT 200"
        );

        sqlx::query(&sql)
            .bind(running_before.0)
            .bind(queued_before.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_run)
            .collect()
    }

    async fn list_active(&self, org_id: &Id) -> Result<Vec<LogInspectionRun>> {
        let sql = format!(
            "SELECT {COLS_WITHOUT_REPORT} FROM log_inspection_runs
              WHERE org_id = $1 AND status IN ('queued', 'running')
              ORDER BY created_at_micros, id"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_run)
            .collect()
    }

    async fn list(
        &self,
        org_id: &Id,
        inspection_id: &Id,
        before: Option<TimestampMicros>,
        limit: i64,
    ) -> Result<Vec<LogInspectionRun>> {
        let sql = format!(
            "SELECT {COLS_WITHOUT_REPORT} FROM log_inspection_runs
              WHERE org_id = $1 AND inspection_id = $2
                AND ($3::BIGINT IS NULL OR created_at_micros < $3)
              ORDER BY created_at_micros DESC, id DESC
              LIMIT $4"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&inspection_id.0)
            .bind(before.map(|at| at.0))
            .bind(limit.clamp(1, 200))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_run)
            .collect()
    }

    async fn get(&self, org_id: &Id, inspection_id: &Id, run_id: &Id) -> Result<LogInspectionRun> {
        let sql = format!(
            "SELECT {COLS} FROM log_inspection_runs
              WHERE org_id = $1 AND inspection_id = $2 AND id = $3"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&inspection_id.0)
            .bind(&run_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| crate::shared::Error::not_found("log inspection run"))?;

        row_to_run(row)
    }

    async fn delete_before(&self, cutoff: TimestampMicros) -> Result<u64> {
        let deleted = sqlx::query(
            "DELETE FROM log_inspection_runs
              WHERE created_at_micros < $1 AND status NOT IN ('queued', 'running')",
        )
        .bind(cutoff.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(deleted)
    }
}
