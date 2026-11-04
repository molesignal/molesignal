// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::{super::sqlx_err, like_prefix};
use crate::{
    domain::notify::{follow_up::NotifyFollowUp, repositories::NotifyFollowUpRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyFollowUpRepository {
    pool: PgPool,
}

impl PgNotifyFollowUpRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 领取后这么久还没处理完的后续步骤，看作领取它的节点已经不在了，可以重新领取。
const RECLAIM_AFTER_MICROS: i64 = 300_000_000;

const COLS: &str = "organization_id, event_id, policy_id, recipient_user_id, delay_secs,
    due_at";

fn row_to_follow_up(row: sqlx::postgres::PgRow) -> Result<NotifyFollowUp> {
    Ok(NotifyFollowUp {
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        event_id: row.try_get("event_id").map_err(sqlx_err)?,
        policy_id: Id::from_string(row.try_get::<String, _>("policy_id").map_err(sqlx_err)?),
        recipient_user_id: Id::from_string(
            row.try_get::<String, _>("recipient_user_id")
                .map_err(sqlx_err)?,
        ),
        delay_secs: row.try_get("delay_secs").map_err(sqlx_err)?,
        due_at: row
            .try_get::<TimestampMicros, _>("due_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl NotifyFollowUpRepository for PgNotifyFollowUpRepository {
    async fn schedule(&self, follow_up: NotifyFollowUp) -> Result<()> {
        sqlx::query(
            "INSERT INTO notify_follow_ups (
                 organization_id, event_id, policy_id, recipient_user_id, delay_secs,
                 due_at, status, created_at
             ) VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
             ON CONFLICT (organization_id, event_id, policy_id, recipient_user_id, delay_secs)
             DO NOTHING",
        )
        .bind(&follow_up.organization_id.0)
        .bind(&follow_up.event_id)
        .bind(&follow_up.policy_id.0)
        .bind(&follow_up.recipient_user_id.0)
        .bind(follow_up.delay_secs)
        .bind(follow_up.due_at)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn claim_due(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyFollowUp>> {
        let rows = sqlx::query(&format!(
            "UPDATE notify_follow_ups AS follow_up
                SET status = 'processing',
                    claimed_at = $2
               FROM (
                    SELECT {COLS}
                      FROM notify_follow_ups
                     WHERE organization_id = $1
                       AND (
                           (status = 'pending' AND due_at <= $2)
                           OR (status = 'processing' AND claimed_at <= $4)
                       )
                  ORDER BY due_at
                     LIMIT $3
                       FOR UPDATE SKIP LOCKED
               ) AS due
              WHERE follow_up.organization_id = due.organization_id
                AND follow_up.event_id = due.event_id
                AND follow_up.policy_id = due.policy_id
                AND follow_up.recipient_user_id = due.recipient_user_id
                AND follow_up.delay_secs = due.delay_secs
          RETURNING follow_up.organization_id, follow_up.event_id, follow_up.policy_id,
                    follow_up.recipient_user_id, follow_up.delay_secs,
                    follow_up.due_at"
        ))
        .bind(&organization_id.0)
        .bind(now)
        .bind(i64::from(limit.clamp(1, 500)))
        .bind(TimestampMicros(now.0 - RECLAIM_AFTER_MICROS))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_follow_up).collect()
    }

    async fn finish(&self, follow_up: &NotifyFollowUp) -> Result<()> {
        sqlx::query(
            "UPDATE notify_follow_ups
                SET status = 'done'
              WHERE organization_id = $1
                AND event_id = $2
                AND policy_id = $3
                AND recipient_user_id = $4
                AND delay_secs = $5
                AND status = 'processing'",
        )
        .bind(&follow_up.organization_id.0)
        .bind(&follow_up.event_id)
        .bind(&follow_up.policy_id.0)
        .bind(&follow_up.recipient_user_id.0)
        .bind(follow_up.delay_secs)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn postpone(&self, follow_up: &NotifyFollowUp, due_at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE notify_follow_ups
                SET status = 'pending',
                    due_at = $6,
                    claimed_at = NULL
              WHERE organization_id = $1
                AND event_id = $2
                AND policy_id = $3
                AND recipient_user_id = $4
                AND delay_secs = $5
                AND status = 'processing'",
        )
        .bind(&follow_up.organization_id.0)
        .bind(&follow_up.event_id)
        .bind(&follow_up.policy_id.0)
        .bind(&follow_up.recipient_user_id.0)
        .bind(follow_up.delay_secs)
        .bind(due_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn cancel(&self, organization_id: &Id, event_id: &str) -> Result<u64> {
        let cancelled = sqlx::query(
            "UPDATE notify_follow_ups
                SET status = 'cancelled'
              WHERE organization_id = $1
                AND event_id = $2
                AND status = 'pending'",
        )
        .bind(&organization_id.0)
        .bind(event_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(cancelled.rows_affected())
    }

    async fn cancel_prefix(&self, organization_id: &Id, event_id_prefix: &str) -> Result<u64> {
        let cancelled = sqlx::query(
            "UPDATE notify_follow_ups
                SET status = 'cancelled'
              WHERE organization_id = $1
                AND event_id LIKE $2 ESCAPE '\\'
                AND status = 'pending'",
        )
        .bind(&organization_id.0)
        .bind(like_prefix(event_id_prefix))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(cancelled.rows_affected())
    }
}
