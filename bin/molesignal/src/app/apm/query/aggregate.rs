// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use super::{RedSparkPoint, RedSummary, RedTrendPoint};
use crate::{
    domain::apm::{
        BucketMeasurements, HistogramBucket, HistogramSchema, LatencyHistogram, MergedBucket,
        count_status,
    },
    shared::{Result, time::TimestampMicros},
};

/// The Apdex threshold T until a service sets its own: a request within T
/// satisfies, one within 4T is tolerated.
pub const DEFAULT_APDEX_THRESHOLD_MICROS: u64 = 500_000;

pub(super) fn red_from_buckets(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<RedSummary> {
    red_from_buckets_at(buckets, schema, DEFAULT_APDEX_THRESHOLD_MICROS)
}

/// The summary with Apdex read against `apdex_threshold`.
pub(super) fn red_from_buckets_at(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
    apdex_threshold_micros: u64,
) -> Result<RedSummary> {
    red_from_measurements(
        buckets.iter().map(|bucket| &bucket.measurements),
        schema,
        apdex_threshold_micros,
    )
}

pub(super) fn trend_from_buckets(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<Vec<RedTrendPoint>> {
    trend_from_buckets_at(buckets, schema, DEFAULT_APDEX_THRESHOLD_MICROS)
}

/// The trend with Apdex read against `apdex_threshold`.
pub(super) fn trend_from_buckets_at(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
    apdex_threshold_micros: u64,
) -> Result<Vec<RedTrendPoint>> {
    let mut grouped = BTreeMap::<TimestampMicros, Vec<&BucketMeasurements>>::new();

    for bucket in buckets {
        grouped
            .entry(bucket.bucket_at)
            .or_default()
            .push(&bucket.measurements);
    }

    grouped
        .into_iter()
        .map(|(bucket_at, measurements)| {
            Ok(RedTrendPoint {
                bucket_at,
                red: red_from_measurements(measurements, schema, apdex_threshold_micros)?,
            })
        })
        .collect()
}

/// The trend as a list draws it beside each row: counts and P95 a bucket at a time.
pub(super) fn sparkline_from_buckets<'a>(
    buckets: impl IntoIterator<Item = &'a MergedBucket>,
    schema: &HistogramSchema,
) -> Result<Vec<RedSparkPoint>> {
    let mut grouped = BTreeMap::<TimestampMicros, Vec<&BucketMeasurements>>::new();

    for bucket in buckets {
        grouped
            .entry(bucket.bucket_at)
            .or_default()
            .push(&bucket.measurements);
    }

    grouped
        .into_iter()
        .map(|(bucket_at, measurements)| {
            let request_count = measurements.iter().map(|item| item.request_count).sum();
            let error_count = measurements.iter().map(|item| item.error_count).sum();
            let (histogram, _) = merged_latency(measurements, schema)?;

            Ok(RedSparkPoint {
                bucket_at,
                request_count,
                error_count,
                p95_time: histogram.quantile(schema, 0.95)?,
            })
        })
        .collect()
}

/// How the latencies of the buckets are spread: the buckets that hold any.
pub(super) fn distribution_from_buckets(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<Vec<HistogramBucket>> {
    let (histogram, _) = merged_latency(buckets.iter().map(|bucket| &bucket.measurements), schema)?;

    histogram.filled_buckets(schema)
}

/// The latencies of all the measurements in one histogram of `schema`. One
/// written with another layout is converted when its layout can be told from
/// it; one that cannot is left out, and the result says so.
pub(super) fn merged_latency<'a>(
    measurements: impl IntoIterator<Item = &'a BucketMeasurements>,
    schema: &HistogramSchema,
) -> Result<(LatencyHistogram, bool)> {
    let mut histogram = LatencyHistogram::empty(schema);
    let mut partial = false;

    for measurement in measurements {
        let latency = &measurement.latency;

        if latency.schema_version == schema.version
            && latency.counts.len() == schema.upper_bounds_time.len()
        {
            histogram.merge(latency)?;
            continue;
        }

        match HistogramSchema::of_stored(latency.schema_version, latency.counts.len()) {
            Some(stored) => histogram.merge(&latency.converted(&stored, schema)?)?,
            None => partial = true,
        }
    }

    Ok((histogram, partial))
}

pub(super) fn red_from_measurements<'a>(
    measurements: impl IntoIterator<Item = &'a BucketMeasurements>,
    schema: &HistogramSchema,
    apdex_threshold_micros: u64,
) -> Result<RedSummary> {
    let measurements = measurements.into_iter().collect::<Vec<_>>();
    let (histogram, latency_partial) = merged_latency(measurements.iter().copied(), schema)?;
    let mut request_count = 0_u64;
    let mut error_count = 0_u64;
    let mut duration_sum_micros = 0_u64;
    let mut exemplars = Vec::new();
    let mut status_codes = BTreeMap::new();
    let (mut timeout_count, mut retry_count) = (0_u64, 0_u64);

    for measurement in measurements {
        request_count = request_count.saturating_add(measurement.request_count);
        timeout_count = timeout_count.saturating_add(measurement.timeout_count);
        retry_count = retry_count.saturating_add(measurement.retry_count);

        for (status, count) in &measurement.status_codes {
            count_status(&mut status_codes, status, *count);
        }

        error_count = error_count.saturating_add(measurement.error_count);
        duration_sum_micros = duration_sum_micros.saturating_add(measurement.latency.sum_time);

        for exemplar in &measurement.exemplars {
            if !exemplars
                .iter()
                .any(|current: &crate::domain::apm::TraceExemplar| {
                    current.trace_id == exemplar.trace_id && current.span_id == exemplar.span_id
                })
            {
                exemplars.push(exemplar.clone());
            }
        }
    }

    exemplars.sort_by(|left, right| {
        right
            .duration
            .cmp(&left.duration)
            .then_with(|| left.trace_id.cmp(&right.trace_id))
    });

    exemplars.truncate(8);
    let error_rate = ratio(error_count, request_count);

    Ok(RedSummary {
        request_count,
        error_count,
        error_rate,
        duration_sum: duration_sum_micros,
        duration_average: (request_count != 0).then(|| duration_sum_micros / request_count),
        p50_time: histogram.quantile(schema, 0.50)?,
        p90_time: histogram.quantile(schema, 0.90)?,
        p95_time: histogram.quantile(schema, 0.95)?,
        p99_time: histogram.quantile(schema, 0.99)?,
        apdex: apdex(&histogram, schema, error_rate, apdex_threshold_micros)?,
        latency_partial,
        exemplars,
        status_codes,
        timeout_count,
        retry_count,
    })
}

/// Apdex against `threshold_micros`: the share of requests within T and half
/// the share within 4T. The histogram does not tell which latencies failed,
/// so failures are taken out evenly across them: a failed request is never
/// satisfied nor tolerated.
pub(super) fn apdex(
    histogram: &LatencyHistogram,
    schema: &HistogramSchema,
    error_rate: f64,
    threshold_micros: u64,
) -> Result<Option<f64>> {
    let count = histogram.count();

    if count == 0 {
        return Ok(None);
    }

    let satisfied = histogram.count_at_or_below(schema, threshold_micros)?;
    let within_tolerance =
        histogram.count_at_or_below(schema, threshold_micros.saturating_mul(4))?;
    let tolerating = (within_tolerance - satisfied).max(0.0);
    let score = (satisfied + tolerating / 2.0) / count as f64 * (1.0 - error_rate.clamp(0.0, 1.0));

    Ok(Some(score.clamp(0.0, 1.0)))
}

pub(super) fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
