// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped APM read use cases.

use std::sync::Arc;

use crate::{
    config::ApmSettings,
    domain::apm::{ApmQueryRepository, HistogramSchema},
    shared::{Result, ids::Id},
};

mod aggregate;
mod common;
mod context;
mod errors;
mod model;
mod overview;
mod pagination;
mod ranking;
mod versions;

pub use context::*;
pub use model::*;

#[derive(Debug, Clone)]
pub struct ApmQueryConfig {
    pub max_range_micros: i64,
    pub hot_resolution_micros: i64,
    pub minimum_version_requests: u64,
    pub histogram: HistogramSchema,
}

impl ApmQueryConfig {
    pub fn from_settings(settings: &ApmSettings) -> Self {
        let mut upper_bounds_micros = settings
            .histogram
            .boundaries_ms
            .iter()
            .map(|value| value.saturating_mul(1_000))
            .collect::<Vec<_>>();
        upper_bounds_micros.push(u64::MAX);
        Self {
            max_range_micros: i64::from(settings.max_query_range_days)
                .saturating_mul(86_400_000_000),
            hot_resolution_micros: i64::from(settings.hot_retention_hours)
                .saturating_mul(3_600_000_000),
            minimum_version_requests: settings.version_comparison.min_requests_per_version,
            histogram: HistogramSchema {
                version: settings.histogram.schema_version,
                upper_bounds_micros,
            },
        }
    }
}

pub struct ApmQueryService {
    repository: Arc<dyn ApmQueryRepository>,
    config: ApmQueryConfig,
}

impl ApmQueryService {
    pub fn new(repository: Arc<dyn ApmQueryRepository>, config: ApmQueryConfig) -> Self {
        Self { repository, config }
    }

    pub fn context(
        &self,
        org_id: Id,
        request: ApmQueryRequest,
        allowed_sorts: &[&str],
        default_sort: &str,
    ) -> Result<ApmQueryContext> {
        ApmQueryContext::build(
            org_id,
            request,
            self.config.max_range_micros,
            self.config.hot_resolution_micros,
            allowed_sorts,
            default_sort,
        )
    }
}

#[cfg(test)]
mod tests;
