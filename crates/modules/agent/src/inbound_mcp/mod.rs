// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Inbound MCP control-plane models and persistence boundary.
//!
//! The protocol adapter lives in `bin/molesignal`; this module owns the durable,
//! protocol-neutral organization settings, OAuth grants, idempotency records, and tasks.

mod idempotency;
mod oauth;
mod repository;
mod settings;
mod tasks;

pub use idempotency::*;
pub use oauth::*;
pub use repository::*;
pub use settings::*;
pub use tasks::*;
