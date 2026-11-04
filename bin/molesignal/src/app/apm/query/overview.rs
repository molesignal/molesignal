// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use super::{
    ApmQueryContext, ApmQueryService, ApmTenantHealthResponse, InstrumentationSummary,
    OverviewResponse, PagedResponse, ServiceDetailResponse, ServiceSummary, VersionSummary,
    aggregate::{
        distribution_from_buckets, red_from_buckets_at, sparkline_from_buckets,
        trend_from_buckets_at,
    },
    apdex::ApdexThresholds,
    common::{overflow_dimensions, signal_handle},
    deploys::judge_versions,
    errors::error_summaries,
    health::{assess_health, health_counts, health_trend},
    pagination::paginate,
    profile::{instances_by_service, service_profile},
    ranking::{dependency_summaries, transaction_summaries},
    standings,
};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, HealthLines, InstanceObservation, MergedBucket,
        ServiceIdentity, ServiceObservation, VersionObservation,
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

        // Every environment the services run in, whichever one is in view: what can be chosen.
        let mut everywhere = context.clone();
        everywhere.environment = None;
        let everywhere_query = self.catalog_query(&everywhere);

        let (
            service_buckets,
            transaction_buckets,
            dependency_buckets,
            catalog,
            versions,
            groups,
            error_buckets,
            everywhere_catalog,
            thresholds,
        ) = tokio::try_join!(
            self.repository.query_buckets(&service_query),
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&dependency_query),
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
            self.repository.list_error_groups(&group_query),
            self.repository.query_buckets(&error_query),
            self.repository.list_services(&everywhere_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        let mut available_environments = everywhere_catalog
            .iter()
            .map(|observation| observation.service.environment.clone())
            .collect::<Vec<_>>();
        available_environments.sort();
        available_environments.dedup();

        let lines = self.org_settings(&context.org_id).await.health;
        let mut services = build_service_summaries(
            context,
            &catalog,
            &versions,
            None,
            &service_buckets,
            &self.config.histogram,
            (&thresholds, &lines),
        )?;

        let service_health = health_counts(&services);
        let identities = services
            .iter()
            .map(|service| service.service.clone())
            .collect::<Vec<_>>();

        let service_count = services.len() as u64;
        let active_service_count = services
            .iter()
            .filter(|service| service.red.request_count > 0)
            .count() as u64;
        let health_trend = health_trend(
            context,
            &identities,
            &service_buckets,
            &self.config.histogram,
            &lines,
        )?;
        let rankings = standings::rankings(&services);
        let environments = standings::environments(&services);
        let instrumentation = standings::instrumentation(&services);
        let mut deployments = standings::deployments(context, &versions);

        judge_versions(
            &mut deployments,
            &versions,
            &service_buckets,
            &self.config.histogram,
            &thresholds,
            self.config.minimum_version_requests,
        )?;

        services.sort_by(|left, right| {
            right
                .red
                .duration_sum
                .cmp(&left.red.duration_sum)
                .then_with(|| left.service.stable_key().cmp(&right.service.stable_key()))
        });

        services.truncate(10);
        let mut transactions = transaction_summaries(
            context,
            &transaction_buckets,
            &self.config.histogram,
            &thresholds,
        )?;

        transactions.sort_by(|left, right| {
            right
                .total_time
                .cmp(&left.total_time)
                .then_with(|| left.transaction.name.cmp(&right.transaction.name))
        });

        transactions.truncate(10);
        let mut dependencies =
            dependency_summaries(context, &dependency_buckets, &self.config.histogram)?;

        dependencies.sort_by(|left, right| {
            right
                .total_time
                .cmp(&left.total_time)
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
        // Every service at once is read with the organization's threshold.
        let threshold = match &context.service_name {
            Some(name) => thresholds.named(context.namespace.as_deref(), name),
            None => thresholds.default_micros(),
        };
        let red = red_from_buckets_at(&service_buckets, &self.config.histogram, threshold)?;

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
            trend: trend_from_buckets_at(&service_buckets, &self.config.histogram, threshold)?,
            service_health,
            red,
            services,
            top_transactions: transactions,
            top_dependencies: dependencies,
            top_errors: errors,
            recent_versions,
            service_count,
            active_service_count,
            health_trend,
            rankings,
            deployments,
            environments,
            available_environments,
            instrumentation,
        })
    }

    pub async fn services(
        &self,
        context: &ApmQueryContext,
    ) -> Result<PagedResponse<ServiceSummary>> {
        let bucket_query = self.bucket_query(context, BucketKind::Service);
        let catalog_query = self.catalog_query(context);

        let (buckets, catalog, versions, instances, thresholds) = tokio::try_join!(
            self.repository.query_buckets(&bucket_query),
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
            self.repository.list_instances(&catalog_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        let lines = self.org_settings(&context.org_id).await.health;
        let items = build_service_summaries(
            context,
            &catalog,
            &versions,
            Some(&instances),
            &buckets,
            &self.config.histogram,
            (&thresholds, &lines),
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

        let (catalog, versions, instances, groups, thresholds) = tokio::try_join!(
            self.repository.list_services(&catalog_query),
            self.repository.list_versions(&catalog_query),
            self.repository.list_instances(&catalog_query),
            self.repository.list_error_groups(&group_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        let lines = self.org_settings(&context.org_id).await.health;
        let mut services = build_service_summaries(
            context,
            &catalog,
            &versions,
            Some(&instances),
            &service_buckets,
            &self.config.histogram,
            (&thresholds, &lines),
        )?;

        if services.len() > 1 && (context.namespace.is_none() || context.environment.is_none()) {
            return Err(Error::invalid(
                "APM service identity is ambiguous; provide namespace and environment",
            ));
        }

        let service = services
            .pop()
            .ok_or_else(|| Error::not_found("APM service"))?;

        let transactions = transaction_summaries(
            context,
            &transaction_buckets,
            &self.config.histogram,
            &thresholds,
        )?;
        let dependencies =
            dependency_summaries(context, &dependency_buckets, &self.config.histogram)?;
        let mut errors = error_summaries(context, &groups, &error_buckets, &self.config.histogram)?;

        self.attach_issues(context, &mut errors).await?;

        let threshold = service.apdex_threshold;
        let red = red_from_buckets_at(&service_buckets, &self.config.histogram, threshold)?;

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
            trend: trend_from_buckets_at(&service_buckets, &self.config.histogram, threshold)?,
            latency_distribution: distribution_from_buckets(
                &service_buckets,
                &self.config.histogram,
            )?,
            service,
            red,
            transactions,
            dependencies,
            errors,
            versions: self
                .judged_versions(context, versions, &service_buckets, &thresholds)
                .await?,
        })
    }
}

pub(super) fn build_service_summaries(
    context: &ApmQueryContext,
    catalog: &[ServiceObservation],
    versions: &[VersionObservation],
    instances: Option<&[InstanceObservation]>,
    buckets: &[MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
    (thresholds, lines): (&ApdexThresholds, &HealthLines),
) -> Result<Vec<ServiceSummary>> {
    let mut grouped = HashMap::<ServiceIdentity, Vec<MergedBucket>>::new();
    let instances = instances.map(instances_by_service);

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
                entry_kinds: Vec::new(),
            }
        });
    }

    observations
        .into_values()
        .map(|observation| {
            let matching = grouped.remove(&observation.service).unwrap_or_default();
            let apdex_threshold_micros = thresholds.of(&observation.service);
            let red = red_from_buckets_at(&matching, histogram, apdex_threshold_micros)?;
            let (health, health_reasons) = assess_health(&red, lines);

            let observed = versions
                .iter()
                .filter(|version| version.service == observation.service)
                .collect::<Vec<_>>();

            let current = observed
                .iter()
                .max_by(|left, right| {
                    left.last_seen_at
                        .cmp(&right.last_seen_at)
                        .then_with(|| left.first_seen_at.cmp(&right.first_seen_at))
                })
                .map(|version| (version.version.clone(), version.first_seen_at));

            let mut service_versions = observed
                .iter()
                .map(|version| version.version.clone())
                .collect::<Vec<_>>();

            service_versions.sort();
            service_versions.dedup();

            let profile = instances.as_ref().map(|instances| {
                service_profile(
                    instances
                        .get(&observation.service)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    context.version.as_deref(),
                    &observation.entry_kinds,
                )
            });

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
                current_version: current.as_ref().map(|(version, _)| version.clone()),
                current_version_since: current.map(|(_, since)| since),
                health,
                health_reasons,
                sparkline: sparkline_from_buckets(&matching, histogram)?,
                red,
                apdex_threshold: apdex_threshold_micros,
                profile,
            })
        })
        .collect()
}

pub(super) fn version_summary(version: VersionObservation) -> VersionSummary {
    VersionSummary {
        service: version.service,
        version: version.version,
        first_seen_at: version.first_seen_at,
        last_seen_at: version.last_seen_at,
        observation_count: version.observation_count,
        red: None,
        verdict: None,
    }
}
