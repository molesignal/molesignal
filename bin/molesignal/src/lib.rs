// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal application package.
//!
//! This crate owns HTTP/gRPC delivery, application orchestration, bootstrap
//! wiring, and infrastructure adapters that have not yet earned an independent
//! engine boundary. Stable core, product modules, engines, transports, and
//! support facilities live in the workspace crates under `crates/`.

pub mod api;
pub mod app;
pub mod bootstrap;
pub mod config;
pub mod domain;
pub mod infra;
pub mod protocol;
pub mod shared;
pub mod tantivy;

// Product modules are workspace crates and are re-exported for API compatibility.
pub use ::agent;
pub use ::domain_management;
pub use ::report_renderer;
