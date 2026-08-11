// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Workspace-level fallback preferences.

use async_trait::async_trait;
use sqlx::PgPool;

use super::{
    sqlx_err,
    user_preferences::{UserPreferences, row_to_preferences},
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait WorkspacePreferenceDefaultsRepository: Send + Sync {
    async fn get(&self, org_id: &Id) -> Result<UserPreferences>;
    async fn upsert(&self, org_id: &Id, preferences: UserPreferences) -> Result<UserPreferences>;
}

pub struct PgWorkspacePreferenceDefaultsRepository {
    pool: PgPool,
}

impl PgWorkspacePreferenceDefaultsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkspacePreferenceDefaultsRepository for PgWorkspacePreferenceDefaultsRepository {
    async fn get(&self, org_id: &Id) -> Result<UserPreferences> {
        let row = sqlx::query(
            "SELECT theme, density, language, default_home_route, time_format,
                    date_format, timezone, keyboard_shortcuts_enabled
             FROM workspace_preference_defaults
             WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        match row {
            Some(row) => row_to_preferences(row),
            None => Ok(UserPreferences::default()),
        }
    }

    async fn upsert(&self, org_id: &Id, preferences: UserPreferences) -> Result<UserPreferences> {
        sqlx::query(
            "INSERT INTO workspace_preference_defaults (
                org_id,
                theme,
                density,
                language,
                default_home_route,
                time_format,
                date_format,
                timezone,
                keyboard_shortcuts_enabled,
                updated_at_micros
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (org_id) DO UPDATE
                SET theme = EXCLUDED.theme,
                    density = EXCLUDED.density,
                    language = EXCLUDED.language,
                    default_home_route = EXCLUDED.default_home_route,
                    time_format = EXCLUDED.time_format,
                    date_format = EXCLUDED.date_format,
                    timezone = EXCLUDED.timezone,
                    keyboard_shortcuts_enabled = EXCLUDED.keyboard_shortcuts_enabled,
                    updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(&preferences.theme)
        .bind(&preferences.density)
        .bind(&preferences.language)
        .bind(&preferences.default_home_route)
        .bind(&preferences.time_format)
        .bind(&preferences.date_format)
        .bind(&preferences.timezone)
        .bind(preferences.keyboard_shortcuts_enabled)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(preferences)
    }
}
