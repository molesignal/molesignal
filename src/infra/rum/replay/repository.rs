// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::{MAX_REPLAY_SEGMENTS_PER_SESSION, RumReplayMetaRepository, RumReplayRecord};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

pub struct PgRumReplayMetaRepository {
    pool: PgPool,
}

impl PgRumReplayMetaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RumReplayMetaRepository for PgRumReplayMetaRepository {
    async fn find_segment(
        &self,
        org_id: &Id,
        application_id: &str,
        session_id: &str,
        seq: i32,
    ) -> Result<Option<RumReplayRecord>> {
        sqlx::query(
            "SELECT id, org_id, application_id, session_id, seq, object_key, bytes_uncompressed, event_count,
                    has_full_snapshot, content_hash, first_event_at_micros, created_at_micros
             FROM rum_replay_events
             WHERE org_id = $1 AND application_id = $2 AND session_id = $3 AND seq = $4",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .bind(session_id)
        .bind(seq)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)
        .map(|row| row.map(record_from_row))
    }

    async fn insert_if_absent(&self, row: &RumReplayRecord) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO rum_replay_events
                (id, org_id, application_id, session_id, seq, object_key, bytes_uncompressed,
                 event_count, has_full_snapshot, content_hash, first_event_at_micros,
                 created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (org_id, application_id, session_id, seq) DO NOTHING",
        )
        .bind(&row.id.0)
        .bind(&row.org_id.0)
        .bind(&row.application_id)
        .bind(&row.session_id)
        .bind(row.seq)
        .bind(&row.object_key)
        .bind(row.bytes_uncompressed as i64)
        .bind(row.event_count as i32)
        .bind(row.has_full_snapshot)
        .bind(&row.content_hash)
        .bind(row.first_event_at_micros)
        .bind(row.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(result.rows_affected() == 1)
    }

    async fn list_for_session(
        &self,
        org_id: &Id,
        session_id: &str,
    ) -> Result<Vec<RumReplayRecord>> {
        sqlx::query(
            "SELECT id, org_id, application_id, session_id, seq, object_key, bytes_uncompressed, event_count,
                    has_full_snapshot, content_hash, first_event_at_micros, created_at_micros
             FROM rum_replay_events WHERE org_id = $1 AND session_id = $2
             ORDER BY seq ASC LIMIT $3",
        )
        .bind(&org_id.0)
        .bind(session_id)
        .bind((MAX_REPLAY_SEGMENTS_PER_SESSION + 1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)
        .map(|rows| rows.into_iter().map(record_from_row).collect())
    }

    async fn session_usage(
        &self,
        org_id: &Id,
        application_id: &str,
        session_id: &str,
    ) -> Result<(u64, usize)> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(bytes_uncompressed), 0)::BIGINT AS total,
                    COUNT(*) AS segments
             FROM rum_replay_events
             WHERE org_id = $1 AND application_id = $2 AND session_id = $3",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .bind(session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;
        let total = row
            .try_get::<i64, _>("total")
            .map_err(crate::infra::persistence::sqlx_err)?;
        let segments = row
            .try_get::<i64, _>("segments")
            .map_err(crate::infra::persistence::sqlx_err)?;
        Ok((total.max(0) as u64, segments.max(0) as usize))
    }

    async fn existing_session_ids(
        &self,
        org_id: &Id,
        session_ids: &[String],
    ) -> Result<HashSet<String>> {
        if session_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let rows = sqlx::query(
            "SELECT DISTINCT session_id FROM rum_replay_events
             WHERE org_id = $1 AND session_id = ANY($2) AND has_full_snapshot = TRUE",
        )
        .bind(&org_id.0)
        .bind(session_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("session_id"))
            .collect())
    }

    async fn session_ids_in_window(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        limit: usize,
    ) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT session_id, MIN(first_event_at_micros) AS first_segment
             FROM rum_replay_events
             WHERE org_id = $1 AND first_event_at_micros >= $2 AND first_event_at_micros < $3
               AND has_full_snapshot = TRUE
             GROUP BY session_id ORDER BY first_segment DESC LIMIT $4",
        )
        .bind(&org_id.0)
        .bind(from_micros)
        .bind(to_micros)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("session_id"))
            .collect())
    }

    async fn list_expired(&self, cutoff_micros: i64, limit: usize) -> Result<Vec<RumReplayRecord>> {
        sqlx::query(
            "SELECT id, org_id, application_id, session_id, seq, object_key, bytes_uncompressed, event_count,
                    has_full_snapshot, content_hash, first_event_at_micros, created_at_micros
             FROM rum_replay_events WHERE created_at_micros < $1
             ORDER BY created_at_micros ASC LIMIT $2",
        )
        .bind(cutoff_micros)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)
        .map(|rows| rows.into_iter().map(record_from_row).collect())
    }

    async fn delete_ids(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM rum_replay_events WHERE id = ANY($1)")
            .bind(ids)
            .execute(&self.pool)
            .await
            .map_err(crate::infra::persistence::sqlx_err)?;
        Ok(())
    }
}

fn record_from_row(row: sqlx::postgres::PgRow) -> RumReplayRecord {
    RumReplayRecord {
        id: Id(row.get("id")),
        org_id: Id(row.get("org_id")),
        application_id: row.get("application_id"),
        session_id: row.get("session_id"),
        seq: row.get("seq"),
        object_key: row.get("object_key"),
        bytes_uncompressed: row.get::<i64, _>("bytes_uncompressed").max(0) as u64,
        event_count: row.get::<i32, _>("event_count").max(0) as usize,
        has_full_snapshot: row.get("has_full_snapshot"),
        content_hash: row.get("content_hash"),
        first_event_at_micros: row.get("first_event_at_micros"),
        created_at: TimestampMicros(row.get("created_at_micros")),
    }
}
