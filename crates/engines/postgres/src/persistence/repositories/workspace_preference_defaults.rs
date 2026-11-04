// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Workspace-level fallback preferences, kept in the `workspace_preferences`
//! section of `org_settings` with the same shape as `users_ext.preferences`.

use async_trait::async_trait;
use sqlx::PgPool;

use super::{
    org_settings::{self, sections::WORKSPACE_PREFERENCES},
    user_preferences::UserPreferences,
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
        Ok(
            org_settings::read(&self.pool, &org_id.0, WORKSPACE_PREFERENCES)
                .await?
                .map(|section| section.value)
                .unwrap_or_default(),
        )
    }

    async fn upsert(&self, org_id: &Id, preferences: UserPreferences) -> Result<UserPreferences> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            WORKSPACE_PREFERENCES,
            &preferences,
            None,
            TimestampMicros::now(),
        )
        .await?;

        Ok(preferences)
    }
}
