// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::{PgStatusPageRepository, subscriptions::optional_timestamp};
use crate::{
    domain::status_page::{
        StatusPageDeliveryPage, StatusPageDeliveryRepository, StatusPageDeliveryStatus,
        StatusPageNotificationDelivery, StatusPageSubscriberChannel,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DELIVERY_COLS: &str = "delivery.id, delivery.org_id, delivery.status_page_id,
    delivery.subscriber_id, delivery.event_key, delivery.payload, delivery.status,
    delivery.attempts, delivery.next_attempt_at_micros, delivery.last_error,
    delivery.delivered_at_micros, delivery.created_at_micros, delivery.updated_at_micros,
    subscriber.channel, subscriber.target_ciphertext, subscriber.target_nonce";
const CLAIMED_DELIVERY_COLS: &str = "claimed.id, claimed.org_id, claimed.status_page_id,
    claimed.subscriber_id, claimed.event_key, claimed.payload, claimed.status,
    claimed.attempts, claimed.next_attempt_at_micros, claimed.last_error,
    claimed.delivered_at_micros, claimed.created_at_micros, claimed.updated_at_micros,
    subscriber.channel, subscriber.target_ciphertext, subscriber.target_nonce";

#[async_trait]
impl StatusPageDeliveryRepository for PgStatusPageRepository {
    async fn list_notification_deliveries(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
        per_page: u32,
    ) -> Result<StatusPageDeliveryPage> {
        let per_page = per_page.clamp(1, 100);
        let offset = u64::from(page.saturating_sub(1)).saturating_mul(u64::from(per_page));
        let total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM status_page_notification_deliveries
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        let sql = format!(
            "SELECT {DELIVERY_COLS}
             FROM status_page_notification_deliveries delivery
             JOIN status_page_subscribers subscriber
               ON subscriber.org_id = delivery.org_id
              AND subscriber.status_page_id = delivery.status_page_id
              AND subscriber.id = delivery.subscriber_id
             WHERE delivery.org_id = $1 AND delivery.status_page_id = $2
             ORDER BY delivery.created_at_micros DESC, delivery.id DESC
             LIMIT $3 OFFSET $4"
        );
        let items = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(i64::from(per_page))
            .bind(i64::try_from(offset).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(|row| self.row_to_delivery(row))
            .collect::<Result<Vec<_>>>()?;
        Ok(StatusPageDeliveryPage {
            items,
            page,
            per_page,
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }

    async fn claim_notification_deliveries(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPageNotificationDelivery>> {
        let stale_before = now.0.saturating_sub(5 * 60 * 1_000_000);
        let sql = format!(
            "WITH candidates AS (
                SELECT delivery.id
                FROM status_page_notification_deliveries delivery
                JOIN status_page_subscribers subscriber
                  ON subscriber.org_id = delivery.org_id
                 AND subscriber.status_page_id = delivery.status_page_id
                 AND subscriber.id = delivery.subscriber_id
                JOIN status_pages page
                  ON page.org_id = delivery.org_id AND page.id = delivery.status_page_id
                WHERE subscriber.status = 'active' AND page.lifecycle = 'active'
                  AND ((delivery.status = 'pending' AND delivery.next_attempt_at_micros <= $1)
                       OR (delivery.status = 'processing' AND delivery.claimed_at_micros < $2))
                ORDER BY delivery.next_attempt_at_micros, delivery.created_at_micros
                LIMIT $3 FOR UPDATE OF delivery SKIP LOCKED
             ), claimed AS (
                UPDATE status_page_notification_deliveries delivery
                SET status = 'processing', attempts = delivery.attempts + 1,
                    claimed_at_micros = $1, updated_at_micros = $1
                FROM candidates WHERE delivery.id = candidates.id
                RETURNING delivery.*
             )
             SELECT {CLAIMED_DELIVERY_COLS}
             FROM claimed
             JOIN status_page_subscribers subscriber
               ON subscriber.org_id = claimed.org_id
              AND subscriber.status_page_id = claimed.status_page_id
              AND subscriber.id = claimed.subscriber_id
             ORDER BY claimed.created_at_micros"
        );
        sqlx::query(&sql)
            .bind(now.0)
            .bind(stale_before)
            .bind(i64::from(limit.min(500)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(|row| self.row_to_delivery(row))
            .collect()
    }

    async fn complete_notification_delivery(
        &self,
        delivery_id: &Id,
        status: StatusPageDeliveryStatus,
        next_attempt_at: TimestampMicros,
        last_error: Option<String>,
        updated_at: TimestampMicros,
    ) -> Result<()> {
        if status == StatusPageDeliveryStatus::Processing {
            return Err(Error::invalid(
                "a completed delivery cannot remain processing",
            ));
        }
        let result = sqlx::query(
            "UPDATE status_page_notification_deliveries SET
                status = $2, next_attempt_at_micros = $3, claimed_at_micros = NULL,
                delivered_at_micros = CASE WHEN $2 = 'delivered' THEN $5
                                           ELSE delivered_at_micros END,
                last_error = $4, updated_at_micros = $5
             WHERE id = $1 AND status = 'processing'",
        )
        .bind(&delivery_id.0)
        .bind(status.as_str())
        .bind(next_attempt_at.0)
        .bind(last_error)
        .bind(updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() != 1 {
            return Err(Error::conflict(
                "status-page notification delivery claim was lost",
            ));
        }
        Ok(())
    }

    async fn purge_terminal_notification_deliveries(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u64> {
        let result = sqlx::query(
            "WITH expired AS (
                SELECT delivery.id
                FROM status_page_notification_deliveries delivery
                JOIN status_pages page
                  ON page.org_id = delivery.org_id AND page.id = delivery.status_page_id
                WHERE delivery.status IN ('delivered', 'failed')
                  AND delivery.updated_at_micros <=
                      $1 - page.delivery_retention_days::BIGINT * 86400000000
                ORDER BY delivery.updated_at_micros, delivery.id
                LIMIT $2 FOR UPDATE OF delivery SKIP LOCKED
             )
             DELETE FROM status_page_notification_deliveries delivery
             USING expired WHERE delivery.id = expired.id",
        )
        .bind(now.0)
        .bind(i64::from(limit.min(1000)))
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        Ok(result.rows_affected())
    }
}

impl PgStatusPageRepository {
    fn row_to_delivery(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<StatusPageNotificationDelivery> {
        let channel: String = row.try_get("channel").map_err(super::sqlx_err)?;
        let status: String = row.try_get("status").map_err(super::sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("target_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("target_nonce").map_err(super::sqlx_err)?;
        Ok(StatusPageNotificationDelivery {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            subscriber_id: Id(row.try_get("subscriber_id").map_err(super::sqlx_err)?),
            channel: StatusPageSubscriberChannel::parse(&channel)
                .ok_or_else(|| Error::internal(format!("unknown delivery channel: {channel}")))?,
            target: self.open_target(&nonce, &ciphertext)?,
            event_key: row.try_get("event_key").map_err(super::sqlx_err)?,
            payload: row.try_get("payload").map_err(super::sqlx_err)?,
            status: StatusPageDeliveryStatus::parse(&status)
                .ok_or_else(|| Error::internal(format!("unknown delivery status: {status}")))?,
            attempts: row.try_get("attempts").map_err(super::sqlx_err)?,
            next_attempt_at: TimestampMicros(
                row.try_get("next_attempt_at_micros")
                    .map_err(super::sqlx_err)?,
            ),
            last_error: row.try_get("last_error").map_err(super::sqlx_err)?,
            delivered_at: optional_timestamp(&row, "delivered_at_micros")?,
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
            updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        })
    }
}
