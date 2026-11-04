// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `holiday_calendars` 表：组织的节假日日历。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::holiday::{
        CalendarReference, HolidayCalendar, HolidayCalendarRepository, HolidayDay,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgHolidayCalendarRepository {
    pool: PgPool,
}

impl PgHolidayCalendarRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, description, days, created_by, created_at, updated_at";

/// 能引用日历的地方：表、种类、组织列、名称列、存着时间段的 JSONB 列。日历 id 是随机串，按文本找
/// 就够准。
const REFERRERS: [(&str, &str, &str, &str); 6] = [
    (
        "alert_routing_rules",
        "routing_rule",
        "org_id",
        "time_condition",
    ),
    (
        "escalation_policies",
        "escalation_policy",
        "org_id",
        "urgency_rule",
    ),
    ("schedules", "schedule", "org_id", "rotations"),
    ("mute_rules", "mute_rule", "org_id", "time_window"),
    (
        "notify_policies",
        "notify_policy",
        "organization_id",
        "time",
    ),
    (
        "scheduled_pipelines",
        "alert_pipeline",
        "org_id",
        "function_steps",
    ),
];

fn row_to(row: sqlx::postgres::PgRow) -> Result<HolidayCalendar> {
    let days: Json<Vec<HolidayDay>> = row.try_get("days").map_err(sqlx_err)?;

    Ok(HolidayCalendar {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        days: days.0,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl HolidayCalendarRepository for PgHolidayCalendarRepository {
    async fn create(&self, calendar: HolidayCalendar) -> Result<HolidayCalendar> {
        sqlx::query(
            "INSERT INTO holiday_calendars
                 (id, org_id, name, description, days, created_by, created_at,
                  updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&calendar.id.0)
        .bind(&calendar.org_id.0)
        .bind(&calendar.name)
        .bind(&calendar.description)
        .bind(Json(&calendar.days))
        .bind(calendar.created_by.as_ref().map(|id| id.0.as_str()))
        .bind(calendar.created_at)
        .bind(calendar.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(calendar)
    }

    async fn update(&self, calendar: HolidayCalendar) -> Result<HolidayCalendar> {
        let updated = sqlx::query(
            "UPDATE holiday_calendars
                SET name = $3, description = $4, days = $5, updated_at = $6
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&calendar.org_id.0)
        .bind(&calendar.id.0)
        .bind(&calendar.name)
        .bind(&calendar.description)
        .bind(Json(&calendar.days))
        .bind(calendar.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if updated.rows_affected() == 0 {
            return Err(Error::not_found("holiday calendar"));
        }

        Ok(calendar)
    }

    async fn get(&self, id: &Id) -> Result<HolidayCalendar> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM holiday_calendars WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found("holiday calendar"))?;

        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<HolidayCalendar>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM holiday_calendars WHERE org_id = $1 ORDER BY name, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn list_all(&self) -> Result<Vec<HolidayCalendar>> {
        let rows = sqlx::query(&format!("SELECT {COLS} FROM holiday_calendars"))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM holiday_calendars WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn references(&self, org_id: &Id, id: &Id) -> Result<Vec<CalendarReference>> {
        let union = REFERRERS
            .iter()
            .map(|(table, kind, org, column)| {
                format!(
                    "SELECT '{kind}' AS kind, id, name FROM {table}
                      WHERE {org} = $1 AND strpos({column}::text, $2) > 0"
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION ALL ");

        let rows = sqlx::query(&format!("{union} ORDER BY kind, name"))
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(CalendarReference {
                    kind: row.try_get("kind").map_err(sqlx_err)?,
                    id: Id(row.try_get("id").map_err(sqlx_err)?),
                    name: row.try_get("name").map_err(sqlx_err)?,
                })
            })
            .collect()
    }
}
