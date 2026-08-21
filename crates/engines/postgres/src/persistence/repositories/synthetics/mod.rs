// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for user-facing synthetic monitoring.

mod agent_tokens;
mod agents;
mod codec;
mod locations;
mod monitors;
mod register;
mod results;
mod secrets;
mod states;
mod tasks;

use sqlx::PgPool;

pub(crate) use super::sqlx_err;
use crate::infra::cipher::CipherRootKey;

pub struct PgSyntheticRepository {
    pub(crate) pool: PgPool,
    pub(crate) cipher: CipherRootKey,
}

impl PgSyntheticRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}
