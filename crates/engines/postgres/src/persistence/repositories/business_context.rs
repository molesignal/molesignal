// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The business context of an organization, kept as the `business_context` section of
//! `org_settings`. What it means and how it is checked is in `domain::business_context`.

use async_trait::async_trait;
use domain::business_context::{BusinessContext, BusinessContextRepository};
use kernel::{Result, ids::Id, time::TimestampMicros};
use sqlx::PgPool;

use super::org_settings::{self, sections::BUSINESS_CONTEXT};

pub struct PgBusinessContextRepository {
    pool: PgPool,
}

impl PgBusinessContextRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BusinessContextRepository for PgBusinessContextRepository {
    async fn business_context(&self, org_id: &Id) -> Result<BusinessContext> {
        Ok(
            org_settings::read::<BusinessContext>(&self.pool, &org_id.0, BUSINESS_CONTEXT)
                .await?
                .map(|section| section.value)
                .unwrap_or_default(),
        )
    }

    async fn save_business_context(
        &self,
        org_id: &Id,
        context: &BusinessContext,
        updated_by: &Id,
        now: TimestampMicros,
    ) -> Result<()> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            BUSINESS_CONTEXT,
            context,
            Some(&updated_by.0),
            now,
        )
        .await
    }
}
