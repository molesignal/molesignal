// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        connector::NotifyMessage,
        event::{NotifyEventClaim, NotifyEventRecord, NotifyEventStatus},
        policy::NotifyEvent,
        repositories::NotifyEventRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyEventRepository {
    pool: PgPool,
}

impl PgNotifyEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, organization_id, event_type, occurred_at, attributes,
    message, status, attempt, next_attempt_at, claimed_at,
    last_error, created_at, updated_at";

const QUALIFIED_COLS: &str = "event.id, event.organization_id, event.event_type,
    event.occurred_at, event.attributes, event.message, event.status,
    event.attempt, event.next_attempt_at, event.claimed_at,
    event.last_error, event.created_at, event.updated_at";

fn row_to_event(row: sqlx::postgres::PgRow) -> Result<NotifyEventRecord> {
    let attributes: Json<serde_json::Value> = row.try_get("attributes").map_err(sqlx_err)?;
    let message: Json<NotifyMessage> = row.try_get("message").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(NotifyEventRecord {
        event: NotifyEvent {
            id: row.try_get("id").map_err(sqlx_err)?,
            event_type: row.try_get("event_type").map_err(sqlx_err)?,
            organization_id: Id::from_string(
                row.try_get::<String, _>("organization_id")
                    .map_err(sqlx_err)?,
            ),
            occurred_at: row
                .try_get::<TimestampMicros, _>("occurred_at")
                .map_err(sqlx_err)?,
            attributes: attributes.0,
        },
        message: message.0,
        status: NotifyEventStatus::parse(&status)?,
        attempt: row.try_get("attempt").map_err(sqlx_err)?,
        next_attempt_at: row
            .try_get::<TimestampMicros, _>("next_attempt_at")
            .map_err(sqlx_err)?,
        claimed_at: row
            .try_get::<Option<TimestampMicros>, _>("claimed_at")
            .map_err(sqlx_err)?,
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl NotifyEventRepository for PgNotifyEventRepository {
    async fn enqueue(&self, record: NotifyEventRecord) -> Result<NotifyEventRecord> {
        sqlx::query(
            "INSERT INTO notify_events (
                 id, organization_id, event_type, occurred_at, attributes,
                 message, status, attempt, next_attempt_at, claimed_at,
                 last_error, created_at, updated_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, 'pending', 0, $7, NULL, NULL, $8, $9
             )
             ON CONFLICT (organization_id, id) DO NOTHING",
        )
        .bind(&record.event.id)
        .bind(&record.event.organization_id.0)
        .bind(&record.event.event_type)
        .bind(record.event.occurred_at)
        .bind(Json(&record.event.attributes))
        .bind(Json(&record.message))
        .bind(record.next_attempt_at)
        .bind(record.created_at)
        .bind(record.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.get(&record.event.organization_id, &record.event.id)
            .await
    }

    async fn get(&self, organization_id: &Id, id: &str) -> Result<NotifyEventRecord> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_events
              WHERE organization_id = $1 AND id = $2"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_event(row)
    }

    async fn claim(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim> {
        let row = sqlx::query(&format!(
            "UPDATE notify_events
                SET status = 'processing',
                    attempt = attempt + 1,
                    claimed_at = $3,
                    updated_at = $3
              WHERE organization_id = $1
                AND id = $2
                AND (
                    (status = 'pending' AND next_attempt_at <= $3)
                    OR (status = 'processing' AND claimed_at <= $3 - INTERVAL '5 minutes')
                )
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if let Some(row) = row {
            return Ok(NotifyEventClaim {
                record: row_to_event(row)?,
                acquired: true,
            });
        }

        Ok(NotifyEventClaim {
            record: self.get(organization_id, id).await?,
            acquired: false,
        })
    }

    async fn claim_retry(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim> {
        let row = sqlx::query(&format!(
            "UPDATE notify_events
                SET status = 'processing',
                    attempt = attempt + 1,
                    claimed_at = $3,
                    updated_at = $3
              WHERE organization_id = $1
                AND id = $2
                AND (
                    status <> 'processing'
                    OR claimed_at <= $3 - INTERVAL '5 minutes'
                )
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if let Some(row) = row {
            return Ok(NotifyEventClaim {
                record: row_to_event(row)?,
                acquired: true,
            });
        }

        Ok(NotifyEventClaim {
            record: self.get(organization_id, id).await?,
            acquired: false,
        })
    }

    async fn claim_pending(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyEventRecord>> {
        let rows = sqlx::query(&format!(
            "UPDATE notify_events AS event
                SET status = 'processing',
                    attempt = event.attempt + 1,
                    claimed_at = $2,
                    updated_at = $2
               FROM (
                    SELECT organization_id, id
                      FROM notify_events
                     WHERE organization_id = $1
                       AND (
                           (status = 'pending' AND next_attempt_at <= $2)
                           OR (
                               status = 'processing'
                               AND claimed_at <= $2 - INTERVAL '5 minutes'
                           )
                       )
                  ORDER BY next_attempt_at, created_at, id
                     LIMIT $3
                       FOR UPDATE SKIP LOCKED
               ) AS claimable
              WHERE event.organization_id = claimable.organization_id
                AND event.id = claimable.id
          RETURNING {QUALIFIED_COLS}"
        ))
        .bind(&organization_id.0)
        .bind(now)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_event).collect()
    }

    async fn finish(
        &self,
        organization_id: &Id,
        id: &str,
        status: NotifyEventStatus,
        next_attempt_at: TimestampMicros,
        error: Option<String>,
        now: TimestampMicros,
    ) -> Result<NotifyEventRecord> {
        if status == NotifyEventStatus::Processing {
            return Err(Error::invalid(
                "notify event finish status cannot be processing",
            ));
        }

        let row = sqlx::query(&format!(
            "UPDATE notify_events
                SET status = $3,
                    next_attempt_at = $4,
                    claimed_at = NULL,
                    last_error = $5,
                    updated_at = $6
              WHERE organization_id = $1 AND id = $2
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(status.as_str())
        .bind(next_attempt_at)
        .bind(error)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_event(row)
    }

    async fn postpone(
        &self,
        organization_id: &Id,
        id: &str,
        next_attempt_at: TimestampMicros,
        note: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventRecord> {
        let row = sqlx::query(&format!(
            "UPDATE notify_events
                SET status = 'pending',
                    attempt = GREATEST(attempt - 1, 0),
                    next_attempt_at = $3,
                    claimed_at = NULL,
                    last_error = $4,
                    updated_at = $5
              WHERE organization_id = $1 AND id = $2
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(next_attempt_at)
        .bind(note)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_event(row)
    }
}
