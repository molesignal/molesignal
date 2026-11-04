// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the figures of a service say about it: healthy, a warning or
//! critical, which figures made it so, and how many services were in each
//! state over the range.

use std::collections::{BTreeMap, HashMap};

use super::{
    ApmQueryContext, HealthReason, HealthTrendPoint, RedSummary, ServiceHealth,
    ServiceHealthCounts, ServiceSummary, aggregate::red_from_buckets,
};
use crate::{
    domain::apm::{
        BucketDimension, HealthLines, HistogramSchema, MergedBucket, QueryResolution,
        ServiceIdentity,
    },
    shared::{Result, time::TimestampMicros},
};

/// The most slots a health trend is cut into, whatever the range.
const HEALTH_TREND_SLOTS: i64 = 48;

/// The state of a service by its figures, and each figure past its line.
pub(super) fn assess_health(
    red: &RedSummary,
    lines: &HealthLines,
) -> (ServiceHealth, Vec<HealthReason>) {
    if red.request_count == 0 {
        return (ServiceHealth::NoTraffic, Vec::new());
    }

    let mut reasons = Vec::new();

    if let Some(reason) = reason(
        "error_rate",
        red.error_rate,
        lines.error_rate_warning,
        lines.error_rate_critical,
    ) {
        reasons.push(reason);
    }

    if let Some(p95) = red.p95_time
        && let Some(reason) = reason(
            "p95",
            p95 as f64,
            lines.p95_warning_time as f64,
            lines.p95_critical_time as f64,
        )
    {
        reasons.push(reason);
    }

    let health = if reasons
        .iter()
        .any(|reason| reason.level == ServiceHealth::Critical)
    {
        ServiceHealth::Critical
    } else if reasons.is_empty() {
        ServiceHealth::Healthy
    } else {
        ServiceHealth::Warning
    };

    (health, reasons)
}

fn reason(metric: &'static str, value: f64, warning: f64, critical: f64) -> Option<HealthReason> {
    let (level, threshold) = if value >= critical {
        (ServiceHealth::Critical, critical)
    } else if value >= warning {
        (ServiceHealth::Warning, warning)
    } else {
        return None;
    };

    Some(HealthReason {
        metric,
        level,
        value,
        threshold,
    })
}

pub(super) fn health_counts(services: &[ServiceSummary]) -> ServiceHealthCounts {
    let mut counts = ServiceHealthCounts::default();

    for service in services {
        count(&mut counts, service.health);
    }

    counts
}

fn count(counts: &mut ServiceHealthCounts, health: ServiceHealth) {
    match health {
        ServiceHealth::Healthy => counts.healthy += 1,
        ServiceHealth::Warning => counts.warning += 1,
        ServiceHealth::Critical => counts.critical += 1,
        ServiceHealth::NoTraffic => counts.no_traffic += 1,
    }
}

/// How many of `services` were in each state, a slot of the range at a time.
/// A slot is as long as the range over [`HEALTH_TREND_SLOTS`], and never
/// shorter than a bucket; a service with no request in a slot has no traffic
/// there.
pub(super) fn health_trend(
    context: &ApmQueryContext,
    services: &[ServiceIdentity],
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
    lines: &HealthLines,
) -> Result<Vec<HealthTrendPoint>> {
    let start = context.range.start.0;
    let length = (context.range.end.0 - start).max(1);
    let step = match context.resolution {
        QueryResolution::Hour => 3_600_000_000,
        _ => 60_000_000,
    };
    let slot = ((length + HEALTH_TREND_SLOTS - 1) / HEALTH_TREND_SLOTS).max(step);
    let slot = (slot + step - 1) / step * step;

    let mut cells = HashMap::<(ServiceIdentity, i64), Vec<MergedBucket>>::new();

    for bucket in buckets {
        if let BucketDimension::Service { service, .. } = &bucket.dimension {
            let index = (bucket.bucket_at.0 - start).max(0) / slot;
            cells
                .entry((service.clone(), index))
                .or_default()
                .push(bucket.clone());
        }
    }

    let slots = (length + slot - 1) / slot;
    let mut trend = BTreeMap::<i64, ServiceHealthCounts>::new();

    for index in 0..slots {
        let counts = trend.entry(index).or_default();

        for service in services {
            let health = match cells.get(&(service.clone(), index)) {
                Some(values) => assess_health(&red_from_buckets(values, schema)?, lines).0,
                None => ServiceHealth::NoTraffic,
            };

            count(counts, health);
        }
    }

    Ok(trend
        .into_iter()
        .map(|(index, counts)| HealthTrendPoint {
            bucket_at: TimestampMicros(start + index * slot),
            counts,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red(request_count: u64, error_rate: f64, p95_micros: Option<u64>) -> RedSummary {
        RedSummary {
            request_count,
            error_rate,
            p95_time: p95_micros,
            ..RedSummary::default()
        }
    }

    #[test]
    fn says_which_figures_made_a_service_a_warning_or_critical() {
        let lines = HealthLines::default();

        let (health, reasons) = assess_health(&red(100, 0.0, Some(100_000)), &lines);
        assert_eq!(health, ServiceHealth::Healthy);
        assert!(reasons.is_empty());

        let (health, reasons) = assess_health(&red(100, 0.05, Some(100_000)), &lines);
        assert_eq!(health, ServiceHealth::Warning);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].metric, "error_rate");
        assert_eq!(reasons[0].threshold, lines.error_rate_warning);

        let (health, reasons) = assess_health(&red(100, 0.05, Some(3_000_000)), &lines);
        assert_eq!(health, ServiceHealth::Critical);
        assert_eq!(
            reasons
                .iter()
                .map(|reason| reason.metric)
                .collect::<Vec<_>>(),
            ["error_rate", "p95"]
        );
        assert_eq!(reasons[1].level, ServiceHealth::Critical);

        assert_eq!(
            assess_health(&red(0, 0.0, None), &lines).0,
            ServiceHealth::NoTraffic
        );
    }

    #[test]
    fn an_organization_draws_its_own_lines() {
        let strict = HealthLines {
            error_rate_warning: 0.001,
            error_rate_critical: 0.01,
            p95_warning_time: 50_000,
            p95_critical_time: 200_000,
        };

        let (health, reasons) = assess_health(&red(100, 0.005, Some(100_000)), &strict);

        assert_eq!(health, ServiceHealth::Warning);
        assert_eq!(
            reasons
                .iter()
                .map(|reason| reason.threshold)
                .collect::<Vec<_>>(),
            [0.001, 50_000.0]
        );
    }
}
