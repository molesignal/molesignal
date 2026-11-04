// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for the playlists of RUM sessions.

use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::rum::{
        MAX_PLAYLIST_SESSIONS, RumPlaylist, RumPlaylistEntry, RumPlaylistPatch,
        RumPlaylistRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgRumPlaylistRepository {
    pool: PgPool,
}

impl PgRumPlaylistRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A playlist with how many sessions it holds.
const SELECT: &str = "SELECT p.org_id, p.id, p.application_id, p.name, p.description, p.created_by,
        p.created_at, p.updated_at,
        (SELECT COUNT(*) FROM rum_playlist_sessions s
          WHERE s.org_id = p.org_id AND s.playlist_id = p.id) AS session_count
   FROM rum_playlists p";

fn row_to(row: sqlx::postgres::PgRow) -> RumPlaylist {
    RumPlaylist {
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        id: row.try_get::<String, _>("id").unwrap_or_default(),
        application_id: row
            .try_get::<String, _>("application_id")
            .unwrap_or_default(),
        name: row.try_get::<String, _>("name").unwrap_or_default(),
        description: row.try_get::<String, _>("description").unwrap_or_default(),
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .unwrap_or_default()
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .unwrap_or_default(),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .unwrap_or_default(),
        session_count: row
            .try_get::<i64, _>("session_count")
            .ok()
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or_default(),
    }
}

fn entry_to(row: sqlx::postgres::PgRow) -> RumPlaylistEntry {
    RumPlaylistEntry {
        session_id: row.try_get::<String, _>("session_id").unwrap_or_default(),
        at_time: row
            .try_get::<Option<TimestampMicros>, _>("at")
            .unwrap_or_default()
            .map(|t| t.0),
        added_by: row
            .try_get::<Option<String>, _>("added_by")
            .unwrap_or_default()
            .map(Id),
        added_at: row
            .try_get::<TimestampMicros, _>("added_at")
            .unwrap_or_default(),
    }
}

/// A taken name is the caller's mistake, not the database's.
fn conflict_or(error: sqlx::Error) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            Error::conflict("a playlist of this application already has this name")
        }
        _ => sqlx_err(error),
    }
}

fn not_found(id: &str) -> Error {
    Error::not_found(format!("RUM playlist `{id}` does not exist"))
}

/// Puts the sessions in, within the room a playlist has. The playlist row is locked first, so
/// two additions to one playlist take their turns and neither goes past the limit.
async fn put(
    transaction: &mut PgConnection,
    org_id: &Id,
    id: &str,
    entries: &[RumPlaylistEntry],
) -> Result<()> {
    let locked =
        sqlx::query("SELECT 1 FROM rum_playlists WHERE org_id = $1 AND id = $2 FOR UPDATE")
            .bind(&org_id.0)
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

    if locked.is_none() {
        return Err(not_found(id));
    }

    let held: Vec<String> = sqlx::query_scalar(
        "SELECT session_id FROM rum_playlist_sessions WHERE org_id = $1 AND playlist_id = $2",
    )
    .bind(&org_id.0)
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(sqlx_err)?;

    let new = entries
        .iter()
        .map(|entry| entry.session_id.as_str())
        .filter(|session| !held.iter().any(|kept| kept == session))
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    if held.len() + new > MAX_PLAYLIST_SESSIONS {
        return Err(Error::invalid(format!(
            "a playlist holds at most {MAX_PLAYLIST_SESSIONS} sessions"
        )));
    }

    for entry in entries {
        sqlx::query(
            "INSERT INTO rum_playlist_sessions
                (org_id, playlist_id, session_id, at, added_by, added_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (org_id, playlist_id, session_id)
             DO UPDATE SET at = EXCLUDED.at",
        )
        .bind(&org_id.0)
        .bind(id)
        .bind(&entry.session_id)
        .bind(entry.at_time.map(TimestampMicros))
        .bind(entry.added_by.as_ref().map(|id| id.0.as_str()))
        .bind(entry.added_at)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
    }

    sqlx::query("UPDATE rum_playlists SET updated_at = $3 WHERE org_id = $1 AND id = $2")
        .bind(&org_id.0)
        .bind(id)
        .bind(TimestampMicros::now())
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}

#[async_trait]
impl RumPlaylistRepository for PgRumPlaylistRepository {
    async fn create(
        &self,
        playlist: RumPlaylist,
        entries: Vec<RumPlaylistEntry>,
    ) -> Result<RumPlaylist> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO rum_playlists
                (org_id, id, application_id, name, description, created_by,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&playlist.org_id.0)
        .bind(&playlist.id)
        .bind(&playlist.application_id)
        .bind(playlist.name.trim())
        .bind(&playlist.description)
        .bind(playlist.created_by.as_ref().map(|id| id.0.as_str()))
        .bind(playlist.created_at)
        .bind(playlist.updated_at)
        .execute(&mut *transaction)
        .await
        .map_err(conflict_or)?;

        put(&mut transaction, &playlist.org_id, &playlist.id, &entries).await?;

        transaction.commit().await.map_err(sqlx_err)?;

        self.get(&playlist.org_id, &playlist.id).await
    }

    async fn get(&self, org_id: &Id, id: &str) -> Result<RumPlaylist> {
        let sql = format!("{SELECT} WHERE p.org_id = $1 AND p.id = $2");

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(row_to)
            .ok_or_else(|| not_found(id))
    }

    async fn list(&self, org_id: &Id, application_id: Option<&str>) -> Result<Vec<RumPlaylist>> {
        let sql = format!(
            "{SELECT} WHERE p.org_id = $1 AND ($2::TEXT IS NULL OR p.application_id = $2)
              ORDER BY p.updated_at DESC, p.id"
        );

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(application_id)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn update(&self, org_id: &Id, id: &str, patch: RumPlaylistPatch) -> Result<RumPlaylist> {
        let current = self.get(org_id, id).await?;

        sqlx::query(
            "UPDATE rum_playlists SET name = $3, description = $4, updated_at = $5
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(id)
        .bind(
            patch
                .name
                .as_deref()
                .map(str::trim)
                .unwrap_or(&current.name),
        )
        .bind(patch.description.as_deref().unwrap_or(&current.description))
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(conflict_or)?;

        self.get(org_id, id).await
    }

    async fn delete(&self, org_id: &Id, id: &str) -> Result<()> {
        let deleted = sqlx::query("DELETE FROM rum_playlists WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        if deleted == 0 {
            return Err(not_found(id));
        }

        Ok(())
    }

    async fn of_session(&self, org_id: &Id, session_id: &str) -> Result<Vec<RumPlaylist>> {
        let sql = format!(
            "{SELECT} WHERE p.org_id = $1
               AND EXISTS (SELECT 1 FROM rum_playlist_sessions s
                            WHERE s.org_id = p.org_id AND s.playlist_id = p.id AND s.session_id = $2)
              ORDER BY p.updated_at DESC, p.id"
        );

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn entries(&self, org_id: &Id, id: &str) -> Result<Vec<RumPlaylistEntry>> {
        // A playlist that does not exist is not one that is empty.
        self.get(org_id, id).await?;

        let rows = sqlx::query(
            "SELECT session_id, at, added_by, added_at
               FROM rum_playlist_sessions
              WHERE org_id = $1 AND playlist_id = $2
              ORDER BY added_at, session_id",
        )
        .bind(&org_id.0)
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(entry_to).collect())
    }

    async fn add(&self, org_id: &Id, id: &str, entries: Vec<RumPlaylistEntry>) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        put(&mut transaction, org_id, id, &entries).await?;

        transaction.commit().await.map_err(sqlx_err)
    }

    async fn remove(&self, org_id: &Id, id: &str, session_id: &str) -> Result<()> {
        self.get(org_id, id).await?;

        sqlx::query(
            "DELETE FROM rum_playlist_sessions
              WHERE org_id = $1 AND playlist_id = $2 AND session_id = $3",
        )
        .bind(&org_id.0)
        .bind(id)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        sqlx::query("UPDATE rum_playlists SET updated_at = $3 WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(id)
            .bind(TimestampMicros::now())
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
