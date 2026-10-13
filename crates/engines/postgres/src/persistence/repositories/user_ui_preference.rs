// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a user prefers a page to look, kept per organization, purpose and key.
//!
//! Unlike the settings of `user_preferences`, these are written as the user
//! works, read back the next time the page opens, and seen by nobody else. The
//! first of its kind is the arrangement of a log stream's columns, keyed by the
//! stream's name.

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// How many keys one user keeps for one purpose in one organization. Past it
/// the least recently written are dropped, so the table is bounded by its users.
pub const MAX_KEYS_PER_SCOPE: i64 = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserUiPreference {
    pub value: serde_json::Value,
    pub updated_at_micros: i64,
}

/// Whose preference it is, in which organization, of what purpose, and under what key.
#[derive(Debug, Clone, Copy)]
pub struct UiPreferenceKey<'a> {
    pub user_id: &'a Id,
    pub org_id: &'a Id,
    pub scope: &'a str,
    pub key: &'a str,
}

#[async_trait]
pub trait UserUiPreferenceRepository: Send + Sync {
    async fn get(&self, key: UiPreferenceKey<'_>) -> Result<Option<UserUiPreference>>;
    async fn put(
        &self,
        key: UiPreferenceKey<'_>,
        value: serde_json::Value,
    ) -> Result<UserUiPreference>;
    async fn delete(&self, key: UiPreferenceKey<'_>) -> Result<()>;
}

pub struct PgUserUiPreferenceRepository {
    pool: PgPool,
}

impl PgUserUiPreferenceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserUiPreferenceRepository for PgUserUiPreferenceRepository {
    async fn get(&self, key: UiPreferenceKey<'_>) -> Result<Option<UserUiPreference>> {
        let row = sqlx::query(
            "SELECT preference_value, updated_at_micros
             FROM user_ui_preference
             WHERE user_id = $1 AND org_id = $2 AND scope = $3 AND preference_key = $4",
        )
        .bind(&key.user_id.0)
        .bind(&key.org_id.0)
        .bind(key.scope)
        .bind(key.key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let value: Json<serde_json::Value> = row.try_get("preference_value").map_err(sqlx_err)?;

        Ok(Some(UserUiPreference {
            value: value.0,
            updated_at_micros: row.try_get("updated_at_micros").map_err(sqlx_err)?,
        }))
    }

    async fn put(
        &self,
        key: UiPreferenceKey<'_>,
        value: serde_json::Value,
    ) -> Result<UserUiPreference> {
        let updated_at_micros = TimestampMicros::now().0;

        sqlx::query(
            "INSERT INTO user_ui_preference (
                user_id, org_id, scope, preference_key, preference_value, updated_at_micros
             )
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_id, org_id, scope, preference_key) DO UPDATE
                SET preference_value = EXCLUDED.preference_value,
                    updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&key.user_id.0)
        .bind(&key.org_id.0)
        .bind(key.scope)
        .bind(key.key)
        .bind(Json(&value))
        .bind(updated_at_micros)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        // What was written longest ago gives way once the purpose holds more keys than it may.
        sqlx::query(
            "DELETE FROM user_ui_preference
             WHERE user_id = $1 AND org_id = $2 AND scope = $3
               AND preference_key IN (
                   SELECT preference_key
                   FROM user_ui_preference
                   WHERE user_id = $1 AND org_id = $2 AND scope = $3
                   ORDER BY updated_at_micros DESC, preference_key ASC
                   OFFSET $4
               )",
        )
        .bind(&key.user_id.0)
        .bind(&key.org_id.0)
        .bind(key.scope)
        .bind(MAX_KEYS_PER_SCOPE)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(UserUiPreference {
            value,
            updated_at_micros,
        })
    }

    async fn delete(&self, key: UiPreferenceKey<'_>) -> Result<()> {
        sqlx::query(
            "DELETE FROM user_ui_preference
             WHERE user_id = $1 AND org_id = $2 AND scope = $3 AND preference_key = $4",
        )
        .bind(&key.user_id.0)
        .bind(&key.org_id.0)
        .bind(key.scope)
        .bind(key.key)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
