// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use super::{
    ApmQueryContext, ApmQueryService, ApmTenantHealthResponse, InstrumentationSummary,
    OverviewResponse, PagedResponse, ServiceDetailResponse, ServiceHealth, ServiceHealthCounts,
    ServiceSummary, VersionSummary,
    aggregate::{red_from_buckets, trend_from_buckets},
    common::{overflow_dimensions, signal_handle},
    errors::error_summaries,
    pagination::paginate,
    ranking::{dependency_summaries, transaction_summaries},
};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, MergedBucket, ServiceIdentity, ServiceObservation,
        VersionObservation,
    },
    shared::{Error, Result},
};

impl ApmQueryService {
    pub async fn tenant_health(
        &self,
        context: &ApmQueryContext,
        runtime: Option<&crate::app::apm::ApmRuntime>,
    ) -> Result<ApmTenantHealthResponse> {
        let runtime_health = runtime.map(|runtime| runtime.health());
        let degraded = runtime_health.as_ref().is_some_and(|health| {
            health
                .projector
                .as_ref()
                .is_some_and(|projector| projector.degraded)
                || health.rollup.degraded
        });
        Ok(ApmTenantHealthResponse {
            meta: self.response_meta(context, Vec::new(), false).await?,
            enabled: runtime.is_some(),
            degraded,
            runtime: runtime_health,
        })
    }

    pub async fn overview(&self, context: &ApmQueryContext) -> Result<OverviewResponse> {
        let service_query = self.bucket_query(context, BucketKind::Service);
        let transaction_query = self.bucket_query(context, BucketKind::Transaction);
        let dependency_query = self.bucket_query(context, BucketKind::Dependency);
        let error_query = self.bucket_query(context, BucketKind::Error);
        let catalog_query = self.catalog_query(context);
        let group_query = self.error_group_query(context, None);
        let (
            service_buckets,
            transaction_buckets,
            dependency_buckets,
            catalog,
            versions,
            groups,
            error_buckets,
        ) = tokio::try_join!(
            self.repository.query_buckets(&service_query),
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&dependency_query),
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
            self.repository.list_error_groups(&group_query),
            self.repository.query_buckets(&error_query),
        )?;
        let mut services = build_service_summaries(
            context,
            &catalog,
            &versions,
            &service_buckets,
            &self.config.histogram,
        )?;
        let service_health = health_counts(&services);
        services.sort_by(|left, right| {
            right
                .red
                .duration_sum_micros
                .cmp(&left.red.duration_sum_micros)
                .then_with(|| left.service.stable_key().cmp(&right.service.stable_key()))
        });
        services.truncate(10);
        let mut transactions =
            transaction_summaries(context, &transaction_buckets, &self.config.histogram)?;
        transactions.sort_by(|left, right| {
            right
                .total_time_micros
                .cmp(&left.total_time_micros)
                .then_with(|| left.transaction.name.cmp(&right.transaction.name))
        });
        transactions.truncate(10);
        let mut dependencies =
            dependency_summaries(context, &dependency_buckets, &self.config.histogram)?;
        dependencies.sort_by(|left, right| {
            right
                .total_time_micros
                .cmp(&left.total_time_micros)
                .then_with(|| left.dependency.target.cmp(&right.dependency.target))
        });
        dependencies.truncate(10);
        let mut errors = error_summaries(context, &groups, &error_buckets, &self.config.histogram)?;
        errors.sort_by(|left, right| {
            right
                .occurrence_count
                .cmp(&left.occurrence_count)
                .then_with(|| left.error.fingerprint.cmp(&right.error.fingerprint))
        });
        errors.truncate(10);
        let recent_versions = versions.into_iter().take(10).map(version_summary).collect();
        let red = red_from_buckets(&service_buckets, &self.config.histogram)?;
        let latency_partial = red.latency_partial
            || services.iter().any(|item| item.red.latency_partial)
            || transactions.iter().any(|item| item.red.latency_partial)
            || dependencies.iter().any(|item| item.red.latency_partial)
            || errors.iter().any(|item| item.red.latency_partial);
        let mut overflows = overflow_dimensions(&service_buckets);
        overflows.extend(overflow_dimensions(&transaction_buckets));
        overflows.extend(overflow_dimensions(&dependency_buckets));
        overflows.extend(overflow_dimensions(&error_buckets));
        Ok(OverviewResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            trend: trend_from_buckets(&service_buckets, &self.config.histogram)?,
            service_health,
            red,
            services,
            top_transactions: transactions,
            top_dependencies: dependencies,
            top_errors: errors,
            recent_versions,
        })
    }

    pub async fn services(
        &self,
        context: &ApmQueryContext,
    ) -> Result<PagedResponse<ServiceSummary>> {
        let bucket_query = self.bucket_query(context, BucketKind::Service);
        let catalog_query = self.catalog_query(context);
        let (buckets, catalog, versions) = tokio::try_join!(
            self.repository.query_buckets(&bucket_query),
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
        )?;
        let items = build_service_summaries(
            context,
            &catalog,
            &versions,
            &buckets,
            &self.config.histogram,
        )?;
        let latency_partial = items.iter().any(|item| item.red.latency_partial);
        let (items, previous_cursor, next_cursor) = paginate(items, context)?;
        let has_more = next_cursor.is_some();
        Ok(PagedResponse {
            meta: self
                .response_meta(context, overflow_dimensions(&buckets), latency_partial)
                .await?,
            items,
            next_cursor,
            previous_cursor,
            has_more,
            sort: context.sort.clone(),
        })
    }

    pub async fn service_detail(&self, context: &ApmQueryContext) -> Result<ServiceDetailResponse> {
        let service_query = self.bucket_query(context, BucketKind::Service);
        let transaction_query = self.bucket_query(context, BucketKind::Transaction);
        let dependency_query = self.bucket_query(context, BucketKind::Dependency);
        let error_query = self.bucket_query(context, BucketKind::Error);
        let (service_buckets, transaction_buckets, dependency_buckets, error_buckets) = tokio::try_join!(
            self.repository.query_buckets(&service_query),
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&dependency_query),
            self.repository.query_buckets(&error_query),
        )?;
        let catalog_query = self.catalog_query(context);
        let group_query = self.error_group_query(context, None);
        let (catalog, versions, groups) = tokio::try_join!(
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
            self.repository.list_error_groups(&group_query),
        )?;
        let mut services = build_service_summaries(
            context,
            &catalog,
            &versions,
            &service_buckets,
            &self.config.histogram,
        )?;
        if services.len() > 1 && (context.namespace.is_none() || context.environment.is_none()) {
            return Err(Error::invalid(
                "APM service identity is ambiguous; provide namespace and environment",
            ));
        }
        let service = services
            .pop()
            .ok_or_else(|| Error::not_found("APM service"))?;
        let transactions =
            transaction_summaries(context, &transaction_buckets, &self.config.histogram)?;
        let dependencies =
            dependency_summaries(context, &dependency_buckets, &self.config.histogram)?;
        let errors = error_summaries(context, &groups, &error_buckets, &self.config.histogram)?;
        let red = red_from_buckets(&service_buckets, &self.config.histogram)?;
        let latency_partial = red.latency_partial
            || transactions.iter().any(|item| item.red.latency_partial)
            || dependencies.iter().any(|item| item.red.latency_partial)
            || errors.iter().any(|item| item.red.latency_partial);
        let mut overflows = overflow_dimensions(&service_buckets);
        overflows.extend(overflow_dimensions(&transaction_buckets));
        overflows.extend(overflow_dimensions(&dependency_buckets));
        overflows.extend(overflow_dimensions(&error_buckets));
        Ok(ServiceDetailResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            trend: trend_from_buckets(&service_buckets, &self.config.histogram)?,
            service,
            red,
            transactions,
            dependencies,
            errors,
            versions: versions.into_iter().map(version_summary).collect(),
        })
    }
}

pub(super) fn build_service_summaries(
    context: &ApmQueryContext,
    catalog: &[ServiceObservation],
    versions: &[VersionObservation],
    buckets: &[MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
) -> Result<Vec<ServiceSummary>> {
    let mut grouped = HashMap::<ServiceIdentity, Vec<MergedBucket>>::new();
    for bucket in buckets {
        if let BucketDimension::Service { service, .. } = &bucket.dimension {
            grouped
                .entry(service.clone())
                .or_default()
                .push(bucket.clone());
        }
    }
    let mut observations = catalog
        .iter()
        .map(|value| (value.service.clone(), value.clone()))
        .collect::<HashMap<_, _>>();
    for (service, values) in &grouped {
        observations.entry(service.clone()).or_insert_with(|| {
            let first = values
                .iter()
                .map(|bucket| bucket.bucket_at)
                .min()
                .unwrap_or(context.range.start);
            let last = values
                .iter()
                .map(|bucket| bucket.bucket_at)
                .max()
                .unwrap_or(context.range.end);
            ServiceObservation {
                org_id: context.org_id.clone(),
                service: service.clone(),
                first_seen_at: first,
                last_seen_at: last,
                runtime_language: None,
                telemetry_sdk_name: None,
                telemetry_sdk_version: None,
                recent_instance_count: 0,
            }
        });
    }
    observations
        .into_values()
        .map(|observation| {
            let matching = grouped.remove(&observation.service).unwrap_or_default();
            let red = red_from_buckets(&matching, histogram)?;
            let mut service_versions = versions
                .iter()
                .filter(|version| version.service == observation.service)
                .map(|version| version.version.clone())
                .collect::<Vec<_>>();
            service_versions.sort();
            service_versions.dedup();
            Ok(ServiceSummary {
                traces: signal_handle(
                    context,
                    &observation.service,
                    context.version.clone(),
                    None,
                    None,
                    None,
                ),
                service: observation.service,
                first_seen_at: observation.first_seen_at,
                last_seen_at: observation.last_seen_at,
                instrumentation: InstrumentationSummary {
                    runtime_language: observation.runtime_language,
                    telemetry_sdk_name: observation.telemetry_sdk_name,
                    telemetry_sdk_version: observation.telemetry_sdk_version,
                    recent_instance_count: observation.recent_instance_count,
                },
                versions: service_versions,
                health: classify_health(&red),
                red,
            })
        })
        .collect()
}

fn classify_health(red: &super::RedSummary) -> ServiceHealth {
    if red.request_count == 0 {
        ServiceHealth::NoTraffic
    } else if red.error_rate >= 0.10 || red.p95_micros.is_some_and(|value| value >= 2_000_000) {
        ServiceHealth::Critical
    } else if red.error_rate >= 0.02 || red.p95_micros.is_some_and(|value| value >= 500_000) {
        ServiceHealth::Warning
    } else {
        ServiceHealth::Healthy
    }
}

fn health_counts(services: &[ServiceSummary]) -> ServiceHealthCounts {
    let mut counts = ServiceHealthCounts::default();
    for service in services {
        match service.health {
            ServiceHealth::Healthy => counts.healthy += 1,
            ServiceHealth::Warning => counts.warning += 1,
            ServiceHealth::Critical => counts.critical += 1,
            ServiceHealth::NoTraffic => counts.no_traffic += 1,
        }
    }
    counts
}

pub(super) fn version_summary(version: VersionObservation) -> VersionSummary {
    VersionSummary {
        service: version.service,
        version: version.version,
        first_seen_at: version.first_seen_at,
        last_seen_at: version.last_seen_at,
        observation_count: version.observation_count,
    }
}
