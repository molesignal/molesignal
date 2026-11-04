// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Application Performance Monitoring domain contracts.
//!
//! APM is a bounded projection of sanitized Trace candidates. The domain
//! types in this module are independent of PostgreSQL, Axum and the Trace
//! sampler so projector and query implementations can share one stable
//! persistence contract.

mod aggregate;
mod alert;
mod catalog;
mod deployment;
mod error_rules;
mod fact;
mod histogram;
mod identity;
mod instance;
mod limits;
mod org_settings;
mod quality;
mod query;
mod repository;
mod repository_address;
mod slo;

pub use aggregate::*;
pub use alert::*;
pub use catalog::*;
pub use deployment::*;
pub use error_rules::*;
pub use fact::*;
pub use histogram::*;
pub use identity::*;
pub use instance::*;
pub use limits::*;
pub use org_settings::*;
pub use quality::*;
pub use query::*;
pub use repository::*;
pub use repository_address::*;
pub use slo::*;

#[cfg(test)]
mod tests;
