// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的优先级与紧急度档位，存在 `org_settings` 的 `incident.importance` 一节；没有这一节时用内置的。

use async_trait::async_trait;
use sqlx::PgPool;

use super::org_settings::{self, sections::INCIDENT_IMPORTANCE};
use crate::{
    domain::alerting::importance::{ImportanceLevels, ImportanceRepository, StoredImportance},
    shared::{Result, ids::Id},
};

pub struct PgImportanceRepository {
    pool: PgPool,
}

impl PgImportanceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ImportanceRepository for PgImportanceRepository {
    async fn get(&self, org_id: &Id) -> Result<Option<StoredImportance>> {
        let section =
            org_settings::read::<ImportanceLevels>(&self.pool, &org_id.0, INCIDENT_IMPORTANCE)
                .await?;

        Ok(section.map(|section| StoredImportance {
            levels: section.value,
            updated_by: section.updated_by.map(Id),
            updated_at: section.updated_at,
        }))
    }

    async fn save(&self, org_id: &Id, stored: &StoredImportance) -> Result<()> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            INCIDENT_IMPORTANCE,
            &stored.levels,
            stored.updated_by.as_ref().map(|id| id.0.as_str()),
            stored.updated_at,
        )
        .await
    }
}
