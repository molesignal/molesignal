// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped APM read use cases.

use std::sync::Arc;

use crate::{
    config::ApmSettings,
    domain::apm::{ApmCatalogRepository, ApmOrgSettings, ApmQueryRepository, HistogramSchema},
    shared::{Result, ids::Id},
};

mod aggregate;
mod alert;
mod aliases;
mod apdex;
mod blast;
mod breakdown;
mod common;
mod context;
mod deploys;
mod errors;
mod health;
mod insights;
mod instances;
mod map;
mod map_edge;
mod model;
mod overview;
mod pagination;
mod profile;
mod ranking;
mod settings;
mod slo;
mod standings;
mod versions;

pub use blast::{BlastNode, BlastRadius};
pub use breakdown::{ErrorBreakdown, ErrorShare};
pub use context::*;
pub use errors::ErrorIssueFilter;
pub use insights::{
    BaselineFigures, ChangeSource, Insight, InsightKind, InsightSeverity, InsightsResponse,
    NearbyChange,
};
pub use instances::InstanceSummary;
pub use map::{
    GraphCall, MapChange, MapEdge, MapEdgeKind, MapNode, MapNodeKind, MapRed, MapResponse,
};
pub use map_edge::{GraphPoint, MapEdgeDetail, MapOperation};
pub use model::*;
pub use profile::{ProfileValue, ServiceProfile};
pub use slo::{SloBurnRates, SloDay, SloDetailResponse, SloState, SloStatus};

#[derive(Debug, Clone)]
pub struct ApmQueryConfig {
    pub max_range_micros: i64,
    pub hot_resolution_micros: i64,
    pub minimum_version_requests: u64,
    pub histogram: HistogramSchema,
}

impl ApmQueryConfig {
    pub fn from_settings(settings: &ApmSettings) -> Self {
        Self {
            max_range_micros: i64::from(settings.max_query_range_days)
                .saturating_mul(86_400_000_000),
            hot_resolution_micros: i64::from(settings.hot_retention_hours)
                .saturating_mul(3_600_000_000),
            minimum_version_requests: settings.version_comparison.min_requests_per_version,
            histogram: HistogramSchema::logarithmic(
                settings.histogram.schema_version,
                settings.histogram.buckets_per_octave,
            ),
        }
    }
}

pub struct ApmQueryService {
    repository: Arc<dyn ApmQueryRepository>,
    config: ApmQueryConfig,
    /// What people keep about error groups and set for APM; without it every
    /// group reads as open and every organization has the default settings.
    catalog: Option<Arc<dyn ApmCatalogRepository>>,
    settings: Arc<settings::OrgSettingsCache>,
}

impl ApmQueryService {
    pub fn new(repository: Arc<dyn ApmQueryRepository>, config: ApmQueryConfig) -> Self {
        Self {
            repository,
            config,
            catalog: None,
            settings: Arc::default(),
        }
    }

    /// Reads the state of each error group and the organization's settings
    /// from `catalog`, and services under their aliases as the services they are.
    pub fn with_catalog(mut self, catalog: Arc<dyn ApmCatalogRepository>) -> Self {
        self.settings = Arc::new(settings::OrgSettingsCache::new(catalog.clone()));
        self.repository = Arc::new(aliases::AliasedRepository::new(
            self.repository,
            self.settings.clone(),
        ));
        self.catalog = Some(catalog);
        self
    }

    /// The organization's APM settings as its queries read them.
    pub async fn org_settings(&self, org_id: &Id) -> Arc<ApmOrgSettings> {
        self.settings.get(org_id).await
    }

    /// Reads the organization's settings anew on its next query.
    pub fn forget_settings(&self, org_id: &Id) {
        self.settings.forget(org_id);
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
