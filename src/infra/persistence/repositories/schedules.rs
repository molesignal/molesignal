// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        repositories::ScheduleRepository,
        schedule::{Rotation, Schedule, ScheduleOverride},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgScheduleRepository {
    pool: PgPool,
}

impl PgScheduleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, description, team_id, timezone, enabled,
    rotations, overrides, created_by, updated_by, created_at_micros, updated_at_micros";

fn row_to_schedule(row: sqlx::postgres::PgRow) -> Result<Schedule> {
    let rotations: Json<Vec<Rotation>> = row.try_get("rotations").map_err(sqlx_err)?;
    let overrides: Json<Vec<ScheduleOverride>> = row.try_get("overrides").map_err(sqlx_err)?;
    Ok(Schedule {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        team_id: row
            .try_get::<Option<String>, _>("team_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        timezone: row.try_get("timezone").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        rotations: rotations.0,
        overrides: overrides.0,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        updated_by: row
            .try_get::<Option<String>, _>("updated_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl ScheduleRepository for PgScheduleRepository {
    async fn create(&self, s: Schedule) -> Result<Schedule> {
        sqlx::query(
            "INSERT INTO schedules
             (id, org_id, name, description, team_id, timezone, enabled,
              rotations, overrides, created_by, updated_by,
              created_at_micros, updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(&s.id.0)
        .bind(&s.org_id.0)
        .bind(&s.name)
        .bind(&s.description)
        .bind(s.team_id.as_ref().map(|id| &id.0))
        .bind(&s.timezone)
        .bind(s.enabled)
        .bind(Json(&s.rotations))
        .bind(Json(&s.overrides))
        .bind(s.created_by.as_ref().map(|id| &id.0))
        .bind(s.updated_by.as_ref().map(|id| &id.0))
        .bind(s.created_at.0)
        .bind(s.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(s)
    }

    async fn update(&self, s: Schedule) -> Result<Schedule> {
        sqlx::query(
            "UPDATE schedules SET
               name = $2, description = $3, team_id = $4, timezone = $5,
               enabled = $6, rotations = $7, overrides = $8, updated_by = $9,
               updated_at_micros = $10
             WHERE id = $1",
        )
        .bind(&s.id.0)
        .bind(&s.name)
        .bind(&s.description)
        .bind(s.team_id.as_ref().map(|id| &id.0))
        .bind(&s.timezone)
        .bind(s.enabled)
        .bind(Json(&s.rotations))
        .bind(Json(&s.overrides))
        .bind(s.updated_by.as_ref().map(|id| &id.0))
        .bind(s.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(s)
    }

    async fn get(&self, id: &Id) -> Result<Schedule> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM schedules WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_schedule(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Schedule>> {
        let rows = sqlx::query(&format!("SELECT {COLS} FROM schedules WHERE org_id = $1"))
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_schedule).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
