// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use super::{RedSummary, RedTrendPoint};
use crate::{
    domain::apm::{BucketMeasurements, HistogramSchema, LatencyHistogram, MergedBucket},
    shared::{Result, time::TimestampMicros},
};

pub(super) fn red_from_buckets(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
) -> Result<RedSummary> {
    red_from_measurements(buckets.iter().map(|bucket| &bucket.measurements), schema)
}

pub(super) fn trend_from_buckets(
    buckets: &[MergedBucket],
    schema: &HistogramSchema,
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
                red: red_from_measurements(measurements, schema)?,
            })
        })
        .collect()
}

fn red_from_measurements<'a>(
    measurements: impl IntoIterator<Item = &'a BucketMeasurements>,
    schema: &HistogramSchema,
) -> Result<RedSummary> {
    let mut request_count = 0_u64;
    let mut error_count = 0_u64;
    let mut duration_sum_micros = 0_u64;
    let mut histogram = LatencyHistogram::empty(schema);
    let mut latency_partial = false;
    let mut exemplars = Vec::new();
    for measurement in measurements {
        request_count = request_count.saturating_add(measurement.request_count);
        error_count = error_count.saturating_add(measurement.error_count);
        duration_sum_micros = duration_sum_micros.saturating_add(measurement.latency.sum_micros);
        if measurement.latency.schema_version == schema.version
            && measurement.latency.counts.len() == schema.upper_bounds_micros.len()
        {
            histogram.merge(&measurement.latency)?;
        } else {
            latency_partial = true;
        }
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
            .duration_micros
            .cmp(&left.duration_micros)
            .then_with(|| left.trace_id.cmp(&right.trace_id))
    });
    exemplars.truncate(8);
    Ok(RedSummary {
        request_count,
        error_count,
        error_rate: ratio(error_count, request_count),
        duration_sum_micros,
        duration_average_micros: (request_count != 0).then(|| duration_sum_micros / request_count),
        p50_micros: histogram.quantile(schema, 0.50)?,
        p95_micros: histogram.quantile(schema, 0.95)?,
        p99_micros: histogram.quantile(schema, 0.99)?,
        latency_partial,
        exemplars,
    })
}

pub(super) fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
