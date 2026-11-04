// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::{super::sqlx_err, like_prefix};
use crate::{
    domain::notify::{
        delivery::{
            DeliveryClaim, DeliveryCompletion, DeliveryFilter, DeliveryStage, DeliveryStatus,
            NotifyDelivery, PageRecord,
        },
        repositories::NotifyDeliveryRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyDeliveryRepository {
    pool: PgPool,
}

impl PgNotifyDeliveryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, organization_id, event_id, policy_id, recipient_user_id,
    connector_id, endpoint_id, target_type, target_value_masked, stage, attempt,
    status, error_code, error_message, latency_ms, sent_at,
    delivered_at, acknowledged_at, escalated_at, idempotency_key,
    created_at";

const QUALIFIED_COLS: &str = "delivery.id, delivery.organization_id, delivery.event_id,
    delivery.policy_id, delivery.recipient_user_id, delivery.connector_id,
    delivery.endpoint_id, delivery.target_type, delivery.target_value_masked,
    delivery.stage, delivery.attempt, delivery.status, delivery.error_code,
    delivery.error_message, delivery.latency_ms, delivery.sent_at,
    delivery.delivered_at, delivery.acknowledged_at,
    delivery.escalated_at, delivery.idempotency_key, delivery.created_at";

fn optional_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Id>> {
    Ok(row
        .try_get::<Option<String>, _>(column)
        .map_err(sqlx_err)?
        .map(Id::from_string))
}

fn optional_timestamp(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<TimestampMicros>> {
    row.try_get(column).map_err(sqlx_err)
}

fn row_to_delivery(row: sqlx::postgres::PgRow) -> Result<NotifyDelivery> {
    let stage: String = row.try_get("stage").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(NotifyDelivery {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        event_id: row.try_get("event_id").map_err(sqlx_err)?,
        policy_id: optional_id(&row, "policy_id")?,
        recipient_user_id: optional_id(&row, "recipient_user_id")?,
        connector_id: optional_id(&row, "connector_id")?,
        endpoint_id: optional_id(&row, "endpoint_id")?,
        target_type: row.try_get("target_type").map_err(sqlx_err)?,
        target_value_masked: row.try_get("target_value_masked").map_err(sqlx_err)?,
        stage: DeliveryStage::parse(&stage)?,
        attempt: row.try_get("attempt").map_err(sqlx_err)?,
        status: DeliveryStatus::parse(&status)?,
        error_code: row.try_get("error_code").map_err(sqlx_err)?,
        error_message: row.try_get("error_message").map_err(sqlx_err)?,
        latency_ms: row.try_get("latency_ms").map_err(sqlx_err)?,
        sent_at: optional_timestamp(&row, "sent_at")?,
        delivered_at: optional_timestamp(&row, "delivered_at")?,
        acknowledged_at: optional_timestamp(&row, "acknowledged_at")?,
        escalated_at: optional_timestamp(&row, "escalated_at")?,
        idempotency_key: row.try_get("idempotency_key").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

/// 值班负担一次最多读多少条送到成员手上的告警通知。
const MAX_PAGE_RECORDS: i64 = 100_000;

#[async_trait]
impl NotifyDeliveryRepository for PgNotifyDeliveryRepository {
    async fn record_once(&self, delivery: NotifyDelivery) -> Result<NotifyDelivery> {
        sqlx::query(
            "INSERT INTO notify_deliveries (
                 id, organization_id, event_id, policy_id, recipient_user_id,
                 connector_id, endpoint_id, target_type, target_value_masked,
                 stage, attempt, status, error_code, error_message, latency_ms,
                 sent_at, delivered_at, acknowledged_at,
                 idempotency_key, created_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, $12, $13, $14, $15, $16, $17, $18, $19, $20
             )
             ON CONFLICT (idempotency_key) DO NOTHING",
        )
        .bind(&delivery.id.0)
        .bind(&delivery.organization_id.0)
        .bind(&delivery.event_id)
        .bind(delivery.policy_id.as_ref().map(|value| value.0.as_str()))
        .bind(
            delivery
                .recipient_user_id
                .as_ref()
                .map(|value| value.0.as_str()),
        )
        .bind(delivery.connector_id.as_ref().map(|value| value.0.as_str()))
        .bind(delivery.endpoint_id.as_ref().map(|value| value.0.as_str()))
        .bind(&delivery.target_type)
        .bind(&delivery.target_value_masked)
        .bind(delivery.stage.as_str())
        .bind(delivery.attempt)
        .bind(delivery.status.as_str())
        .bind(&delivery.error_code)
        .bind(&delivery.error_message)
        .bind(delivery.latency_ms)
        .bind(delivery.sent_at)
        .bind(delivery.delivered_at)
        .bind(delivery.acknowledged_at)
        .bind(&delivery.idempotency_key)
        .bind(delivery.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.find_by_idempotency_key(&delivery.organization_id, &delivery.idempotency_key)
            .await?
            .ok_or_else(|| Error::internal("notify delivery idempotency conflict across orgs"))
    }

    async fn claim(&self, delivery: NotifyDelivery) -> Result<DeliveryClaim> {
        let row = sqlx::query(&format!(
            "INSERT INTO notify_deliveries (
                 id, organization_id, event_id, policy_id, recipient_user_id,
                 connector_id, endpoint_id, target_type, target_value_masked,
                 stage, attempt, status, error_code, error_message, latency_ms,
                 sent_at, delivered_at, acknowledged_at,
                 idempotency_key, created_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, 'sending', NULL, NULL, NULL, $12, NULL, NULL, $13, $14
             )
             ON CONFLICT (idempotency_key) DO UPDATE SET
                 attempt = notify_deliveries.attempt + 1,
                 status = 'sending',
                 error_code = NULL,
                 error_message = NULL,
                 latency_ms = NULL,
                 sent_at = EXCLUDED.sent_at,
                 delivered_at = NULL,
                 acknowledged_at = NULL
             WHERE notify_deliveries.organization_id = EXCLUDED.organization_id
               AND notify_deliveries.status IN ('pending', 'failed', 'skipped')
             RETURNING {COLS}"
        ))
        .bind(&delivery.id.0)
        .bind(&delivery.organization_id.0)
        .bind(&delivery.event_id)
        .bind(delivery.policy_id.as_ref().map(|value| value.0.as_str()))
        .bind(
            delivery
                .recipient_user_id
                .as_ref()
                .map(|value| value.0.as_str()),
        )
        .bind(delivery.connector_id.as_ref().map(|value| value.0.as_str()))
        .bind(delivery.endpoint_id.as_ref().map(|value| value.0.as_str()))
        .bind(&delivery.target_type)
        .bind(&delivery.target_value_masked)
        .bind(delivery.stage.as_str())
        .bind(delivery.attempt.max(1))
        .bind(delivery.sent_at)
        .bind(&delivery.idempotency_key)
        .bind(delivery.created_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if let Some(row) = row {
            return Ok(DeliveryClaim {
                delivery: row_to_delivery(row)?,
                acquired: true,
            });
        }

        let existing = self
            .find_by_idempotency_key(&delivery.organization_id, &delivery.idempotency_key)
            .await?
            .ok_or_else(|| Error::internal("notify delivery idempotency conflict across orgs"))?;

        Ok(DeliveryClaim {
            delivery: existing,
            acquired: false,
        })
    }

    async fn complete(
        &self,
        organization_id: &Id,
        id: &Id,
        completion: DeliveryCompletion,
    ) -> Result<NotifyDelivery> {
        if !matches!(
            completion.status,
            DeliveryStatus::Success
                | DeliveryStatus::Failed
                | DeliveryStatus::Skipped
                | DeliveryStatus::Queued
        ) {
            return Err(Error::invalid(
                "notify delivery completion must be success, failed, skipped, or queued",
            ));
        }

        // 排在电话线路上的投递，在合并的那一通电话拨出后完成。

        let updated = sqlx::query(
            "UPDATE notify_deliveries
                SET status = $3,
                    error_code = $4,
                    error_message = $5,
                    latency_ms = $6,
                    delivered_at = $7
              WHERE organization_id = $1
                AND id = $2
                AND status IN ('sending', 'queued')",
        )
        .bind(&organization_id.0)
        .bind(&id.0)
        .bind(completion.status.as_str())
        .bind(completion.error_code)
        .bind(completion.error_message)
        .bind(completion.latency_ms)
        .bind(completion.delivered_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if updated.rows_affected() == 0 {
            return Err(Error::conflict("notify delivery is no longer sending"));
        }

        self.get(organization_id, id).await
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyDelivery> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_deliveries
              WHERE organization_id = $1 AND id = $2"
        ))
        .bind(&organization_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_delivery(row)
    }

    async fn find_by_idempotency_key(
        &self,
        organization_id: &Id,
        idempotency_key: &str,
    ) -> Result<Option<NotifyDelivery>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_deliveries
              WHERE organization_id = $1 AND idempotency_key = $2"
        ))
        .bind(&organization_id.0)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to_delivery).transpose()
    }

    async fn list(
        &self,
        organization_id: &Id,
        filter: &DeliveryFilter,
    ) -> Result<Vec<NotifyDelivery>> {
        let policy_id = filter.policy_id.as_ref().map(|value| value.0.clone());

        let recipient_user_id = filter
            .recipient_user_id
            .as_ref()
            .map(|value| value.0.clone());

        let connector_id = filter.connector_id.as_ref().map(|value| value.0.clone());
        let status = filter.status.map(DeliveryStatus::as_str);
        let stage = filter.stage.map(DeliveryStage::as_str);

        let limit = i64::from(if filter.limit == 0 {
            100
        } else {
            filter.limit.min(500)
        });

        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_deliveries
              WHERE organization_id = $1
                AND ($2::TEXT IS NULL OR event_id = $2)
                AND ($3::TEXT IS NULL OR policy_id = $3)
                AND ($4::TEXT IS NULL OR recipient_user_id = $4)
                AND ($5::TEXT IS NULL OR connector_id = $5)
                AND ($6::TEXT IS NULL OR status = $6)
                AND ($7::TEXT IS NULL OR stage = $7)
                AND ($8::TIMESTAMPTZ IS NULL OR created_at >= $8)
                AND ($9::TIMESTAMPTZ IS NULL OR created_at <= $9)
           ORDER BY created_at DESC, id
              LIMIT $10"
        ))
        .bind(&organization_id.0)
        .bind(&filter.event_id)
        .bind(policy_id)
        .bind(recipient_user_id)
        .bind(connector_id)
        .bind(status)
        .bind(stage)
        .bind(filter.from)
        .bind(filter.to)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_delivery).collect()
    }

    async fn acknowledge_event(
        &self,
        organization_id: &Id,
        event_id: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64> {
        let updated = sqlx::query(
            "UPDATE notify_deliveries
                SET status = 'acknowledged',
                    acknowledged_at = $3
              WHERE organization_id = $1
                AND event_id = $2
                AND status IN ('success', 'acknowledged', 'queued')",
        )
        .bind(&organization_id.0)
        .bind(event_id)
        .bind(acknowledged_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(updated.rows_affected())
    }

    async fn acknowledge_event_prefix(
        &self,
        organization_id: &Id,
        event_id_prefix: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64> {
        let updated = sqlx::query(
            "UPDATE notify_deliveries
                SET status = 'acknowledged',
                    acknowledged_at = $3
              WHERE organization_id = $1
                AND event_id LIKE $2 ESCAPE '\\'
                AND status IN ('success', 'acknowledged', 'queued')",
        )
        .bind(&organization_id.0)
        .bind(like_prefix(event_id_prefix))
        .bind(acknowledged_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(updated.rows_affected())
    }

    async fn list_for_alert(
        &self,
        organization_id: &Id,
        incident_id: &Id,
        limit: u32,
    ) -> Result<Vec<NotifyDelivery>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM notify_deliveries
              WHERE organization_id = $1
                AND (event_id = ANY($2) OR event_id LIKE $3 ESCAPE '\\')
           ORDER BY created_at, id
              LIMIT $4"
        ))
        .bind(&organization_id.0)
        .bind(vec![
            format!("alert.triggered:{incident_id}"),
            format!("alert.acknowledged:{incident_id}"),
            format!("alert.resolved:{incident_id}"),
        ])
        .bind(like_prefix(&format!("alert.escalated:{incident_id}:")))
        .bind(i64::from(limit.clamp(1, 1000)))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_delivery).collect()
    }

    async fn pages_between(
        &self,
        organization_id: &Id,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Result<Vec<PageRecord>> {
        let rows = sqlx::query(
            "SELECT recipient_user_id, event_id, MIN(created_at) AS at
               FROM notify_deliveries
              WHERE organization_id = $1
                AND recipient_user_id IS NOT NULL
                AND created_at >= $2 AND created_at < $3
                AND status IN ('success', 'acknowledged')
                AND (event_id LIKE 'alert.triggered:%' OR event_id LIKE 'alert.escalated:%')
           GROUP BY recipient_user_id, event_id
           ORDER BY at
              LIMIT $4",
        )
        .bind(&organization_id.0)
        .bind(from)
        .bind(to)
        .bind(MAX_PAGE_RECORDS)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(PageRecord {
                    user_id: Id(row.try_get("recipient_user_id").map_err(sqlx_err)?),
                    event_id: row.try_get("event_id").map_err(sqlx_err)?,
                    at: row.try_get::<TimestampMicros, _>("at").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn list_due_ack(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyDelivery>> {
        let rows = sqlx::query(&format!(
            "SELECT {QUALIFIED_COLS}
               FROM notify_deliveries AS delivery
               JOIN notify_policies AS policy
                 ON policy.organization_id = delivery.organization_id
                AND policy.id = delivery.policy_id
              WHERE delivery.organization_id = $1
                AND delivery.status = 'success'
                AND delivery.acknowledged_at IS NULL
                AND delivery.escalated_at IS NULL
                AND delivery.delivered_at IS NOT NULL
                AND policy.ack_timeout_seconds IS NOT NULL
                AND policy.escalation_config IS NOT NULL
                AND delivery.delivered_at
                    + make_interval(secs => policy.ack_timeout_seconds) <= $2
           ORDER BY delivery.delivered_at, delivery.id
              LIMIT $3"
        ))
        .bind(&organization_id.0)
        .bind(now)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_delivery).collect()
    }

    async fn mark_escalated(
        &self,
        organization_id: &Id,
        id: &Id,
        escalated_at: TimestampMicros,
    ) -> Result<NotifyDelivery> {
        let updated = sqlx::query(
            "UPDATE notify_deliveries
                SET escalated_at = $3
              WHERE organization_id = $1
                AND id = $2
                AND escalated_at IS NULL",
        )
        .bind(&organization_id.0)
        .bind(&id.0)
        .bind(escalated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if updated.rows_affected() == 0 {
            return Err(Error::conflict(
                "notify delivery was already escalated or does not exist",
            ));
        }

        self.get(organization_id, id).await
    }
}
