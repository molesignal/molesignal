// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL repositories, migrations, encrypted persistence, and Probe CA storage.

pub mod certificate_authority;
pub mod cipher;
pub mod persistence;
pub mod synthetics;

pub use persistence::{MetaStore, is_unique_violation, pool, repositories, sqlx_err};
