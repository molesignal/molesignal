// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::{
        alerting::importance::StepGroup,
        notify::{
            preference::{NotifyCategory, UserNotifyPreference, UserNotifyPreferenceStep},
            repositories::UserNotifyPreferenceRepository,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// `user_notify_preferences.steps` 里的一步。
#[derive(Serialize, Deserialize)]
struct StoredStep {
    id: String,
    endpoint_id: String,
    step_order: i32,
    delay_secs: i32,
    urgency: String,
    created_at: TimestampMicros,
}

impl StoredStep {
    fn from_step(step: &UserNotifyPreferenceStep) -> Self {
        Self {
            id: step.id.0.clone(),
            endpoint_id: step.endpoint_id.0.clone(),
            step_order: step.step_order,
            delay_secs: step.delay_secs,
            urgency: step.urgency.as_str().to_owned(),
            created_at: step.created_at,
        }
    }

    fn into_step(self, preference_id: &str) -> Result<UserNotifyPreferenceStep> {
        let urgency = StepGroup::from_label(&self.urgency).ok_or_else(|| {
            Error::internal(format!("unknown notify step urgency `{}`", self.urgency))
        })?;

        Ok(UserNotifyPreferenceStep {
            id: Id::from_string(self.id),
            preference_id: Id::from_string(preference_id.to_owned()),
            endpoint_id: Id::from_string(self.endpoint_id),
            step_order: self.step_order,
            delay_secs: self.delay_secs,
            urgency,
            created_at: self.created_at,
        })
    }
}

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
                    allow_high_urgency_bypass, steps, created_at, updated_at
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

        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id").map_err(sqlx_err)?;
                let category: String = row.try_get("category").map_err(sqlx_err)?;
                let quiet_hours: Option<Json<serde_json::Value>> =
                    row.try_get("quiet_hours").map_err(sqlx_err)?;

                let steps = row
                    .try_get::<Json<Vec<StoredStep>>, _>("steps")
                    .map_err(sqlx_err)?
                    .0
                    .into_iter()
                    .map(|step| step.into_step(&id))
                    .collect::<Result<Vec<_>>>()?;

                Ok(UserNotifyPreference {
                    id: Id::from_string(id),
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
                    allow_high_urgency_bypass: row
                        .try_get("allow_high_urgency_bypass")
                        .map_err(sqlx_err)?,
                    steps,
                    created_at: row
                        .try_get::<TimestampMicros, _>("created_at")
                        .map_err(sqlx_err)?,
                    updated_at: row
                        .try_get::<TimestampMicros, _>("updated_at")
                        .map_err(sqlx_err)?,
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

    async fn upsert(&self, preference: UserNotifyPreference) -> Result<UserNotifyPreference> {
        let mut steps: Vec<StoredStep> =
            preference.steps.iter().map(StoredStep::from_step).collect();
        steps.sort_by(|left, right| {
            (&left.urgency, left.step_order, &left.id).cmp(&(
                &right.urgency,
                right.step_order,
                &right.id,
            ))
        });

        let endpoint_ids: Vec<String> = steps
            .iter()
            .map(|step| step.endpoint_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // 锁住步骤用到的通知方式：删除通知方式要等这次写完，之后就能看到它还被引用。
        let locked = sqlx::query(
            "SELECT id FROM user_notify_endpoints
              WHERE id = ANY($1::TEXT[])
                FOR KEY SHARE",
        )
        .bind(&endpoint_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if locked.len() != endpoint_ids.len() {
            return Err(Error::not_found("user notify endpoint"));
        }

        sqlx::query(
            "INSERT INTO user_notify_preferences (
                 id, organization_id, user_id, category, enabled, quiet_hours,
                 allow_high_urgency_bypass, steps, created_at, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (organization_id, user_id, category) DO UPDATE SET
                 enabled = EXCLUDED.enabled,
                 quiet_hours = EXCLUDED.quiet_hours,
                 allow_high_urgency_bypass = EXCLUDED.allow_high_urgency_bypass,
                 steps = EXCLUDED.steps,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(&preference.id.0)
        .bind(&preference.organization_id.0)
        .bind(&preference.user_id.0)
        .bind(preference.category.as_str())
        .bind(preference.enabled)
        .bind(preference.quiet_hours.as_ref().map(Json))
        .bind(preference.allow_high_urgency_bypass)
        .bind(Json(&steps))
        .bind(preference.created_at)
        .bind(preference.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

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
