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

const COLS: &str = "id, organization_id, event_type, occurred_at_micros, attributes,
    message, status, attempt, next_attempt_at_micros, claimed_at_micros,
    last_error, created_at_micros, updated_at_micros";
const QUALIFIED_COLS: &str = "event.id, event.organization_id, event.event_type,
    event.occurred_at_micros, event.attributes, event.message, event.status,
    event.attempt, event.next_attempt_at_micros, event.claimed_at_micros,
    event.last_error, event.created_at_micros, event.updated_at_micros";

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
            occurred_at: TimestampMicros(row.try_get("occurred_at_micros").map_err(sqlx_err)?),
            attributes: attributes.0,
        },
        message: message.0,
        status: NotifyEventStatus::parse(&status)?,
        attempt: row.try_get("attempt").map_err(sqlx_err)?,
        next_attempt_at: TimestampMicros(row.try_get("next_attempt_at_micros").map_err(sqlx_err)?),
        claimed_at: row
            .try_get::<Option<i64>, _>("claimed_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl NotifyEventRepository for PgNotifyEventRepository {
    async fn enqueue(&self, record: NotifyEventRecord) -> Result<NotifyEventRecord> {
        sqlx::query(
            "INSERT INTO notify_events (
                 id, organization_id, event_type, occurred_at_micros, attributes,
                 message, status, attempt, next_attempt_at_micros, claimed_at_micros,
                 last_error, created_at_micros, updated_at_micros
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, 'pending', 0, $7, NULL, NULL, $8, $9
             )
             ON CONFLICT (organization_id, id) DO NOTHING",
        )
        .bind(&record.event.id)
        .bind(&record.event.organization_id.0)
        .bind(&record.event.event_type)
        .bind(record.event.occurred_at.0)
        .bind(Json(&record.event.attributes))
        .bind(Json(&record.message))
        .bind(record.next_attempt_at.0)
        .bind(record.created_at.0)
        .bind(record.updated_at.0)
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
                    claimed_at_micros = $3,
                    updated_at_micros = $3
              WHERE organization_id = $1
                AND id = $2
                AND (
                    (status = 'pending' AND next_attempt_at_micros <= $3)
                    OR (status = 'processing' AND claimed_at_micros <= $3 - 300000000)
                )
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(now.0)
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
                    claimed_at_micros = $3,
                    updated_at_micros = $3
              WHERE organization_id = $1
                AND id = $2
                AND (
                    status <> 'processing'
                    OR claimed_at_micros <= $3 - 300000000
                )
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(now.0)
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
                    claimed_at_micros = $2,
                    updated_at_micros = $2
               FROM (
                    SELECT organization_id, id
                      FROM notify_events
                     WHERE organization_id = $1
                       AND (
                           (status = 'pending' AND next_attempt_at_micros <= $2)
                           OR (
                               status = 'processing'
                               AND claimed_at_micros <= $2 - 300000000
                           )
                       )
                  ORDER BY next_attempt_at_micros, created_at_micros, id
                     LIMIT $3
                       FOR UPDATE SKIP LOCKED
               ) AS claimable
              WHERE event.organization_id = claimable.organization_id
                AND event.id = claimable.id
          RETURNING {QUALIFIED_COLS}"
        ))
        .bind(&organization_id.0)
        .bind(now.0)
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
                    next_attempt_at_micros = $4,
                    claimed_at_micros = NULL,
                    last_error = $5,
                    updated_at_micros = $6
              WHERE organization_id = $1 AND id = $2
          RETURNING {COLS}"
        ))
        .bind(&organization_id.0)
        .bind(id)
        .bind(status.as_str())
        .bind(next_attempt_at.0)
        .bind(error)
        .bind(now.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_event(row)
    }
}
