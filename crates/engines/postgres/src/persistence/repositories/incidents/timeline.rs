// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `incident_timeline`（事件的时间线与备注）的 Pg 实装。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::alerting::timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgIncidentTimelineRepository {
    pool: PgPool,
}

impl PgIncidentTimelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, incident_id, at, kind, actor_user_id, detail";

fn row_to(row: sqlx::postgres::PgRow) -> Result<TimelineEntry> {
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;
    let detail: Json<serde_json::Value> = row.try_get("detail").map_err(sqlx_err)?;

    Ok(TimelineEntry {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        incident_id: Id::from_string(row.try_get::<String, _>("incident_id").map_err(sqlx_err)?),
        at: row.try_get::<TimestampMicros, _>("at").map_err(sqlx_err)?,
        kind: TimelineKind::parse(&kind)?,
        actor: row
            .try_get::<Option<String>, _>("actor_user_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        detail: detail.0,
    })
}

#[async_trait]
impl IncidentTimelineRepository for PgIncidentTimelineRepository {
    async fn append(&self, entry: TimelineEntry) -> Result<TimelineEntry> {
        sqlx::query(
            "INSERT INTO incident_timeline
             (id, org_id, incident_id, at, kind, actor_user_id, detail)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&entry.id.0)
        .bind(&entry.org_id.0)
        .bind(&entry.incident_id.0)
        .bind(entry.at)
        .bind(entry.kind.as_str())
        .bind(entry.actor.as_ref().map(|actor| &actor.0))
        .bind(Json(&entry.detail))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(entry)
    }

    async fn list(&self, org_id: &Id, incident_id: &Id) -> Result<Vec<TimelineEntry>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incident_timeline
             WHERE org_id = $1 AND incident_id = $2
             ORDER BY at, id"
        ))
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn list_for_incidents(
        &self,
        org_id: &Id,
        incident_ids: &[Id],
        kinds: &[TimelineKind],
    ) -> Result<Vec<TimelineEntry>> {
        if incident_ids.is_empty() || kinds.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = incident_ids.iter().map(|id| id.0.as_str()).collect();
        let kinds: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();

        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incident_timeline
             WHERE org_id = $1 AND incident_id = ANY($2) AND kind = ANY($3)
             ORDER BY at, id"
        ))
        .bind(&org_id.0)
        .bind(&ids)
        .bind(&kinds)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn recent_notes(
        &self,
        org_id: &Id,
        incident_id: &Id,
        limit: u32,
    ) -> Result<Vec<TimelineEntry>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incident_timeline
             WHERE org_id = $1 AND incident_id = $2 AND kind = 'note'
             ORDER BY at DESC, id DESC
             LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }
}
