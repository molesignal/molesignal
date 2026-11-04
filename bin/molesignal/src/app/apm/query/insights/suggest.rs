// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where a service's time could be won back, from what it served and called
//! in the range: a dependency that takes most of its requests' time, the
//! same call made many times a request (an N+1 pattern), a database
//! statement slow enough to look at, and the endpoint most of its time goes
//! to. Suggestions, not faults: nothing here says the service is unhealthy.

use std::collections::HashMap;

use super::{
    super::{
        RedSummary, ServiceHealth,
        aggregate::{DEFAULT_APDEX_THRESHOLD_MICROS, red_from_measurements},
    },
    detect::MIN_REQUESTS,
    model::{Insight, InsightKind, InsightSeverity},
};
use crate::{
    domain::apm::{
        BucketDimension, BucketMeasurements, DependencyCategory, DependencyIdentity,
        HistogramSchema, MergedBucket, OTHER_DIMENSION, ServiceIdentity, TransactionIdentity,
    },
    shared::Result,
};

/// The share of a service's time a dependency, or an endpoint, takes to be named.
const TIME_SHARE: f64 = 0.4;
/// Calls a request, and calls in all, for a call to read as made in a loop.
const CALLS_PER_REQUEST: f64 = 10.0;
const REPEATED_CALLS: u64 = 200;
/// A statement's P95, and its calls, to be called slow.
const SLOW_STATEMENT_MICROS: u64 = 100_000;
const STATEMENT_CALLS: u64 = 20;
/// Requests an endpoint serves to be called where the time goes.
const HOT_ENDPOINT_REQUESTS: u64 = 50;
/// The slow statements named for each service.
const STATEMENTS_EACH: usize = 2;

/// What a service served in the range, and how it stands.
pub(super) struct Served<'a> {
    pub red: &'a RedSummary,
    pub health: ServiceHealth,
}

/// The suggestions for each service of `served`, from its `transactions` and
/// `dependencies` buckets of the range.
pub(super) fn suggestions(
    served: &HashMap<ServiceIdentity, Served<'_>>,
    transactions: &[MergedBucket],
    dependencies: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<Vec<Insight>> {
    let calls = grouped(dependencies, |dimension| match dimension {
        BucketDimension::Dependency {
            service,
            dependency,
            ..
        } if dependency.target != OTHER_DIMENSION => Some((service, dependency.clone())),
        _ => None,
    });
    let targets = grouped(dependencies, |dimension| match dimension {
        BucketDimension::Dependency {
            service,
            dependency,
            ..
        } if dependency.target != OTHER_DIMENSION => Some((
            service,
            DependencyIdentity {
                category: dependency.category,
                target: dependency.target.clone(),
                operation: None,
            },
        )),
        _ => None,
    });
    let endpoints = grouped(transactions, |dimension| match dimension {
        BucketDimension::Transaction {
            service,
            transaction,
            ..
        } if transaction.name != OTHER_DIMENSION => Some((service, transaction.clone())),
        _ => None,
    });

    let mut found = Vec::new();

    for (service, measured) in served {
        if measured.red.request_count < MIN_REQUESTS {
            continue;
        }

        let targets = summed(targets.get(service), schema)?;
        let calls = summed(calls.get(service), schema)?;
        let endpoints = summed(endpoints.get(service), schema)?;

        found.extend(slow_dependency(service, measured, &targets));
        found.extend(repeated_call(service, measured, &calls));
        found.extend(slow_statements(service, &calls));
        found.extend(hot_endpoint(service, measured, &endpoints));
    }

    Ok(found)
}

type Groups<'a, K> = HashMap<ServiceIdentity, HashMap<K, Vec<&'a BucketMeasurements>>>;

/// Buckets of `buckets` by service, then by what `key` reads from their dimension.
fn grouped<'a, K: std::hash::Hash + Eq>(
    buckets: &'a [MergedBucket],
    key: impl Fn(&'a BucketDimension) -> Option<(&'a ServiceIdentity, K)>,
) -> Groups<'a, K> {
    let mut groups = Groups::new();

    for bucket in buckets {
        if let Some((service, key)) = key(&bucket.dimension) {
            groups
                .entry(service.clone())
                .or_default()
                .entry(key)
                .or_default()
                .push(&bucket.measurements);
        }
    }

    groups
}

/// The figures of each group of one service.
fn summed<K: Clone>(
    groups: Option<&HashMap<K, Vec<&BucketMeasurements>>>,
    schema: &HistogramSchema,
) -> Result<Vec<(K, RedSummary)>> {
    groups
        .into_iter()
        .flatten()
        .map(|(key, measurements)| {
            Ok((
                key.clone(),
                red_from_measurements(
                    measurements.iter().copied(),
                    schema,
                    DEFAULT_APDEX_THRESHOLD_MICROS,
                )?,
            ))
        })
        .collect()
}

fn share(part: &RedSummary, whole: &RedSummary) -> f64 {
    (part.duration_sum as f64 / whole.duration_sum.max(1) as f64).min(1.0)
}

fn suggestion(
    kind: InsightKind,
    service: &ServiceIdentity,
    value: f64,
    red: &RedSummary,
) -> Insight {
    let mut insight = Insight::new(kind, InsightSeverity::Info, service.clone(), value);
    insight.calls = Some(red.request_count);
    insight.p95_time = red.p95_time;
    insight
}

/// The dependency that takes the largest share of the service's time, past
/// [`TIME_SHARE`]; a warning while the service is not healthy.
fn slow_dependency(
    service: &ServiceIdentity,
    served: &Served<'_>,
    targets: &[(DependencyIdentity, RedSummary)],
) -> Option<Insight> {
    let (target, red) = targets
        .iter()
        .filter(|(_, red)| red.request_count >= MIN_REQUESTS)
        .max_by(|left, right| left.1.duration_sum.cmp(&right.1.duration_sum))?;
    let part = share(red, served.red);

    if part < TIME_SHARE {
        return None;
    }

    let mut insight = suggestion(InsightKind::SlowDependency, service, part, red);
    insight.dependency = Some(target.clone());

    if matches!(
        served.health,
        ServiceHealth::Warning | ServiceHealth::Critical
    ) {
        insight.severity = InsightSeverity::Warning;
    }

    Some(insight)
}

/// The call made the most times a request, from [`CALLS_PER_REQUEST`] on:
/// what is done in a loop that one call could do.
fn repeated_call(
    service: &ServiceIdentity,
    served: &Served<'_>,
    calls: &[(DependencyIdentity, RedSummary)],
) -> Option<Insight> {
    let requests = served.red.request_count.max(1) as f64;
    let (call, red) = calls
        .iter()
        .filter(|(call, red)| {
            call.operation.is_some()
                && call.category != DependencyCategory::Messaging
                && red.request_count >= REPEATED_CALLS
        })
        .max_by(|left, right| left.1.request_count.cmp(&right.1.request_count))?;
    let each = red.request_count as f64 / requests;

    (each >= CALLS_PER_REQUEST).then(|| {
        let mut insight = suggestion(InsightKind::RepeatedCall, service, each, red);
        insight.dependency = Some(call.clone());
        insight
    })
}

/// The statements, written as more than one word, that are slowest in all,
/// from a P95 of [`SLOW_STATEMENT_MICROS`] on.
fn slow_statements(
    service: &ServiceIdentity,
    calls: &[(DependencyIdentity, RedSummary)],
) -> Vec<Insight> {
    let mut slow = calls
        .iter()
        .filter(|(call, red)| {
            call.category == DependencyCategory::Database
                && call
                    .operation
                    .as_deref()
                    .is_some_and(|statement| statement.trim().contains(char::is_whitespace))
                && red.request_count >= STATEMENT_CALLS
                && red.p95_time.is_some_and(|p95| p95 >= SLOW_STATEMENT_MICROS)
        })
        .collect::<Vec<_>>();

    slow.sort_by_key(|(_, red)| std::cmp::Reverse(red.duration_sum));

    slow.into_iter()
        .take(STATEMENTS_EACH)
        .map(|(call, red)| {
            let p95 = red.p95_time.unwrap_or_default() as f64;
            let mut insight = suggestion(InsightKind::SlowQuery, service, p95, red);
            insight.dependency = Some(call.clone());
            insight
        })
        .collect()
}

/// The endpoint most of the service's time goes to, from [`TIME_SHARE`] on,
/// among two or more that served.
fn hot_endpoint(
    service: &ServiceIdentity,
    served: &Served<'_>,
    endpoints: &[(TransactionIdentity, RedSummary)],
) -> Option<Insight> {
    let serving = endpoints
        .iter()
        .filter(|(_, red)| red.request_count > 0)
        .collect::<Vec<_>>();
    let whole = serving
        .iter()
        .map(|(_, red)| red.duration_sum)
        .sum::<u64>()
        .max(served.red.duration_sum);
    let (endpoint, red) = serving
        .iter()
        .max_by(|left, right| left.1.duration_sum.cmp(&right.1.duration_sum))?;
    let part = red.duration_sum as f64 / whole.max(1) as f64;

    (serving.len() >= 2 && red.request_count >= HOT_ENDPOINT_REQUESTS && part >= TIME_SHARE).then(
        || {
            let mut insight = suggestion(InsightKind::HotEndpoint, service, part, red);
            insight.transaction = Some(endpoint.clone());
            insight
        },
    )
}
