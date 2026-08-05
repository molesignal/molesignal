// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod catalog;
mod codec;
mod maintenance;
mod query;
mod write;

use sqlx::PgPool;

/// PostgreSQL APM adapter. All operations keep `org_id` as the leading
/// predicate and never infer tenant scope from persisted dimensions.
pub struct PgApmRepository {
    pub(super) pool: PgPool,
}

impl PgApmRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
