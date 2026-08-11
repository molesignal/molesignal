// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for customer-facing status pages.

mod access;
mod access_codec;
mod codec;
mod components;
mod deliveries;
mod domains;
mod event_queries;
mod incidents;
mod pages;
mod subscriptions;

use sqlx::PgPool;

pub(crate) use super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgStatusPageRepository {
    pub(crate) pool: PgPool,
    cipher: CipherRootKey,
}

impl PgStatusPageRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

async fn touch_page(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
    updated_at: TimestampMicros,
) -> Result<()> {
    let result = sqlx::query(
        "UPDATE status_pages
         SET updated_at_micros = GREATEST(updated_at_micros, $3)
         WHERE org_id = $1 AND id = $2",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(updated_at.0)
    .execute(transaction)
    .await
    .map_err(sqlx_err)?;
    if result.rows_affected() == 0 {
        return Err(crate::shared::Error::not_found("status page not found"));
    }
    Ok(())
}
