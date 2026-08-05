// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Application Performance Monitoring domain contracts.
//!
//! APM is a bounded projection of sanitized Trace candidates. The domain
//! types in this module are independent of PostgreSQL, Axum and the Trace
//! sampler so projector and query implementations can share one stable
//! persistence contract.

mod aggregate;
mod fact;
mod histogram;
mod identity;
mod quality;
mod query;
mod repository;

pub use aggregate::*;
pub use fact::*;
pub use histogram::*;
pub use identity::*;
pub use quality::*;
pub use query::*;
pub use repository::*;

#[cfg(test)]
mod tests;
