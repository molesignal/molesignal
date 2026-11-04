// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What APM finds worth a look in the range without being asked: services
//! whose errors, latency or traffic moved against the stretch before and the
//! same stretch a week before, versions that did worse than the ones before
//! them, and where time could be won back; each move with the changes near
//! when it began. Read for the services in view, or for one of them.

use std::collections::{HashMap, HashSet};

use self::{
    detect::{ServiceFigures, Stretch},
    suggest::Served,
};
use super::{
    ApmQueryContext, ApmQueryService, RedSummary, RedTrendPoint, VersionSummary,
    aggregate::{red_from_buckets_at, trend_from_buckets_at},
    common::overflow_dimensions,
    health::assess_health,
    standings::red_by_service,
};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, DependencyCategory, DeploymentEvent, DeploymentQuery,
        MergedBucket, QueryResolution, ServiceIdentity,
    },
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod changes;
mod detect;
mod model;
mod suggest;
#[cfg(test)]
mod tests;

pub use model::*;

const WEEK_MICROS: i64 = 7 * 86_400_000_000;
/// The most insights a response lists.
const MOST_INSIGHTS: usize = 30;
/// The most changes read around the range.
const MOST_CHANGES_READ: usize = 200;

impl ApmQueryService {
    /// The insights of `context`'s range, for the services in view.
    pub async fn insights(&self, context: &ApmQueryContext) -> Result<InsightsResponse> {
        let mut context = context.clone();
        // Insights are of a service as a whole, whichever version is in view.
        context.version = None;

        let length = context.range.duration_micros().max(1);
        let (now, now_covered) = self.held_context(&context, 0);
        let (before, before_covered) = self.held_context(&context, length);
        let (week, week_covered) = self.held_context(&context, WEEK_MICROS);

        let service_query = self.bucket_query(&now, BucketKind::Service);
        let transaction_query = self.bucket_query(&now, BucketKind::Transaction);
        let dependency_query = self.bucket_query(&now, BucketKind::Dependency);
        let before_query = self.bucket_query(&before, BucketKind::Service);
        let week_query = self.bucket_query(&week, BucketKind::Service);
        let catalog_query = self.catalog_query(&context);

        let (
            service_buckets,
            transaction_buckets,
            dependency_buckets,
            before_buckets,
            week_buckets,
            versions,
            thresholds,
            reported,
        ) = tokio::try_join!(
            self.repository.query_buckets(&service_query),
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&dependency_query),
            self.repository.query_buckets(&before_query),
            self.repository.query_buckets(&week_query),
            self.repository.list_versions(&catalog_query),
            self.apdex_thresholds(&context.org_id),
            self.changes_around(&context),
        )?;

        let schema = &self.config.histogram;
        let lines = self.org_settings(&context.org_id).await.health;
        let seconds = |covered: i64| covered as f64 / 1_000_000.0;

        let red_now = red_by_service(&service_buckets, schema)?;
        let red_before = red_by_service(&before_buckets, schema)?;
        let red_week = red_by_service(&week_buckets, schema)?;
        let trends = trends_by_service(&service_buckets, schema, thresholds.default_micros())?;
        let bucket_seconds = match now.resolution {
            QueryResolution::Hour => 3_600.0,
            _ => 60.0,
        };

        let silent = RedSummary::default();
        let services = red_now
            .keys()
            .chain(red_before.keys())
            .collect::<HashSet<_>>();
        let figures = services
            .iter()
            .map(|service| ServiceFigures {
                service,
                now: Stretch {
                    red: red_now.get(*service).unwrap_or(&silent),
                    seconds: seconds(now_covered),
                },
                previous: red_before.get(*service).map(|red| Stretch {
                    red,
                    seconds: seconds(before_covered),
                }),
                last_week: red_week.get(*service).map(|red| Stretch {
                    red,
                    seconds: seconds(week_covered),
                }),
                trend: trends.get(*service).map_or(&[], Vec::as_slice),
                bucket_seconds,
            })
            .collect::<Vec<_>>();

        let served = red_now
            .iter()
            .map(|(service, red)| {
                (
                    service.clone(),
                    Served {
                        red,
                        health: assess_health(red, &lines).0,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let callees = callees_by_service(&dependency_buckets);
        let no_callees = HashSet::new();

        let mut insights = detect::anomalies(&figures, &lines);
        let judged = self
            .judged_versions(&now, versions.clone(), &service_buckets, &thresholds)
            .await?;
        insights.extend(regressions(&judged, context.range));

        for insight in &mut insights {
            if let Some(around) = insight.since {
                let callees = callees.get(&insight.service).unwrap_or(&no_callees);
                changes::attach(insight, around, &reported, &versions, callees);
            }
        }

        insights.extend(suggest::suggestions(
            &served,
            &transaction_buckets,
            &dependency_buckets,
            schema,
        )?);
        insights.sort_by(|left, right| {
            left.severity
                .cmp(&right.severity)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        insights.truncate(MOST_INSIGHTS);

        let threshold = thresholds.default_micros();
        let total = |buckets: &[MergedBucket]| red_from_buckets_at(buckets, schema, threshold);
        let figures_of = |red: RedSummary, covered: i64| {
            (red.request_count > 0).then(|| {
                BaselineFigures::of(&red, seconds(covered), length as f64 / covered as f64)
            })
        };

        Ok(InsightsResponse {
            meta: self
                .response_meta(&context, overflow_dimensions(&service_buckets), false)
                .await?,
            now: figures_of(total(&service_buckets)?, now_covered).unwrap_or_default(),
            previous: figures_of(total(&before_buckets)?, before_covered),
            last_week: figures_of(total(&week_buckets)?, week_covered),
            insights,
        })
    }

    /// `context` moved back by `micros`, as the buckets still hold it, and
    /// how long the buckets read cover.
    fn held_context(&self, context: &ApmQueryContext, micros: i64) -> (ApmQueryContext, i64) {
        let range = TimeRange::new(
            TimestampMicros(context.range.start.0 - micros),
            TimestampMicros(context.range.end.0 - micros),
        );
        let (range, resolution, covered) = self.held_range(range, context.resolution);
        let mut held = context.clone();
        held.range = range;
        held.resolution = resolution;

        (held, covered)
    }

    /// The changes pipelines and tools reported around the range, of every
    /// service: a service's move is told by the changes of what it calls too.
    async fn changes_around(&self, context: &ApmQueryContext) -> Result<Vec<DeploymentEvent>> {
        let Some(catalog) = &self.catalog else {
            return Ok(Vec::new());
        };

        catalog
            .list_deployments(
                &context.org_id,
                &DeploymentQuery {
                    namespace: None,
                    service: None,
                    environment: context.environment.clone(),
                    from: Some(TimestampMicros(context.range.start.0 - changes::BEFORE)),
                    to: Some(TimestampMicros(context.range.end.0 + changes::AFTER)),
                    limit: MOST_CHANGES_READ,
                },
            )
            .await
    }
}

/// Each service's trend in `buckets`.
fn trends_by_service(
    buckets: &[MergedBucket],
    schema: &crate::domain::apm::HistogramSchema,
    apdex_threshold_micros: u64,
) -> Result<HashMap<ServiceIdentity, Vec<RedTrendPoint>>> {
    let mut grouped = HashMap::<ServiceIdentity, Vec<MergedBucket>>::new();

    for bucket in buckets {
        if let BucketDimension::Service { service, .. } = &bucket.dimension {
            grouped
                .entry(service.clone())
                .or_default()
                .push(bucket.clone());
        }
    }

    grouped
        .into_iter()
        .map(|(service, values)| {
            Ok((
                service,
                trend_from_buckets_at(&values, schema, apdex_threshold_micros)?,
            ))
        })
        .collect()
}

/// The services each service calls, by name.
fn callees_by_service(dependencies: &[MergedBucket]) -> HashMap<ServiceIdentity, HashSet<String>> {
    let mut callees = HashMap::<ServiceIdentity, HashSet<String>>::new();

    for bucket in dependencies {
        if let BucketDimension::Dependency {
            service,
            dependency,
            ..
        } = &bucket.dimension
            && dependency.category == DependencyCategory::Service
        {
            callees
                .entry(service.clone())
                .or_default()
                .insert(dependency.target.clone());
        }
    }

    callees
}

/// The versions first seen in `range` that did worse than the one before them.
fn regressions(judged: &[VersionSummary], range: TimeRange) -> Vec<Insight> {
    judged
        .iter()
        .filter(|version| range.contains(version.first_seen_at))
        .filter_map(|version| {
            let verdict = version
                .verdict
                .as_ref()
                .filter(|verdict| verdict.status == "regressed")?;
            let red = version.red.as_ref()?;
            let latency = verdict.worse.first() == Some(&"latency");
            let (metric, value, previous) = if latency {
                let p95 = red.p95_time? as f64;
                (
                    "p95",
                    p95,
                    verdict
                        .delta
                        .p95_absolute_time
                        .map(|delta| p95 - delta as f64),
                )
            } else {
                (
                    "error_rate",
                    red.error_rate,
                    Some(red.error_rate - verdict.delta.error_rate_absolute),
                )
            };

            let mut insight = Insight::new(
                InsightKind::Regression,
                InsightSeverity::Warning,
                version.service.clone(),
                value,
            );
            insight.metric = Some(metric);
            insight.previous = previous;
            insight.since = Some(version.first_seen_at);
            insight.version = Some(version.version.clone());
            insight.previous_version = Some(verdict.previous.clone());

            Some(insight)
        })
        .collect()
}
