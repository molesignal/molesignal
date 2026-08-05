// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::dashboard::{Dashboard, repositories::DashboardRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgDashboardRepository {
    pool: PgPool,
}

impl PgDashboardRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) const COLS: &str = "id, org_id, folder_id, uid, title, tags, model, version,
     created_at_micros, updated_at_micros, created_by, updated_by";

pub(crate) fn row_to_dashboard(row: sqlx::postgres::PgRow) -> Result<Dashboard> {
    let folder_id: Option<String> = row.try_get("folder_id").map_err(sqlx_err)?;
    let tags: Json<Vec<String>> = row.try_get("tags").map_err(sqlx_err)?;
    let model: Json<serde_json::Value> = row.try_get("model").map_err(sqlx_err)?;
    let version: i32 = row.try_get("version").map_err(sqlx_err)?;
    Ok(Dashboard {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        folder_id: folder_id.map(Id::from_string),
        uid: row.try_get("uid").map_err(sqlx_err)?,
        title: row.try_get("title").map_err(sqlx_err)?,
        tags: tags.0,
        model: model.0,
        version: version as u32,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
        created_by: Id::from_string(row.try_get::<String, _>("created_by").map_err(sqlx_err)?),
        updated_by: Id::from_string(row.try_get::<String, _>("updated_by").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl DashboardRepository for PgDashboardRepository {
    async fn create(&self, d: Dashboard) -> Result<Dashboard> {
        sqlx::query(
            "INSERT INTO dashboards
             (id, org_id, folder_id, uid, title, tags, model, version,
              created_at_micros, updated_at_micros, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&d.id.0)
        .bind(&d.org_id.0)
        .bind(d.folder_id.as_ref().map(|i| &i.0))
        .bind(&d.uid)
        .bind(&d.title)
        .bind(Json(&d.tags))
        .bind(Json(&d.model))
        .bind(d.version as i32)
        .bind(d.created_at.0)
        .bind(d.updated_at.0)
        .bind(&d.created_by.0)
        .bind(&d.updated_by.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(d)
    }

    async fn update(&self, d: Dashboard) -> Result<Dashboard> {
        sqlx::query(
            "UPDATE dashboards SET
               folder_id = $2, uid = $3, title = $4, tags = $5, model = $6,
               version = $7, updated_at_micros = $8, updated_by = $9
             WHERE id = $1",
        )
        .bind(&d.id.0)
        .bind(d.folder_id.as_ref().map(|i| &i.0))
        .bind(&d.uid)
        .bind(&d.title)
        .bind(Json(&d.tags))
        .bind(Json(&d.model))
        .bind(d.version as i32)
        .bind(d.updated_at.0)
        .bind(&d.updated_by.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(d)
    }

    async fn get(&self, id: &Id) -> Result<Dashboard> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM dashboards WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_dashboard(row)
    }

    async fn get_by_uid(&self, org_id: &Id, uid: &str) -> Result<Dashboard> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM dashboards WHERE org_id = $1 AND uid = $2"
        ))
        .bind(&org_id.0)
        .bind(uid)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_dashboard(row)
    }

    async fn list(&self, org_id: &Id, folder_id: Option<&Id>) -> Result<Vec<Dashboard>> {
        let rows = if let Some(fid) = folder_id {
            sqlx::query(&format!(
                "SELECT {COLS} FROM dashboards WHERE org_id = $1 AND folder_id = $2"
            ))
            .bind(&org_id.0)
            .bind(&fid.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        } else {
            sqlx::query(&format!("SELECT {COLS} FROM dashboards WHERE org_id = $1"))
                .bind(&org_id.0)
                .fetch_all(&self.pool)
                .await
                .map_err(sqlx_err)?
        };
        rows.into_iter().map(row_to_dashboard).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM dashboards WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
