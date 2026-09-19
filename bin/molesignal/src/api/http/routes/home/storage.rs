// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Storage-catalog aggregation for the home overview.

use super::{HealthStatus, OverviewBucket, SignalOverview, StreamOverview};
use crate::{
    domain::{
        storage::QueryFile,
        stream::{StreamDefinition, StreamType},
    },
    infra::persistence::repositories::usage::{HOUR_MICROS, IntakeUsageBucket},
    shared::time::TimeRange,
};

pub(super) const HEALTHY_LAG_MICROS: i64 = 15 * 60 * 1_000_000;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BucketTotals {
    pub(super) rows: u64,
    pub(super) stored_bytes: u64,
}

pub(super) struct StreamScan {
    pub(super) overview: StreamOverview,
    pub(super) buckets: Vec<BucketTotals>,
    pub(super) stats_ok: bool,
}

pub(super) fn summarize_stream(
    definition: StreamDefinition,
    files: &[QueryFile],
    window: TimeRange,
    generated_at_micros: i64,
    bucket_count: usize,
) -> StreamScan {
    let mut rows = 0_u64;
    let mut stored_bytes = 0_u64;
    let mut first_received_at_micros: Option<i64> = None;
    let mut last_received_at_micros: Option<i64> = None;
    let mut buckets = vec![BucketTotals::default(); bucket_count];

    for file in files {
        let Some((file_rows, file_bytes, overlap_start, overlap_end)) =
            file_contribution(file, window)
        else {
            continue;
        };
        rows = rows.saturating_add(file_rows);
        stored_bytes = stored_bytes.saturating_add(file_bytes);
        first_received_at_micros = Some(
            first_received_at_micros.map_or(overlap_start, |current| current.min(overlap_start)),
        );
        last_received_at_micros =
            Some(last_received_at_micros.map_or(overlap_end, |current| current.max(overlap_end)));
        let midpoint = overlap_start.saturating_add(overlap_end.saturating_sub(overlap_start) / 2);
        let index = bucket_index(window, bucket_count, midpoint);
        buckets[index].rows = buckets[index].rows.saturating_add(file_rows);
        buckets[index].stored_bytes = buckets[index].stored_bytes.saturating_add(file_bytes);
    }

    let status = stream_status(last_received_at_micros, generated_at_micros);
    StreamScan {
        overview: StreamOverview {
            id: definition.id.0,
            name: definition.name,
            stream_type: definition.stream_type,
            status,
            rows,
            stored_bytes,
            first_received_at_micros,
            last_received_at_micros,
        },
        buckets,
        stats_ok: true,
    }
}

pub(super) fn unavailable_stream(definition: StreamDefinition, bucket_count: usize) -> StreamScan {
    StreamScan {
        overview: StreamOverview {
            id: definition.id.0,
            name: definition.name,
            stream_type: definition.stream_type,
            status: HealthStatus::Unknown,
            rows: 0,
            stored_bytes: 0,
            first_received_at_micros: None,
            last_received_at_micros: None,
        },
        buckets: vec![BucketTotals::default(); bucket_count],
        stats_ok: false,
    }
}

fn file_contribution(file: &QueryFile, window: TimeRange) -> Option<(u64, u64, i64, i64)> {
    if !file.time_range.overlaps(window) {
        return None;
    }
    let overlap_start = file.time_range.start.0.max(window.start.0);
    let overlap_end = file.time_range.end.0.min(window.end.0);
    let file_duration = file
        .time_range
        .end
        .0
        .saturating_sub(file.time_range.start.0);
    if file_duration <= 0 {
        return Some((file.rows, file.size_bytes, overlap_start, overlap_end));
    }
    let overlap_duration = overlap_end.saturating_sub(overlap_start);
    if overlap_duration <= 0 {
        return None;
    }
    let ratio = (overlap_duration as f64 / file_duration as f64).clamp(0.0, 1.0);
    Some((
        prorate(file.rows, ratio),
        prorate(file.size_bytes, ratio),
        overlap_start,
        overlap_end,
    ))
}

fn prorate(value: u64, ratio: f64) -> u64 {
    if value == 0 || ratio <= 0.0 {
        return 0;
    }
    if ratio >= 1.0 {
        return value;
    }
    ((value as f64 * ratio).round() as u64).max(1).min(value)
}

pub(super) fn stream_status(
    last_received_at_micros: Option<i64>,
    generated_at_micros: i64,
) -> HealthStatus {
    match last_received_at_micros {
        Some(last) if last >= generated_at_micros.saturating_sub(HEALTHY_LAG_MICROS) => {
            HealthStatus::Healthy
        }
        Some(_) => HealthStatus::Delayed,
        None => HealthStatus::NoData,
    }
}

pub(super) fn signal_overviews(streams: &[StreamOverview]) -> Vec<SignalOverview> {
    [
        StreamType::LOGS,
        StreamType::METRICS,
        StreamType::TRACES,
        StreamType::PROFILES,
    ]
    .into_iter()
    .map(|stream_type| {
        let matching = streams
            .iter()
            .filter(|stream| stream.stream_type == stream_type)
            .collect::<Vec<_>>();
        let total_streams = matching.len();
        let active_streams = matching.iter().filter(|stream| stream.rows > 0).count();
        let has_healthy = matching
            .iter()
            .any(|stream| stream.status == HealthStatus::Healthy);
        let has_unknown = matching
            .iter()
            .any(|stream| stream.status == HealthStatus::Unknown);
        let has_degraded = matching
            .iter()
            .any(|stream| stream.status == HealthStatus::Degraded);
        let status = if has_degraded {
            HealthStatus::Degraded
        } else if has_unknown {
            HealthStatus::Unknown
        } else if has_healthy {
            HealthStatus::Healthy
        } else if total_streams == 0 || active_streams == 0 {
            HealthStatus::NoData
        } else {
            HealthStatus::Delayed
        };
        SignalOverview {
            stream_type,
            status,
            total_streams,
            active_streams,
            rows: matching
                .iter()
                .fold(0_u64, |sum, stream| sum.saturating_add(stream.rows)),
            stored_bytes: matching
                .iter()
                .fold(0_u64, |sum, stream| sum.saturating_add(stream.stored_bytes)),
            last_received_at_micros: matching
                .iter()
                .filter_map(|stream| stream.last_received_at_micros)
                .max(),
        }
    })
    .collect()
}

pub(super) fn bucket_raw_usage(
    usage: &[IntakeUsageBucket],
    window: TimeRange,
    bucket_count: usize,
) -> Vec<u64> {
    let mut output = vec![0_u64; bucket_count];
    for item in usage {
        let midpoint = item.bucket_start_micros.saturating_add(HOUR_MICROS / 2);
        let index = bucket_index(window, bucket_count, midpoint);
        output[index] = output[index].saturating_add(item.intake_bytes.max(0) as u64);
    }
    output
}

pub(super) fn build_buckets(
    window: TimeRange,
    totals: &[BucketTotals],
    raw: Option<&[u64]>,
) -> Vec<OverviewBucket> {
    let count = totals.len().max(1);
    let duration = window.duration_micros().max(1);
    (0..totals.len())
        .map(|index| {
            let start = window
                .start
                .0
                .saturating_add(((duration as i128 * index as i128) / count as i128) as i64);
            let end = if index + 1 == count {
                window.end.0
            } else {
                window.start.0.saturating_add(
                    ((duration as i128 * (index + 1) as i128) / count as i128) as i64,
                )
            };
            OverviewBucket {
                start_micros: start,
                end_micros: end,
                intake_bytes: raw.and_then(|values| values.get(index).copied()),
                stored_bytes: totals[index].stored_bytes,
                rows: totals[index].rows,
            }
        })
        .collect()
}

fn bucket_index(window: TimeRange, bucket_count: usize, timestamp_micros: i64) -> usize {
    let duration = window.duration_micros().max(1);
    let offset = timestamp_micros
        .saturating_sub(window.start.0)
        .clamp(0, duration.saturating_sub(1));
    (((offset as i128 * bucket_count as i128) / duration as i128) as usize)
        .min(bucket_count.saturating_sub(1))
}

pub(super) fn status_rank(status: HealthStatus) -> u8 {
    match status {
        HealthStatus::Degraded | HealthStatus::Unknown => 0,
        HealthStatus::Delayed => 1,
        HealthStatus::Healthy => 2,
        HealthStatus::NoData => 3,
    }
}

pub(super) fn needs_attention(status: HealthStatus) -> bool {
    matches!(
        status,
        HealthStatus::Degraded | HealthStatus::Delayed | HealthStatus::Unknown
    )
}
