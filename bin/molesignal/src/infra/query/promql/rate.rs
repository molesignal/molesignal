// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub fn apply_rate_like(func: &str, series: Vec<Series>, range: Duration) -> InstantVector {
    let range_secs = range.as_secs_f64().max(1.0);
    let items = series
        .into_iter()
        .filter_map(|s| {
            let metadata = s.metadata_slice(0, s.samples.len());
            let value =
                rate_window_value_with_metadata(func, s.samples.as_slice(), metadata, range_secs)?;
            Some((s.labels, value))
        })
        .collect();
    InstantVector { items }
}

pub(super) fn apply_rate_like_range(
    func: &str,
    series: Vec<Series>,
    range: Duration,
    start_us: i64,
    end_us: i64,
    step_us: i64,
) -> RangeVector {
    let range_us = (range.as_micros() as i64).max(1);
    let range_secs = range.as_secs_f64().max(1.0);
    let mut points = Vec::new();

    for s in series {
        each_step_window_indices(
            &s.samples,
            start_us,
            end_us,
            step_us,
            range_us,
            |t, lo, hi| {
                let samples = &s.samples[lo..hi];
                let metadata = s.metadata_slice(lo, hi);
                let Some(value) =
                    rate_window_value_with_metadata(func, samples, metadata, range_secs)
                else {
                    return;
                };
                points.push(RangePoint {
                    ts_us: t,
                    labels: s.labels.clone(),
                    value,
                });
            },
        );
    }

    RangeVector { points }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RateSemantics {
    CumulativeCounter,
    CumulativeGauge,
    DeltaCounter,
    DeltaGauge,
}

impl RateSemantics {
    fn from_metadata(metadata: MetricSampleMetadata) -> Self {
        match (metadata.temporality(), metadata.is_monotonic()) {
            (MetricTemporality::Delta, true) => Self::DeltaCounter,
            (MetricTemporality::Delta, false) => Self::DeltaGauge,
            (MetricTemporality::Cumulative, true) => Self::CumulativeCounter,
            (MetricTemporality::Cumulative, false) => Self::CumulativeGauge,
        }
    }

    fn is_delta(self) -> bool {
        matches!(self, Self::DeltaCounter | Self::DeltaGauge)
    }

    fn is_monotonic(self) -> bool {
        matches!(self, Self::CumulativeCounter | Self::DeltaCounter)
    }
}

/// Use only the latest contiguous aggregation mode so a temporality change is
/// not mistaken for either a counter reset or a DELTA sample.
pub(super) fn rate_window_value_with_metadata(
    func: &str,
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    range_secs: f64,
) -> Option<f64> {
    let (samples, metadata, semantics) = semantic_suffix(samples, metadata);
    if semantics.is_delta() {
        return delta_rate_value(
            func,
            samples,
            metadata,
            semantics.is_monotonic(),
            range_secs,
        );
    }
    cumulative_rate_value(func, samples, semantics.is_monotonic(), range_secs)
}

fn semantic_suffix<'a>(
    samples: &'a [(i64, f64)],
    metadata: &'a [MetricSampleMetadata],
) -> (&'a [(i64, f64)], &'a [MetricSampleMetadata], RateSemantics) {
    if samples.is_empty() || metadata.len() != samples.len() {
        return (samples, &[], RateSemantics::CumulativeCounter);
    }
    let semantics = RateSemantics::from_metadata(metadata[metadata.len() - 1]);
    let mut start = metadata.len() - 1;
    while start > 0 && RateSemantics::from_metadata(metadata[start - 1]) == semantics {
        start -= 1;
    }
    (&samples[start..], &metadata[start..], semantics)
}

fn cumulative_rate_value(
    func: &str,
    samples: &[(i64, f64)],
    monotonic: bool,
    range_secs: f64,
) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let samples = match func {
        "irate" => &samples[samples.len() - 2..],
        "rate" | "increase" => samples,
        _ => return None,
    };
    let (first_ts, _) = *samples.first()?;
    let (last_ts, _) = *samples.last()?;
    if last_ts <= first_ts {
        return None;
    }
    let delta = if monotonic {
        counter_delta(samples)?
    } else {
        signed_delta(samples)?
    };
    match func {
        "rate" => Some(delta / range_secs),
        "irate" => {
            let elapsed_secs = (last_ts - first_ts) as f64 / 1_000_000.0;
            Some(delta / elapsed_secs.max(1e-9))
        }
        "increase" => Some(delta),
        _ => None,
    }
}

fn delta_rate_value(
    func: &str,
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    monotonic: bool,
    range_secs: f64,
) -> Option<f64> {
    match func {
        "rate" | "increase" => {
            let mut seen = false;
            let delta = samples
                .iter()
                .zip(metadata)
                .filter_map(|((_, value), _)| {
                    if value.is_finite() && (!monotonic || *value >= 0.0) {
                        seen = true;
                        Some(*value)
                    } else {
                        None
                    }
                })
                .sum::<f64>();
            if !seen {
                return None;
            }
            (func == "rate")
                .then_some(delta / range_secs)
                .or(Some(delta))
        }
        "irate" => delta_irate(samples, metadata, monotonic),
        _ => None,
    }
}

fn delta_irate(
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    monotonic: bool,
) -> Option<f64> {
    for index in (0..samples.len()).rev() {
        let (timestamp, value) = samples[index];
        if !value.is_finite() || (monotonic && value < 0.0) {
            continue;
        }
        let start = metadata[index]
            .start_time_us()
            .filter(|start| *start < timestamp)
            .or_else(|| {
                samples[..index]
                    .iter()
                    .rev()
                    .map(|(previous, _)| *previous)
                    .find(|previous| *previous < timestamp)
            })?;
        let elapsed_secs = (timestamp - start) as f64 / 1_000_000.0;
        return Some(value / elapsed_secs.max(1e-9));
    }
    None
}

fn signed_delta(samples: &[(i64, f64)]) -> Option<f64> {
    let mut iter = samples
        .iter()
        .copied()
        .filter(|(_, value)| value.is_finite());
    let (mut previous_ts, first_value) = iter.next()?;
    let mut last_value = first_value;
    let mut transitions = 0usize;
    for (timestamp, value) in iter {
        if timestamp <= previous_ts {
            continue;
        }
        previous_ts = timestamp;
        last_value = value;
        transitions += 1;
    }
    (transitions > 0).then_some(last_value - first_value)
}

/// Counter reset handling shared by cumulative `rate`, `irate` and `increase`.
fn counter_delta(samples: &[(i64, f64)]) -> Option<f64> {
    let mut iter = samples
        .iter()
        .copied()
        .filter(|(_, value)| value.is_finite());
    let (mut previous_ts, mut previous_value) = iter.next()?;
    let mut delta = 0.0;
    let mut transitions = 0usize;

    for (timestamp, value) in iter {
        if timestamp <= previous_ts {
            continue;
        }
        delta += if value >= previous_value {
            value - previous_value
        } else {
            value.max(0.0)
        };
        previous_ts = timestamp;
        previous_value = value;
        transitions += 1;
    }

    (transitions > 0).then_some(delta)
}
