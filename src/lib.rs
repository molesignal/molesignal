// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal backend — single crate.
//!
//! Each top-level module was previously a separate workspace crate; they were
//! merged into one crate so the backend lives entirely under `src/`. The
//! remaining support crates are `src/sqlx-shim` (package name `sqlx`), which
//! substitutes the external crate name that derive macros resolve against,
//! and the compile-time-only IAM route permission proc macro.

pub mod api;
pub mod app;
pub mod bootstrap;
pub mod config;
pub mod domain;
pub mod infra;
pub mod protocol;
pub mod shared;
pub mod tantivy;

// Former premium / add-on crates, now unconditional modules.
pub mod cloud_marketplace;
pub mod domain_management;
pub mod intelligence;
pub mod license;
pub mod model_pricing;
pub mod report_renderer;
