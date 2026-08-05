// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Home command-center overview.
//!
//! The endpoint intentionally aggregates operational metadata instead of
//! executing one SQL query per stream:
//! - raw ingest bytes come from `ingest_usage_hourly`;
//! - compressed storage bytes, rows and receive timestamps come from
//!   `ParquetFileMetaRepository`;
//! - process health comes from the same probe as `/healthz`.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use futures::{StreamExt, future::try_join_all, stream};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        storage::{ParquetFileMeta, logical_query_datasets},
        stream::{StreamDefinition, StreamType},
    },
    infra::persistence::repositories::usage::{HOUR_MICROS, IngestUsageBucket},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const MIN_WINDOW_SECS: i64 = 60 * 60;
const MAX_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;
const DEFAULT_BUCKET_COUNT: usize = 24;
const MIN_BUCKET_COUNT: usize = 6;
const MAX_BUCKET_COUNT: usize = 48;
const HEALTHY_LAG_MICROS: i64 = 15 * 60 * 1_000_000;
const PARQUET_FILE_META_CONCURRENCY: usize = 8;

pub fn routes() -> Router<AppState> {
    Router::new().route("/home/overview", get(overview))
}

#[derive(Debug, Default, Deserialize)]
struct OverviewParams {
    window_secs: Option<i64>,
    bucket_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HealthStatus {
    Healthy,
    Degraded,
    Delayed,
    NoData,
    Unknown,
}

#[derive(Debug, Serialize)]
struct OverviewWindow {
    start_micros: i64,
    end_micros: i64,
    window_secs: i64,
}

#[derive(Debug, Serialize)]
struct StatsProbe {
    succeeded: usize,
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
struct StreamOverview {
    id: String,
    name: String,
    stream_type: StreamType,
    status: HealthStatus,
    rows: u64,
    stored_bytes: u64,
    first_received_at_micros: Option<i64>,
    last_received_at_micros: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SignalOverview {
    stream_type: StreamType,
    status: HealthStatus,
    total_streams: usize,
    active_streams: usize,
    rows: u64,
    stored_bytes: u64,
    last_received_at_micros: Option<i64>,
}

#[derive(Debug, Serialize)]
struct OverviewBucket {
    start_micros: i64,
    end_micros: i64,
    ingested_bytes: Option<u64>,
    stored_bytes: u64,
    rows: u64,
}

#[derive(Debug, Serialize)]
struct HomeOverviewResponse {
    generated_at_micros: i64,
    window: OverviewWindow,
    ingest_status: HealthStatus,
    probe_reason: Option<String>,
    ingested_bytes: Option<u64>,
    stored_bytes: u64,
    rows: u64,
    compression_savings_ratio: Option<f64>,
    active_streams: usize,
    total_streams: usize,
    attention_streams: usize,
    last_received_at_micros: Option<i64>,
    stats_probe: StatsProbe,
    buckets: Vec<OverviewBucket>,
    signals: Vec<SignalOverview>,
    streams: Vec<StreamOverview>,
}

#[derive(Debug, Clone, Copy, Default)]
struct BucketTotals {
    rows: u64,
    stored_bytes: u64,
}

struct StreamScan {
    overview: StreamOverview,
    buckets: Vec<BucketTotals>,
    stats_ok: bool,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn overview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<OverviewParams>,
) -> Result<Json<HomeOverviewResponse>> {
    let window_secs = params
        .window_secs
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(MIN_WINDOW_SECS, MAX_WINDOW_SECS);
    let bucket_count = params
        .bucket_count
        .unwrap_or(DEFAULT_BUCKET_COUNT)
        .clamp(MIN_BUCKET_COUNT, MAX_BUCKET_COUNT);
    let generated_at = TimestampMicros::now();
    let range = TimeRange::new(
        TimestampMicros(
            generated_at
                .0
                .saturating_sub(window_secs.saturating_mul(1_000_000)),
        ),
        generated_at,
    );

    let (stream_defs, usage_result) = tokio::join!(
        state.telemetry.streams.list(&ctx.org_id),
        state
            .platform
            .usage
            .hourly_ingest_bytes(&ctx.org_id, range.start.0, range.end.0),
    );
    let stream_defs = stream_defs?
        .into_iter()
        .filter(|definition| definition.stream_type != StreamType::Extend)
        .collect::<Vec<_>>();
    let raw_usage_available = usage_result.is_ok();
    let usage = match usage_result {
        Ok(buckets) => buckets,
        Err(error) => {
            tracing::warn!(
                org_id = %ctx.org_id.0,
                error = %error,
                "home overview raw-ingest usage unavailable"
            );
            Vec::new()
        }
    };

    let parquet_file_meta = state.storage.parquet_file_meta.clone();
    let org_id = ctx.org_id.clone();
    let scans = stream::iter(stream_defs)
        .map(|definition| {
            let parquet_file_meta = parquet_file_meta.clone();
            let org_id = org_id.clone();
            async move {
                let lookups =
                    logical_query_datasets(definition.stream_type)
                        .iter()
                        .map(|dataset_kind| {
                            parquet_file_meta.find_dataset(
                                &org_id,
                                &definition.name,
                                definition.stream_type,
                                *dataset_kind,
                                range,
                            )
                        });
                let files = try_join_all(lookups)
                    .await
                    .map(|groups| groups.into_iter().flatten().collect::<Vec<_>>());
                match files {
                    Ok(files) => {
                        summarize_stream(definition, &files, range, generated_at.0, bucket_count)
                    }
                    Err(error) => {
                        tracing::warn!(
                            org_id = %org_id.0,
                            stream = %definition.name,
                            stream_type = ?definition.stream_type,
                            error = %error,
                            "home overview parquet-file-meta scan failed"
                        );
                        unavailable_stream(definition, bucket_count)
                    }
                }
            }
        })
        .buffer_unordered(PARQUET_FILE_META_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut bucket_totals = vec![BucketTotals::default(); bucket_count];
    for scan in &scans {
        for (target, source) in bucket_totals.iter_mut().zip(&scan.buckets) {
            target.rows = target.rows.saturating_add(source.rows);
            target.stored_bytes = target.stored_bytes.saturating_add(source.stored_bytes);
        }
    }
    let raw_buckets = bucket_raw_usage(&usage, range, bucket_count);
    let buckets = build_buckets(
        range,
        &bucket_totals,
        raw_usage_available.then_some(raw_buckets.as_slice()),
    );

    let mut streams = scans
        .iter()
        .map(|scan| scan.overview.clone())
        .collect::<Vec<_>>();
    streams.sort_by(|a, b| {
        status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then(b.rows.cmp(&a.rows))
            .then(a.name.cmp(&b.name))
    });

    let rows = streams
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.rows));
    let stored_bytes = streams
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.stored_bytes));
    let ingested_bytes = raw_usage_available.then(|| {
        usage.iter().fold(0_u64, |sum, item| {
            sum.saturating_add(item.ingest_bytes.max(0) as u64)
        })
    });
    let compression_savings_ratio = ingested_bytes
        .filter(|raw| *raw > 0)
        .map(|raw| 1.0 - stored_bytes as f64 / raw as f64);
    let active_streams = streams.iter().filter(|item| item.rows > 0).count();
    let attention_streams = streams
        .iter()
        .filter(|item| needs_attention(item.status))
        .count();
    let last_received_at_micros = streams
        .iter()
        .filter_map(|item| item.last_received_at_micros)
        .max();
    let stats_succeeded = scans.iter().filter(|scan| scan.stats_ok).count();
    let signals = signal_overviews(&streams);
    let (probe_healthy, probe_reason) = state.telemetry.probe.snapshot();
    let ingest_status = if !probe_healthy {
        HealthStatus::Degraded
    } else if active_streams == 0 {
        HealthStatus::NoData
    } else {
        HealthStatus::Healthy
    };

    Ok(Json(HomeOverviewResponse {
        generated_at_micros: generated_at.0,
        window: OverviewWindow {
            start_micros: range.start.0,
            end_micros: range.end.0,
            window_secs,
        },
        ingest_status,
        probe_reason: probe_reason.map(str::to_owned),
        ingested_bytes,
        stored_bytes,
        rows,
        compression_savings_ratio,
        active_streams,
        total_streams: streams.len(),
        attention_streams,
        last_received_at_micros,
        stats_probe: StatsProbe {
            succeeded: stats_succeeded,
            total: scans.len(),
        },
        buckets,
        signals,
        streams,
    }))
}

fn summarize_stream(
    definition: StreamDefinition,
    files: &[ParquetFileMeta],
    window: TimeRange,
    generated_at_micros: i64,
    bucket_count: usize,
) -> StreamScan {
    let mut rows = 0_u64;
    let mut stored_bytes = 0_u64;
    let mut first_received_at_micros: Option<i64> = None;
    let mut last_received_at_micros: Option<i64> = None;
    let mut buckets = vec![BucketTotals::default(); bucket_count];

    for file in files.iter().filter(|file| !file.deleted) {
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

fn unavailable_stream(definition: StreamDefinition, bucket_count: usize) -> StreamScan {
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

fn file_contribution(file: &ParquetFileMeta, window: TimeRange) -> Option<(u64, u64, i64, i64)> {
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

fn stream_status(last_received_at_micros: Option<i64>, generated_at_micros: i64) -> HealthStatus {
    match last_received_at_micros {
        Some(last) if last >= generated_at_micros.saturating_sub(HEALTHY_LAG_MICROS) => {
            HealthStatus::Healthy
        }
        Some(_) => HealthStatus::Delayed,
        None => HealthStatus::NoData,
    }
}

fn signal_overviews(streams: &[StreamOverview]) -> Vec<SignalOverview> {
    [
        StreamType::Logs,
        StreamType::Metrics,
        StreamType::Traces,
        StreamType::Profiles,
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

fn bucket_raw_usage(
    usage: &[IngestUsageBucket],
    window: TimeRange,
    bucket_count: usize,
) -> Vec<u64> {
    let mut output = vec![0_u64; bucket_count];
    for item in usage {
        let midpoint = item.bucket_start_micros.saturating_add(HOUR_MICROS / 2);
        let index = bucket_index(window, bucket_count, midpoint);
        output[index] = output[index].saturating_add(item.ingest_bytes.max(0) as u64);
    }
    output
}

fn build_buckets(
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
                ingested_bytes: raw.and_then(|values| values.get(index).copied()),
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

fn status_rank(status: HealthStatus) -> u8 {
    match status {
        HealthStatus::Degraded | HealthStatus::Unknown => 0,
        HealthStatus::Delayed => 1,
        HealthStatus::Healthy => 2,
        HealthStatus::NoData => 3,
    }
}

fn needs_attention(status: HealthStatus) -> bool {
    matches!(
        status,
        HealthStatus::Degraded | HealthStatus::Delayed | HealthStatus::Unknown
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::shared::ids::Id;

    fn definition(name: &str, stream_type: StreamType) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(format!("stream-{name}")),
            org_id: Id::from_string("org-1"),
            name: name.to_string(),
            stream_type,
            schema: crate::domain::stream::Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn file(start: i64, end: i64, rows: u64, bytes: u64) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: "app_logs".into(),
            stream_type: StreamType::Logs,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: "test.parquet".into(),
            time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            rows,
            size_bytes: bytes,
            min_values: Map::new(),
            max_values: Map::new(),
            deleted: false,
        }
    }

    #[test]
    fn stream_summary_counts_full_and_prorated_boundary_files() {
        let window = TimeRange::new(TimestampMicros(0), TimestampMicros(100));
        let files = vec![file(20, 40, 10, 100), file(90, 110, 20, 200)];
        let scan = summarize_stream(
            definition("app_logs", StreamType::Logs),
            &files,
            window,
            100,
            5,
        );

        assert_eq!(scan.overview.rows, 20);
        assert_eq!(scan.overview.stored_bytes, 200);
        assert_eq!(scan.overview.first_received_at_micros, Some(20));
        assert_eq!(scan.overview.last_received_at_micros, Some(100));
        assert_eq!(
            scan.buckets.iter().map(|bucket| bucket.rows).sum::<u64>(),
            20
        );
    }

    #[test]
    fn stream_status_distinguishes_fresh_delayed_and_empty() {
        let now = 10 * HEALTHY_LAG_MICROS;
        assert_eq!(stream_status(Some(now), now), HealthStatus::Healthy);
        assert_eq!(
            stream_status(Some(now - HEALTHY_LAG_MICROS - 1), now),
            HealthStatus::Delayed
        );
        assert_eq!(stream_status(None, now), HealthStatus::NoData);
    }

    #[test]
    fn raw_usage_is_placed_in_matching_visual_bucket() {
        let window = TimeRange::new(TimestampMicros(0), TimestampMicros(4 * HOUR_MICROS));
        let usage = vec![
            IngestUsageBucket {
                bucket_start_micros: 0,
                ingest_bytes: 10,
            },
            IngestUsageBucket {
                bucket_start_micros: 2 * HOUR_MICROS,
                ingest_bytes: 30,
            },
        ];
        assert_eq!(bucket_raw_usage(&usage, window, 4), vec![10, 0, 30, 0]);
    }

    #[test]
    fn empty_streams_do_not_downgrade_an_active_signal() {
        let active = StreamOverview {
            id: "active".into(),
            name: "active".into(),
            stream_type: StreamType::Logs,
            status: HealthStatus::Healthy,
            rows: 10,
            stored_bytes: 100,
            first_received_at_micros: Some(1),
            last_received_at_micros: Some(2),
        };
        let empty = StreamOverview {
            id: "empty".into(),
            name: "empty".into(),
            stream_type: StreamType::Logs,
            status: HealthStatus::NoData,
            rows: 0,
            stored_bytes: 0,
            first_received_at_micros: None,
            last_received_at_micros: None,
        };

        let signals = signal_overviews(&[active, empty]);
        let logs = signals
            .iter()
            .find(|signal| signal.stream_type == StreamType::Logs)
            .expect("logs signal");
        assert_eq!(logs.status, HealthStatus::Healthy);
        assert_eq!(logs.active_streams, 1);
        assert_eq!(logs.total_streams, 2);
    }

    #[test]
    fn only_operational_failures_need_attention() {
        assert!(!needs_attention(HealthStatus::Healthy));
        assert!(!needs_attention(HealthStatus::NoData));
        assert!(needs_attention(HealthStatus::Delayed));
        assert!(needs_attention(HealthStatus::Degraded));
        assert!(needs_attention(HealthStatus::Unknown));
        assert!(status_rank(HealthStatus::Healthy) < status_rank(HealthStatus::NoData));
    }
}
