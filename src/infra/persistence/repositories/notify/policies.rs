// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyFallbackConfig, NotifyPolicy},
        preference::NotifyCategory,
        repositories::NotifyPolicyRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyPolicyRepository {
    pool: PgPool,
}

impl PgNotifyPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, organization_id, name, event_type, category, matchers,
    recipient_resolver, resolver_config, delivery_mode, template_id,
    delivery_config, fallback_config, ack_timeout_seconds, escalation_config, enabled, priority,
    created_at_micros, updated_at_micros";

fn row_to_policy(row: sqlx::postgres::PgRow) -> Result<NotifyPolicy> {
    let category: String = row.try_get("category").map_err(sqlx_err)?;
    let delivery_mode: String = row.try_get("delivery_mode").map_err(sqlx_err)?;
    let matchers: Json<serde_json::Value> = row.try_get("matchers").map_err(sqlx_err)?;
    let resolver_config: Json<serde_json::Value> =
        row.try_get("resolver_config").map_err(sqlx_err)?;
    let fallback_config: Json<NotifyFallbackConfig> =
        row.try_get("fallback_config").map_err(sqlx_err)?;
    let delivery_config: Json<NotifyDeliveryConfig> =
        row.try_get("delivery_config").map_err(sqlx_err)?;
    let escalation_config: Option<Json<serde_json::Value>> =
        row.try_get("escalation_config").map_err(sqlx_err)?;
    Ok(NotifyPolicy {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        name: row.try_get("name").map_err(sqlx_err)?,
        event_type: row.try_get("event_type").map_err(sqlx_err)?,
        category: NotifyCategory::parse(&category)?,
        matchers: matchers.0,
        recipient_resolver: row.try_get("recipient_resolver").map_err(sqlx_err)?,
        resolver_config: resolver_config.0,
        delivery_mode: NotifyDeliveryMode::parse(&delivery_mode)?,
        delivery_config: delivery_config.0,
        template_id: row
            .try_get::<Option<String>, _>("template_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        fallback_config: fallback_config.0,
        ack_timeout_seconds: row.try_get("ack_timeout_seconds").map_err(sqlx_err)?,
        escalation_config: escalation_config.map(|value| value.0),
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        priority: row.try_get("priority").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl NotifyPolicyRepository for PgNotifyPolicyRepository {
    async fn create(&self, policy: NotifyPolicy) -> Result<NotifyPolicy> {
        sqlx::query(
            "INSERT INTO notify_policies (
                 id, organization_id, name, event_type, category, matchers,
                 recipient_resolver, resolver_config, delivery_mode, template_id,
                 delivery_config, fallback_config, ack_timeout_seconds,
                 escalation_config, enabled, priority, created_at_micros,
                 updated_at_micros
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, $12, $13, $14, $15, $16, $17, $18
             )",
        )
        .bind(&policy.id.0)
        .bind(&policy.organization_id.0)
        .bind(&policy.name)
        .bind(&policy.event_type)
        .bind(policy.category.as_str())
        .bind(Json(&policy.matchers))
        .bind(&policy.recipient_resolver)
        .bind(Json(&policy.resolver_config))
        .bind(policy.delivery_mode.as_str())
        .bind(policy.template_id.as_ref().map(|value| value.0.as_str()))
        .bind(Json(&policy.delivery_config))
        .bind(Json(&policy.fallback_config))
        .bind(policy.ack_timeout_seconds)
        .bind(policy.escalation_config.as_ref().map(Json))
        .bind(policy.enabled)
        .bind(policy.priority)
        .bind(policy.created_at.0)
        .bind(policy.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(policy)
    }

    async fn update(&self, policy: NotifyPolicy) -> Result<NotifyPolicy> {
        let updated = sqlx::query(
            "UPDATE notify_policies
                SET name = $3,
                    event_type = $4,
                    category = $5,
                    matchers = $6,
                    recipient_resolver = $7,
                    resolver_config = $8,
                    delivery_mode = $9,
                    template_id = $10,
                    delivery_config = $11,
                    fallback_config = $12,
                    ack_timeout_seconds = $13,
                    escalation_config = $14,
                    enabled = $15,
                    priority = $16,
                    updated_at_micros = $17
              WHERE organization_id = $1 AND id = $2",
        )
        .bind(&policy.organization_id.0)
        .bind(&policy.id.0)
        .bind(&policy.name)
        .bind(&policy.event_type)
        .bind(policy.category.as_str())
        .bind(Json(&policy.matchers))
        .bind(&policy.recipient_resolver)
        .bind(Json(&policy.resolver_config))
        .bind(policy.delivery_mode.as_str())
        .bind(policy.template_id.as_ref().map(|value| value.0.as_str()))
        .bind(Json(&policy.delivery_config))
        .bind(Json(&policy.fallback_config))
        .bind(policy.ack_timeout_seconds)
        .bind(policy.escalation_config.as_ref().map(Json))
        .bind(policy.enabled)
        .bind(policy.priority)
        .bind(policy.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if updated.rows_affected() == 0 {
            return Err(Error::not_found("notify policy"));
        }
        Ok(policy)
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyPolicy> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_policies
              WHERE organization_id = $1 AND id = $2"
        ))
        .bind(&organization_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_policy(row)
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyPolicy>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_policies
              WHERE organization_id = $1
           ORDER BY priority, name, id"
        ))
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_policy).collect()
    }

    async fn list_enabled_for_event(
        &self,
        organization_id: &Id,
        event_type: &str,
    ) -> Result<Vec<NotifyPolicy>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_policies
              WHERE organization_id = $1
                AND event_type = $2
                AND enabled = TRUE
           ORDER BY priority, id"
        ))
        .bind(&organization_id.0)
        .bind(event_type)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_policy).collect()
    }

    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()> {
        let deleted =
            sqlx::query("DELETE FROM notify_policies WHERE organization_id = $1 AND id = $2")
                .bind(&organization_id.0)
                .bind(&id.0)
                .execute(&self.pool)
                .await
                .map_err(sqlx_err)?;
        if deleted.rows_affected() == 0 {
            return Err(Error::not_found("notify policy"));
        }
        Ok(())
    }
}
