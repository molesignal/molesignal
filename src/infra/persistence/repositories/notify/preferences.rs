// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        preference::{NotifyCategory, UserNotifyPreference, UserNotifyPreferenceStep},
        repositories::UserNotifyPreferenceRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgUserNotifyPreferenceRepository {
    pool: PgPool,
}

impl PgUserNotifyPreferenceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn list_inner(
        &self,
        organization_id: &Id,
        user_id: Option<&Id>,
    ) -> Result<Vec<UserNotifyPreference>> {
        let rows = sqlx::query(
            "SELECT id, organization_id, user_id, category, enabled, quiet_hours,
                    allow_critical_bypass, created_at_micros, updated_at_micros
               FROM user_notify_preferences
              WHERE organization_id = $1
                AND ($2::TEXT IS NULL OR user_id = $2)
           ORDER BY user_id, category, id",
        )
        .bind(&organization_id.0)
        .bind(user_id.map(|value| value.0.as_str()))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let preference_ids = rows
            .iter()
            .map(|row| row.try_get::<String, _>("id").map_err(sqlx_err))
            .collect::<Result<Vec<_>>>()?;
        let step_rows = if preference_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query(
                "SELECT id, preference_id, endpoint_id, step_order, created_at_micros
                   FROM user_notify_preference_steps
                  WHERE preference_id = ANY($1::TEXT[])
               ORDER BY preference_id, step_order, id",
            )
            .bind(&preference_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        };
        let mut steps: BTreeMap<String, Vec<UserNotifyPreferenceStep>> = BTreeMap::new();
        for row in step_rows {
            let preference_id: String = row.try_get("preference_id").map_err(sqlx_err)?;
            steps
                .entry(preference_id.clone())
                .or_default()
                .push(UserNotifyPreferenceStep {
                    id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                    preference_id: Id::from_string(preference_id),
                    endpoint_id: Id::from_string(
                        row.try_get::<String, _>("endpoint_id").map_err(sqlx_err)?,
                    ),
                    step_order: row.try_get("step_order").map_err(sqlx_err)?,
                    created_at: TimestampMicros(
                        row.try_get("created_at_micros").map_err(sqlx_err)?,
                    ),
                });
        }
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id").map_err(sqlx_err)?;
                let category: String = row.try_get("category").map_err(sqlx_err)?;
                let quiet_hours: Option<Json<serde_json::Value>> =
                    row.try_get("quiet_hours").map_err(sqlx_err)?;
                Ok(UserNotifyPreference {
                    id: Id::from_string(id.clone()),
                    organization_id: Id::from_string(
                        row.try_get::<String, _>("organization_id")
                            .map_err(sqlx_err)?,
                    ),
                    user_id: Id::from_string(
                        row.try_get::<String, _>("user_id").map_err(sqlx_err)?,
                    ),
                    category: NotifyCategory::parse(&category)?,
                    enabled: row.try_get("enabled").map_err(sqlx_err)?,
                    quiet_hours: quiet_hours.map(|value| value.0),
                    allow_critical_bypass: row
                        .try_get("allow_critical_bypass")
                        .map_err(sqlx_err)?,
                    steps: steps.remove(&id).unwrap_or_default(),
                    created_at: TimestampMicros(
                        row.try_get("created_at_micros").map_err(sqlx_err)?,
                    ),
                    updated_at: TimestampMicros(
                        row.try_get("updated_at_micros").map_err(sqlx_err)?,
                    ),
                })
            })
            .collect()
    }
}

#[async_trait]
impl UserNotifyPreferenceRepository for PgUserNotifyPreferenceRepository {
    async fn get(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<UserNotifyPreference>> {
        Ok(self
            .list_inner(organization_id, Some(user_id))
            .await?
            .into_iter()
            .find(|preference| preference.category == category))
    }

    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyPreference>> {
        self.list_inner(organization_id, Some(user_id)).await
    }

    async fn list_for_organization(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<UserNotifyPreference>> {
        self.list_inner(organization_id, None).await
    }

    async fn upsert(&self, mut preference: UserNotifyPreference) -> Result<UserNotifyPreference> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = sqlx::query(
            "INSERT INTO user_notify_preferences (
                 id, organization_id, user_id, category, enabled, quiet_hours,
                 allow_critical_bypass, created_at_micros, updated_at_micros
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (organization_id, user_id, category) DO UPDATE SET
                 enabled = EXCLUDED.enabled,
                 quiet_hours = EXCLUDED.quiet_hours,
                 allow_critical_bypass = EXCLUDED.allow_critical_bypass,
                 updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING id, created_at_micros",
        )
        .bind(&preference.id.0)
        .bind(&preference.organization_id.0)
        .bind(&preference.user_id.0)
        .bind(preference.category.as_str())
        .bind(preference.enabled)
        .bind(preference.quiet_hours.as_ref().map(Json))
        .bind(preference.allow_critical_bypass)
        .bind(preference.created_at.0)
        .bind(preference.updated_at.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let persisted_id: String = row.try_get("id").map_err(sqlx_err)?;
        preference.id = Id::from_string(persisted_id.clone());
        preference.created_at =
            TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?);

        sqlx::query("DELETE FROM user_notify_preference_steps WHERE preference_id = $1")
            .bind(&persisted_id)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        for step in &preference.steps {
            sqlx::query(
                "INSERT INTO user_notify_preference_steps (
                     id, preference_id, endpoint_id, step_order, created_at_micros
                 ) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&step.id.0)
            .bind(&persisted_id)
            .bind(&step.endpoint_id.0)
            .bind(step.step_order)
            .bind(step.created_at.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        self.get(
            &preference.organization_id,
            &preference.user_id,
            preference.category,
        )
        .await?
        .ok_or_else(|| Error::internal("upserted notify preference disappeared"))
    }
}
