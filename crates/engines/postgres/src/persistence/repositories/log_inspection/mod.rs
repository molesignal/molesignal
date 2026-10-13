// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `log_inspections` 与 `log_inspection_runs` 表的 Pg 实装：定时日志巡检的定义和执行记录。
//!
//! 定义在本文件，执行记录在 [`runs`]。同一条巡检同一时刻至多一条没结束的执行，由
//! `uq_log_inspection_runs_active` 这个部分唯一索引保证，不靠先查后写。

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::{
    domain::log_inspection::{
        InspectionLevel, InspectionNotifyRoute, InspectionSchedule, LastInspectionRun,
        LogInspection, NotifyWhen, RunStatus, repositories::LogInspectionRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod runs;

pub struct PgLogInspectionRepository {
    pool: PgPool,
}

impl PgLogInspectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, stream, statement, window_secs, requirements, schedule,
    timezone, provider_id, model, notify_when, notify_routes, language, enabled,
    run_as_user_id, next_run_at_micros, last_run_at_micros, last_run_status, last_run_level,
    created_by, created_at_micros, updated_at_micros";

fn id(row: &PgRow, column: &str) -> Result<Id> {
    Ok(Id::from_string(
        row.try_get::<String, _>(column).map_err(sqlx_err)?,
    ))
}

fn optional_id(row: &PgRow, column: &str) -> Result<Option<Id>> {
    Ok(row
        .try_get::<Option<String>, _>(column)
        .map_err(sqlx_err)?
        .map(Id::from_string))
}

fn timestamp(row: &PgRow, column: &str) -> Result<TimestampMicros> {
    Ok(TimestampMicros(row.try_get(column).map_err(sqlx_err)?))
}

fn optional_timestamp(row: &PgRow, column: &str) -> Result<Option<TimestampMicros>> {
    Ok(row
        .try_get::<Option<i64>, _>(column)
        .map_err(sqlx_err)?
        .map(TimestampMicros))
}

fn optional_level(row: &PgRow, column: &str) -> Result<Option<InspectionLevel>> {
    row.try_get::<Option<String>, _>(column)
        .map_err(sqlx_err)?
        .map(|level| InspectionLevel::parse(&level))
        .transpose()
}

fn row_to_inspection(row: PgRow) -> Result<LogInspection> {
    let schedule: Json<InspectionSchedule> = row.try_get("schedule").map_err(sqlx_err)?;
    let routes: Json<Vec<InspectionNotifyRoute>> =
        row.try_get("notify_routes").map_err(sqlx_err)?;
    let notify_when: String = row.try_get("notify_when").map_err(sqlx_err)?;

    // 三列一起写、一起空：有时间就一定有状态。
    let last_run = match optional_timestamp(&row, "last_run_at_micros")? {
        Some(at) => {
            let status: Option<String> = row.try_get("last_run_status").map_err(sqlx_err)?;

            Some(LastInspectionRun {
                at,
                status: RunStatus::parse(status.as_deref().unwrap_or_default())?,
                level: optional_level(&row, "last_run_level")?,
            })
        }
        None => None,
    };

    Ok(LogInspection {
        id: id(&row, "id")?,
        org_id: id(&row, "org_id")?,
        name: row.try_get("name").map_err(sqlx_err)?,
        stream: row.try_get("stream").map_err(sqlx_err)?,
        statement: row.try_get("statement").map_err(sqlx_err)?,
        window_secs: row.try_get("window_secs").map_err(sqlx_err)?,
        requirements: row.try_get("requirements").map_err(sqlx_err)?,
        schedule: schedule.0,
        timezone: row.try_get("timezone").map_err(sqlx_err)?,
        provider_id: id(&row, "provider_id")?,
        model: row.try_get("model").map_err(sqlx_err)?,
        notify_when: NotifyWhen::parse(&notify_when)?,
        notify_routes: routes.0,
        language: row.try_get("language").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        run_as_user_id: id(&row, "run_as_user_id")?,
        next_run_at: optional_timestamp(&row, "next_run_at_micros")?,
        last_run,
        created_by: id(&row, "created_by")?,
        created_at: timestamp(&row, "created_at_micros")?,
        updated_at: timestamp(&row, "updated_at_micros")?,
    })
}

/// 没有命中任何行的写入：这条巡检不存在，或不在这个组织里。
fn missing() -> Error {
    Error::not_found("log inspection")
}

#[async_trait]
impl LogInspectionRepository for PgLogInspectionRepository {
    async fn create(&self, inspection: LogInspection) -> Result<LogInspection> {
        let sql = format!(
            "INSERT INTO log_inspections (
                 id, org_id, name, stream, statement, window_secs, requirements, schedule,
                 timezone, provider_id, model, notify_when, notify_routes, language, enabled,
                 run_as_user_id, next_run_at_micros, created_by, created_at_micros,
                 updated_at_micros
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, $12, $13, $14, $15, $16, $17, $18, $19, $20
             )
             RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&inspection.id.0)
            .bind(&inspection.org_id.0)
            .bind(&inspection.name)
            .bind(&inspection.stream)
            .bind(&inspection.statement)
            .bind(inspection.window_secs)
            .bind(&inspection.requirements)
            .bind(Json(&inspection.schedule))
            .bind(&inspection.timezone)
            .bind(&inspection.provider_id.0)
            .bind(&inspection.model)
            .bind(inspection.notify_when.as_str())
            .bind(Json(&inspection.notify_routes))
            .bind(&inspection.language)
            .bind(inspection.enabled)
            .bind(&inspection.run_as_user_id.0)
            .bind(inspection.next_run_at.map(|at| at.0))
            .bind(&inspection.created_by.0)
            .bind(inspection.created_at.0)
            .bind(inspection.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row_to_inspection(row)
    }

    async fn update(&self, inspection: LogInspection) -> Result<LogInspection> {
        let sql = format!(
            "UPDATE log_inspections SET
                 name = $3, stream = $4, statement = $5, window_secs = $6, requirements = $7,
                 schedule = $8, timezone = $9, provider_id = $10, model = $11,
                 notify_when = $12, notify_routes = $13, language = $14, enabled = $15,
                 run_as_user_id = $16, next_run_at_micros = $17, updated_at_micros = $18
             WHERE id = $1 AND org_id = $2
             RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&inspection.id.0)
            .bind(&inspection.org_id.0)
            .bind(&inspection.name)
            .bind(&inspection.stream)
            .bind(&inspection.statement)
            .bind(inspection.window_secs)
            .bind(&inspection.requirements)
            .bind(Json(&inspection.schedule))
            .bind(&inspection.timezone)
            .bind(&inspection.provider_id.0)
            .bind(&inspection.model)
            .bind(inspection.notify_when.as_str())
            .bind(Json(&inspection.notify_routes))
            .bind(&inspection.language)
            .bind(inspection.enabled)
            .bind(&inspection.run_as_user_id.0)
            .bind(inspection.next_run_at.map(|at| at.0))
            .bind(inspection.updated_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(missing)?;

        row_to_inspection(row)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<LogInspection> {
        let sql = format!("SELECT {COLS} FROM log_inspections WHERE org_id = $1 AND id = $2");

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(missing)?;

        row_to_inspection(row)
    }

    async fn get_by_id(&self, id: &Id) -> Result<LogInspection> {
        let sql = format!("SELECT {COLS} FROM log_inspections WHERE id = $1");

        let row = sqlx::query(&sql)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(missing)?;

        row_to_inspection(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<LogInspection>> {
        let sql = format!("SELECT {COLS} FROM log_inspections WHERE org_id = $1 ORDER BY name, id");

        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_inspection)
            .collect()
    }

    async fn count(&self, org_id: &Id) -> Result<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM log_inspections WHERE org_id = $1")
            .bind(&org_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        // 执行记录随外键一起删除。
        let deleted = sqlx::query("DELETE FROM log_inspections WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        if deleted == 0 {
            return Err(missing());
        }

        Ok(())
    }

    async fn set_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
        next_run_at: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<LogInspection> {
        let sql = format!(
            "UPDATE log_inspections
                SET enabled = $3, next_run_at_micros = $4, updated_at_micros = $5
              WHERE org_id = $1 AND id = $2
              RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .bind(enabled)
            .bind(next_run_at.map(|at| at.0))
            .bind(now.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(missing)?;

        row_to_inspection(row)
    }

    async fn list_due(&self, now: TimestampMicros, limit: i64) -> Result<Vec<LogInspection>> {
        let sql = format!(
            "SELECT {COLS} FROM log_inspections
              WHERE enabled AND next_run_at_micros IS NOT NULL AND next_run_at_micros <= $1
              ORDER BY next_run_at_micros, id
              LIMIT $2"
        );

        sqlx::query(&sql)
            .bind(now.0)
            .bind(limit.max(1))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_inspection)
            .collect()
    }
}
