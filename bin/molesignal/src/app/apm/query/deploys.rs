// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Each version told against the version of the same service first seen
//! before it, from what both served in the time in view. A version did worse
//! when its error rate rose a point and by half again, or its P95 by a fifth
//! and 50 ms; better when either fell as far and nothing got worse; neither
//! is said when one of them served fewer requests than a comparison needs.

use std::collections::HashMap;

use super::{
    ApmQueryContext, ApmQueryService, RedSummary, VersionSummary, VersionVerdict,
    aggregate::red_from_measurements, apdex::ApdexThresholds, overview::version_summary,
    versions::red_delta,
};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, BucketMeasurements, HistogramSchema, MergedBucket,
        ServiceIdentity, VersionObservation,
    },
    shared::Result,
};

/// How far an error rate rises, in points and as a ratio, to be worse.
const ERROR_RATE_POINTS: f64 = 0.01;
const ERROR_RATE_RATIO: f64 = 1.5;
/// How far a P95 rises, as a ratio and in microseconds, to be worse.
const P95_RATIO: f64 = 1.2;
const P95_MICROS: u64 = 50_000;
/// What floating point may miss a threshold by.
const TOLERANCE: f64 = 1e-9;

impl ApmQueryService {
    /// The service's versions with their figures and verdicts, read from the
    /// service buckets of the whole service: `buckets` unless the page is
    /// narrowed to one version.
    pub(super) async fn judged_versions(
        &self,
        context: &ApmQueryContext,
        versions: Vec<VersionObservation>,
        buckets: &[MergedBucket],
        thresholds: &ApdexThresholds,
    ) -> Result<Vec<VersionSummary>> {
        let whole;
        let buckets = if context.version.is_some() {
            let mut every_version = context.clone();
            every_version.version = None;
            whole = self
                .repository
                .query_buckets(&self.bucket_query(&every_version, BucketKind::Service))
                .await?;
            whole.as_slice()
        } else {
            buckets
        };

        let mut judged = versions
            .iter()
            .cloned()
            .map(version_summary)
            .collect::<Vec<_>>();

        judge_versions(
            &mut judged,
            &versions,
            buckets,
            &self.config.histogram,
            thresholds,
            self.config.minimum_version_requests,
        )?;

        Ok(judged)
    }
}

/// `regressed`, `improved`, `neutral` or `insufficient_data` for `candidate`
/// against `baseline`, with what got worse: `error_rate`, `latency`.
pub(super) fn judge(
    baseline: &RedSummary,
    candidate: &RedSummary,
    minimum_requests: u64,
) -> (&'static str, Vec<&'static str>) {
    if baseline.request_count < minimum_requests.max(1)
        || candidate.request_count < minimum_requests.max(1)
    {
        return ("insufficient_data", Vec::new());
    }

    let worse = [
        ("error_rate", rate_worse(candidate, baseline)),
        ("latency", latency_worse(candidate, baseline)),
    ]
    .into_iter()
    .filter_map(|(what, holds)| holds.then_some(what))
    .collect::<Vec<_>>();

    let status = if !worse.is_empty() {
        "regressed"
    } else if rate_worse(baseline, candidate) || latency_worse(baseline, candidate) {
        "improved"
    } else {
        "neutral"
    };

    (status, worse)
}

fn rate_worse(worse: &RedSummary, better: &RedSummary) -> bool {
    worse.error_rate - better.error_rate + TOLERANCE >= ERROR_RATE_POINTS
        && worse.error_rate + TOLERANCE >= better.error_rate * ERROR_RATE_RATIO
}

fn latency_worse(worse: &RedSummary, better: &RedSummary) -> bool {
    match (worse.p95_time, better.p95_time) {
        (Some(worse), Some(better)) => {
            worse.saturating_sub(better) >= P95_MICROS
                && worse as f64 + TOLERANCE >= better as f64 * P95_RATIO
        }
        _ => false,
    }
}

/// Gives each of `judged` the figures it served in `buckets` and its verdict
/// against the version of its service first seen before it among `seen`.
pub(super) fn judge_versions(
    judged: &mut [VersionSummary],
    seen: &[VersionObservation],
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
    thresholds: &ApdexThresholds,
    minimum_requests: u64,
) -> Result<()> {
    let mut served = HashMap::<(&ServiceIdentity, &str), Vec<&BucketMeasurements>>::new();

    for bucket in buckets {
        if let BucketDimension::Service {
            service,
            version: Some(version),
        } = &bucket.dimension
        {
            served
                .entry((service, version.as_str()))
                .or_default()
                .push(&bucket.measurements);
        }
    }

    let red_of = |service: &ServiceIdentity, version: &str| -> Result<Option<RedSummary>> {
        served
            .get(&(service, version))
            .map(|measurements| {
                red_from_measurements(measurements.iter().copied(), schema, thresholds.of(service))
            })
            .transpose()
    };

    for summary in judged.iter_mut() {
        summary.red = red_of(&summary.service, &summary.version)?;

        let Some(previous) = previous_version(summary, seen) else {
            continue;
        };

        let baseline = red_of(&summary.service, &previous.version)?.unwrap_or_default();
        let candidate = summary.red.clone().unwrap_or_default();
        let (status, worse) = judge(&baseline, &candidate, minimum_requests);

        summary.verdict = Some(VersionVerdict {
            previous: previous.version.clone(),
            status,
            worse,
            delta: red_delta(&baseline, &candidate),
        });
    }

    Ok(())
}

/// The version of the same service first seen last before `summary`.
fn previous_version<'a>(
    summary: &VersionSummary,
    seen: &'a [VersionObservation],
) -> Option<&'a VersionObservation> {
    seen.iter()
        .filter(|version| {
            version.service == summary.service
                && version.version != summary.version
                && version.first_seen_at < summary.first_seen_at
        })
        .max_by(|left, right| {
            left.first_seen_at
                .cmp(&right.first_seen_at)
                .then_with(|| left.version.cmp(&right.version))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red(request_count: u64, error_rate: f64, p95: u64) -> RedSummary {
        RedSummary {
            request_count,
            error_rate,
            p95_time: Some(p95),
            ..RedSummary::default()
        }
    }

    #[test]
    fn errors_count_as_worse_only_a_point_and_half_again_up() {
        let before = red(1_000, 0.02, 100_000);

        assert_eq!(
            judge(&before, &red(1_000, 0.03, 100_000), 100),
            ("regressed", vec!["error_rate"]),
        );
        assert_eq!(
            judge(&before, &red(1_000, 0.025, 100_000), 100).0,
            "neutral"
        );
        assert_eq!(
            judge(&red(1_000, 0.20, 100_000), &red(1_000, 0.22, 100_000), 100).0,
            "neutral",
        );
        assert_eq!(
            judge(&before, &red(1_000, 0.0, 100_000), 100),
            ("improved", vec![])
        );
    }

    #[test]
    fn latency_counts_as_worse_only_a_fifth_and_50_ms_longer() {
        let before = red(1_000, 0.0, 200_000);

        assert_eq!(
            judge(&before, &red(1_000, 0.0, 250_000), 100),
            ("regressed", vec!["latency"]),
        );
        assert_eq!(judge(&before, &red(1_000, 0.0, 230_000), 100).0, "neutral");
        assert_eq!(
            judge(&red(1_000, 0.0, 10_000), &red(1_000, 0.0, 40_000), 100).0,
            "neutral",
        );
        assert_eq!(
            judge(&before, &red(1_000, 0.05, 400_000), 100),
            ("regressed", vec!["error_rate", "latency"]),
        );
    }

    #[test]
    fn too_few_requests_on_either_side_say_nothing() {
        let few = red(20, 0.5, 900_000);
        let many = red(1_000, 0.0, 100_000);

        assert_eq!(judge(&many, &few, 100).0, "insufficient_data");
        assert_eq!(judge(&few, &many, 100).0, "insufficient_data");
        assert_eq!(
            judge(&RedSummary::default(), &RedSummary::default(), 0).0,
            "insufficient_data"
        );
    }

    #[test]
    fn the_previous_version_is_the_one_first_seen_last_before() {
        let service = ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None);
        let other = ServiceIdentity::new(Some("shop"), Some("cart"), Some("prod"), None);
        let observed = |service: &ServiceIdentity, version: &str, first: i64| VersionObservation {
            org_id: crate::shared::ids::Id::from_string("org"),
            service: service.clone(),
            version: version.to_owned(),
            first_seen_at: crate::shared::time::TimestampMicros(first),
            last_seen_at: crate::shared::time::TimestampMicros(first + 10),
            observation_count: 1,
        };
        let seen = vec![
            observed(&service, "1.0", 100),
            observed(&service, "1.1", 200),
            observed(&other, "2.0", 250),
            observed(&service, "1.2", 300),
        ];
        let summary = super::super::overview::version_summary(seen[3].clone());

        assert_eq!(
            previous_version(&summary, &seen).map(|version| version.version.as_str()),
            Some("1.1"),
        );

        let first = super::super::overview::version_summary(seen[0].clone());

        assert!(previous_version(&first, &seen).is_none());
    }
}
