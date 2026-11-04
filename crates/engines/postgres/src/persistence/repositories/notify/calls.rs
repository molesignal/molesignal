// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        call::{CallLine, QueuedCall},
        connector::{NotifyMessage, NotifyTarget},
        repositories::NotifyCallRepository,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyCallRepository {
    pool: PgPool,
}

impl PgNotifyCallRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn window_secs(line: &CallLine) -> i32 {
    i32::try_from(line.window_secs).unwrap_or(i32::MAX)
}

fn row_to_queued(row: sqlx::postgres::PgRow) -> Result<QueuedCall> {
    let target: Json<NotifyTarget> = row.try_get("target").map_err(sqlx_err)?;
    let message: Json<NotifyMessage> = row.try_get("message").map_err(sqlx_err)?;

    Ok(QueuedCall {
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        delivery_id: Id::from_string(row.try_get::<String, _>("delivery_id").map_err(sqlx_err)?),
        connector_id: Id::from_string(row.try_get::<String, _>("connector_id").map_err(sqlx_err)?),
        line_key: row.try_get("line_key").map_err(sqlx_err)?,
        target: target.0,
        message: message.0,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl NotifyCallRepository for PgNotifyCallRepository {
    async fn take_line(
        &self,
        line: &CallLine,
        now: TimestampMicros,
    ) -> Result<Option<TimestampMicros>> {
        // 新线路直接占下；已有的线路只在上一通之后过了窗口才占得下。`previous` 读的是语句开始时的值。
        let row = sqlx::query(
            "WITH previous AS (
                 SELECT last_call_at
                   FROM notify_call_lines
                  WHERE organization_id = $1
                    AND connector_id = $2
                    AND line_key = $3
             )
             INSERT INTO notify_call_lines (
                 organization_id, connector_id, line_key, window_secs, last_call_at
             ) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (organization_id, connector_id, line_key) DO UPDATE SET
                 window_secs = EXCLUDED.window_secs,
                 last_call_at = EXCLUDED.last_call_at
             WHERE notify_call_lines.last_call_at
                   + make_interval(secs => notify_call_lines.window_secs)
                   <= EXCLUDED.last_call_at
             RETURNING (SELECT last_call_at FROM previous) AS previous",
        )
        .bind(&line.organization_id.0)
        .bind(&line.connector_id.0)
        .bind(&line.key)
        .bind(window_secs(line))
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let previous: Option<i64> = row.try_get("previous").map_err(sqlx_err)?;

        Ok(Some(TimestampMicros(previous.unwrap_or_default())))
    }

    async fn release_line(
        &self,
        line: &CallLine,
        taken_at: TimestampMicros,
        previous: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE notify_call_lines
                SET last_call_at = $5
              WHERE organization_id = $1
                AND connector_id = $2
                AND line_key = $3
                AND last_call_at = $4",
        )
        .bind(&line.organization_id.0)
        .bind(&line.connector_id.0)
        .bind(&line.key)
        .bind(taken_at)
        .bind(previous)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn enqueue(&self, call: QueuedCall) -> Result<()> {
        sqlx::query(
            "INSERT INTO notify_call_queue (
                 organization_id, delivery_id, connector_id, line_key, target, message,
                 created_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (organization_id, delivery_id) DO NOTHING",
        )
        .bind(&call.organization_id.0)
        .bind(&call.delivery_id.0)
        .bind(&call.connector_id.0)
        .bind(&call.line_key)
        .bind(Json(&call.target))
        .bind(Json(&call.message))
        .bind(call.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn due_lines(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<CallLine>> {
        let rows = sqlx::query(
            "SELECT line.organization_id, line.connector_id, line.line_key, line.window_secs
               FROM notify_call_lines AS line
              WHERE line.organization_id = $1
                AND line.last_call_at + make_interval(secs => line.window_secs) <= $2
                AND EXISTS (
                    SELECT 1
                      FROM notify_call_queue AS queued
                     WHERE queued.organization_id = line.organization_id
                       AND queued.connector_id = line.connector_id
                       AND queued.line_key = line.line_key
                )
           ORDER BY line.last_call_at
              LIMIT $3",
        )
        .bind(&organization_id.0)
        .bind(now)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let window_secs: i32 = row.try_get("window_secs").map_err(sqlx_err)?;

                Ok(CallLine {
                    organization_id: Id::from_string(
                        row.try_get::<String, _>("organization_id")
                            .map_err(sqlx_err)?,
                    ),
                    connector_id: Id::from_string(
                        row.try_get::<String, _>("connector_id").map_err(sqlx_err)?,
                    ),
                    key: row.try_get("line_key").map_err(sqlx_err)?,
                    window_secs: u32::try_from(window_secs).unwrap_or_default(),
                })
            })
            .collect()
    }

    async fn drain(&self, line: &CallLine) -> Result<Vec<QueuedCall>> {
        let rows = sqlx::query(
            "DELETE FROM notify_call_queue
              WHERE organization_id = $1
                AND connector_id = $2
                AND line_key = $3
          RETURNING organization_id, delivery_id, connector_id, line_key, target, message,
                    created_at",
        )
        .bind(&line.organization_id.0)
        .bind(&line.connector_id.0)
        .bind(&line.key)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut calls = rows
            .into_iter()
            .map(row_to_queued)
            .collect::<Result<Vec<_>>>()?;

        calls.sort_by_key(|call| (call.created_at.0, call.delivery_id.0.clone()));
        Ok(calls)
    }
}
