// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The services set against each other: who ranks first, where they run,
//! what they report about themselves and what they deployed within the range.

use std::collections::{BTreeMap, HashMap};

use super::{
    ApmQueryContext, EnvironmentShare, InstrumentationQuality, RedSummary, SdkShare, ServiceRank,
    ServiceRankings, ServiceSummary, VersionSummary, aggregate::red_from_buckets,
    overview::version_summary,
};
use crate::{
    domain::apm::{
        BucketDimension, DEFAULT_DEPLOYMENT_ENVIRONMENT, HistogramSchema, MergedBucket,
        ServiceIdentity, VersionObservation,
    },
    shared::Result,
};

/// How many services a ranking lists.
const RANKING_SIZE: usize = 5;

/// The fewest requests a service needs for its error rate or latency to
/// rank: below it one slow or failed request decides.
const MIN_REQUESTS: u64 = 20;

/// How many deployments the overview lists.
const DEPLOYMENT_LIMIT: usize = 20;

pub(super) fn rankings(services: &[ServiceSummary]) -> ServiceRankings {
    let measured = services
        .iter()
        .filter(|service| service.red.request_count >= MIN_REQUESTS)
        .collect::<Vec<_>>();

    let mut slowest = measured
        .iter()
        .filter(|service| service.red.p95_time.is_some())
        .copied()
        .collect::<Vec<_>>();
    slowest.sort_by(|left, right| {
        right
            .red
            .p95_time
            .cmp(&left.red.p95_time)
            .then_with(|| left.service.stable_key().cmp(&right.service.stable_key()))
    });

    let mut most_errors = measured
        .iter()
        .filter(|service| service.red.error_count > 0)
        .copied()
        .collect::<Vec<_>>();
    most_errors.sort_by(|left, right| {
        right
            .red
            .error_rate
            .total_cmp(&left.red.error_rate)
            .then_with(|| left.service.stable_key().cmp(&right.service.stable_key()))
    });

    let mut busiest = services
        .iter()
        .filter(|service| service.red.request_count > 0)
        .collect::<Vec<_>>();
    busiest.sort_by(|left, right| {
        right
            .red
            .request_count
            .cmp(&left.red.request_count)
            .then_with(|| left.service.stable_key().cmp(&right.service.stable_key()))
    });

    ServiceRankings {
        slowest: ranked(slowest),
        most_errors: ranked(most_errors),
        busiest: ranked(busiest),
    }
}

fn ranked(services: Vec<&ServiceSummary>) -> Vec<ServiceRank> {
    services
        .into_iter()
        .take(RANKING_SIZE)
        .map(|service| ServiceRank {
            service: service.service.clone(),
            health: service.health,
            request_count: service.red.request_count,
            error_rate: service.red.error_rate,
            p95_time: service.red.p95_time,
        })
        .collect()
}

/// The figures of each service in `buckets`.
pub(super) fn red_by_service(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<HashMap<ServiceIdentity, RedSummary>> {
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
        .map(|(service, values)| Ok((service, red_from_buckets(&values, schema)?)))
        .collect()
}

/// Each environment the services run in, the busiest first.
pub(super) fn environments(services: &[ServiceSummary]) -> Vec<EnvironmentShare> {
    let mut grouped = BTreeMap::<String, EnvironmentShare>::new();

    for service in services {
        let share = grouped
            .entry(service.service.environment.clone())
            .or_insert_with(|| EnvironmentShare {
                environment: service.service.environment.clone(),
                service_count: 0,
                request_count: 0,
            });
        share.service_count += 1;
        share.request_count = share
            .request_count
            .saturating_add(service.red.request_count);
    }

    let mut shares = grouped.into_values().collect::<Vec<_>>();
    shares.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.environment.cmp(&right.environment))
    });
    shares
}

/// What the services say about how they are instrumented, and what they leave out.
pub(super) fn instrumentation(services: &[ServiceSummary]) -> InstrumentationQuality {
    let mut sdks = BTreeMap::<(Option<String>, Option<String>, Option<String>), u64>::new();
    let mut quality = InstrumentationQuality::default();

    for service in services {
        let instrumentation = &service.instrumentation;
        *sdks
            .entry((
                instrumentation.runtime_language.clone(),
                instrumentation.telemetry_sdk_name.clone(),
                instrumentation.telemetry_sdk_version.clone(),
            ))
            .or_default() += 1;

        if service.service.environment == DEFAULT_DEPLOYMENT_ENVIRONMENT {
            quality.without_environment += 1;
        }

        if service.versions.is_empty() {
            quality.without_version += 1;
        }

        if instrumentation.recent_instance_count == 0 {
            quality.without_instance += 1;
        }
    }

    quality.sdks = sdks
        .into_iter()
        .map(
            |((language, sdk_name, sdk_version), service_count)| SdkShare {
                language,
                sdk_name,
                sdk_version,
                service_count,
            },
        )
        .collect();
    quality
        .sdks
        .sort_by_key(|sdk| std::cmp::Reverse(sdk.service_count));
    quality
}

/// The versions first seen within the range, the newest first.
pub(super) fn deployments(
    context: &ApmQueryContext,
    versions: &[VersionObservation],
) -> Vec<VersionSummary> {
    let mut deployed = versions
        .iter()
        .filter(|version| {
            version.first_seen_at >= context.range.start
                && version.first_seen_at <= context.range.end
        })
        .cloned()
        .collect::<Vec<_>>();

    deployed.sort_by_key(|version| std::cmp::Reverse(version.first_seen_at));
    deployed.truncate(DEPLOYMENT_LIMIT);
    deployed.into_iter().map(version_summary).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::apm::query::{InstrumentationSummary, ServiceHealth, SignalFilterHandle},
        shared::time::TimestampMicros,
    };

    fn summary(
        name: &str,
        environment: &str,
        request_count: u64,
        error_rate: f64,
        p95: Option<u64>,
    ) -> ServiceSummary {
        let service = ServiceIdentity::new(Some("shop"), Some(name), Some(environment), None);

        ServiceSummary {
            traces: SignalFilterHandle {
                namespace: service.namespace.clone(),
                service: service.name.clone(),
                environment: service.environment.clone(),
                version: None,
                transaction: None,
                dependency: None,
                error_fingerprint: None,
                from: TimestampMicros(0),
                to: TimestampMicros(1),
            },
            service,
            first_seen_at: TimestampMicros(0),
            last_seen_at: TimestampMicros(1),
            instrumentation: InstrumentationSummary {
                runtime_language: Some("rust".into()),
                telemetry_sdk_name: Some("opentelemetry".into()),
                telemetry_sdk_version: Some("0.31".into()),
                recent_instance_count: 1,
            },
            versions: vec!["1.0".into()],
            current_version: None,
            current_version_since: None,
            red: RedSummary {
                request_count,
                error_count: (request_count as f64 * error_rate) as u64,
                error_rate,
                p95_time: p95,
                ..RedSummary::default()
            },
            apdex_threshold: 500_000,
            sparkline: Vec::new(),
            health: ServiceHealth::Healthy,
            health_reasons: Vec::new(),
            profile: None,
        }
    }

    #[test]
    fn ranks_services_with_enough_requests_to_tell() {
        let services = [
            summary("cart", "prod", 500, 0.01, Some(80_000)),
            summary("pay", "prod", 50, 0.20, Some(900_000)),
            summary("rare", "prod", 3, 1.0, Some(9_000_000)),
        ];
        let rankings = rankings(&services);

        assert_eq!(
            rankings
                .slowest
                .iter()
                .map(|rank| rank.service.name.as_str())
                .collect::<Vec<_>>(),
            ["pay", "cart"]
        );
        assert_eq!(rankings.most_errors[0].service.name, "pay");
        assert_eq!(
            rankings
                .busiest
                .iter()
                .map(|rank| rank.service.name.as_str())
                .collect::<Vec<_>>(),
            ["cart", "pay", "rare"]
        );
    }

    #[test]
    fn counts_environments_and_what_services_leave_out() {
        let mut bare = summary("bare", DEFAULT_DEPLOYMENT_ENVIRONMENT, 10, 0.0, None);
        bare.versions.clear();
        bare.instrumentation.recent_instance_count = 0;
        let services = [
            summary("cart", "prod", 500, 0.0, None),
            summary("pay", "prod", 50, 0.0, None),
            bare,
        ];

        let shares = environments(&services);
        assert_eq!(shares[0].environment, "prod");
        assert_eq!(shares[0].service_count, 2);
        assert_eq!(shares[0].request_count, 550);

        let quality = instrumentation(&services);
        assert_eq!(
            (
                quality.without_environment,
                quality.without_version,
                quality.without_instance
            ),
            (1, 1, 1)
        );
        assert_eq!(quality.sdks[0].service_count, 3);
    }
}
