// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::alerting::calendar::{CalendarFeed, CalendarFeedRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgCalendarFeedRepository {
    pool: PgPool,
}

impl PgCalendarFeedRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, user_id, schedule_id, token_hash, created_at, last_used_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<CalendarFeed> {
    Ok(CalendarFeed {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        schedule_id: row
            .try_get::<Option<String>, _>("schedule_id")
            .map_err(sqlx_err)?
            .map(Id),
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        last_used_at: row
            .try_get::<Option<TimestampMicros>, _>("last_used_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl CalendarFeedRepository for PgCalendarFeedRepository {
    async fn replace(&self, feed: CalendarFeed) -> Result<CalendarFeed> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "DELETE FROM calendar_feeds
              WHERE org_id = $1 AND user_id = $2 AND COALESCE(schedule_id, '') = COALESCE($3, '')",
        )
        .bind(&feed.org_id.0)
        .bind(&feed.user_id.0)
        .bind(feed.schedule_id.as_ref().map(|id| &id.0))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO calendar_feeds
               (id, org_id, user_id, schedule_id, token_hash, created_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&feed.id.0)
        .bind(&feed.org_id.0)
        .bind(&feed.user_id.0)
        .bind(feed.schedule_id.as_ref().map(|id| &id.0))
        .bind(&feed.token_hash)
        .bind(feed.created_at)
        .bind(feed.last_used_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(feed)
    }

    async fn list_for_user(&self, org_id: &Id, user_id: &Id) -> Result<Vec<CalendarFeed>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM calendar_feeds
              WHERE org_id = $1 AND user_id = $2
           ORDER BY created_at"
        ))
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<CalendarFeed>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM calendar_feeds WHERE token_hash = $1"
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to).transpose()
    }

    async fn delete(&self, org_id: &Id, user_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query(
            "DELETE FROM calendar_feeds WHERE org_id = $1 AND user_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .bind(&id.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(deleted.rows_affected() > 0)
    }

    async fn touch(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE calendar_feeds SET last_used_at = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
