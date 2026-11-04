// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! APM projector, rollup and query use-case orchestration.

mod aggregator;
mod cardinality;
mod error_rules;
mod extractor;
mod instance;
mod metrics;
mod org_settings;
mod projector;
mod query;
mod rollup;
mod runtime;

pub use aggregator::*;
pub use cardinality::*;
pub use error_rules::*;
pub use extractor::*;
pub use instance::*;
pub use org_settings::*;
pub use projector::*;
pub use query::*;
pub use rollup::*;
pub use runtime::*;

pub fn record_apm_query_latency(endpoint: &'static str, elapsed: std::time::Duration) {
    metrics::record_api(endpoint, elapsed);
}
