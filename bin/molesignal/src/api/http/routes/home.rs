// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Home command-center overview.
//!
//! The endpoint intentionally aggregates operational metadata instead of
//! executing one SQL query per stream:
//! - raw intake bytes come from `intake_usage_hourly`;
//! - compressed storage bytes, rows and receive timestamps come from
//!   a multi-dataset `FileCatalog` snapshot;
//! - process health comes from the same probe as `/healthz`.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, storage::logical_query_dataset_types, stream::StreamType},
    infra::query::catalog_source::StreamSnapshotSelection,
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod storage;

use storage::{
    BucketTotals, StreamScan, bucket_raw_usage, build_buckets, needs_attention, signal_overviews,
    status_rank, summarize_stream, unavailable_stream,
};
#[cfg(test)]
use storage::{HEALTHY_LAG_MICROS, stream_status};

#[cfg(test)]
use crate::{
    domain::storage::{QueryFile, primary_dataset_type},
    domain::stream::StreamDefinition,
    infra::persistence::repositories::usage::{HOUR_MICROS, IntakeUsageBucket},
};

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const MIN_WINDOW_SECS: i64 = 60 * 60;
const MAX_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;
const DEFAULT_BUCKET_COUNT: usize = 24;
const MIN_BUCKET_COUNT: usize = 6;
const MAX_BUCKET_COUNT: usize = 48;

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
    intake_bytes: Option<u64>,
    stored_bytes: u64,
    rows: u64,
}

#[derive(Debug, Serialize)]
struct HomeOverviewResponse {
    generated_at_micros: i64,
    window: OverviewWindow,
    intake_status: HealthStatus,
    probe_reason: Option<String>,
    intake_bytes: Option<u64>,
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
            .hourly_intake_bytes(&ctx.org_id, range.start.0, range.end.0),
    );
    let stream_defs = stream_defs?
        .into_iter()
        .filter(|definition| definition.stream_type != StreamType::EXTEND)
        .collect::<Vec<_>>();
    let raw_usage_available = usage_result.is_ok();
    let usage = match usage_result {
        Ok(buckets) => buckets,
        Err(error) => {
            tracing::warn!(
                org_id = %ctx.org_id.0,
                error = %error,
                "home overview raw-intake usage unavailable"
            );
            Vec::new()
        }
    };

    let selections = stream_defs
        .iter()
        .map(|definition| {
            Ok(StreamSnapshotSelection::new(
                definition.clone(),
                logical_query_dataset_types(definition.stream_type)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let scans: Vec<StreamScan> = match state
        .storage
        .catalog_query
        .snapshot_streams(&selections, range)
        .await
    {
        Ok(snapshots) => stream_defs
            .into_iter()
            .zip(snapshots)
            .map(|(definition, snapshot)| {
                summarize_stream(
                    definition,
                    &snapshot.files(),
                    range,
                    generated_at.0,
                    bucket_count,
                )
            })
            .collect(),
        Err(error) => {
            tracing::warn!(
                org_id = %ctx.org_id.0,
                %error,
                "home overview catalog snapshot failed"
            );
            stream_defs
                .into_iter()
                .map(|definition| unavailable_stream(definition, bucket_count))
                .collect()
        }
    };

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
    let intake_bytes = raw_usage_available.then(|| {
        usage.iter().fold(0_u64, |sum, item| {
            sum.saturating_add(item.intake_bytes.max(0) as u64)
        })
    });
    let compression_savings_ratio = intake_bytes
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
    let intake_status = if !probe_healthy {
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
        intake_status,
        probe_reason: probe_reason.map(str::to_owned),
        intake_bytes,
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

    fn file(start: i64, end: i64, rows: u64, bytes: u64) -> QueryFile {
        QueryFile {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: "app_logs".into(),
            stream_type: StreamType::LOGS,
            dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
            object_key: "test.parquet".into(),
            checksum: None,
            etag: None,
            time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            rows,
            size_bytes: bytes,
            min_values: Map::new(),
            max_values: Map::new(),
        }
    }

    #[test]
    fn stream_summary_counts_full_and_prorated_boundary_files() {
        let window = TimeRange::new(TimestampMicros(0), TimestampMicros(100));
        let files = vec![file(20, 40, 10, 100), file(90, 110, 20, 200)];
        let scan = summarize_stream(
            definition("app_logs", StreamType::LOGS),
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
            IntakeUsageBucket {
                bucket_start_micros: 0,
                intake_bytes: 10,
            },
            IntakeUsageBucket {
                bucket_start_micros: 2 * HOUR_MICROS,
                intake_bytes: 30,
            },
        ];
        assert_eq!(bucket_raw_usage(&usage, window, 4), vec![10, 0, 30, 0]);
    }

    #[test]
    fn empty_streams_do_not_downgrade_an_active_signal() {
        let active = StreamOverview {
            id: "active".into(),
            name: "active".into(),
            stream_type: StreamType::LOGS,
            status: HealthStatus::Healthy,
            rows: 10,
            stored_bytes: 100,
            first_received_at_micros: Some(1),
            last_received_at_micros: Some(2),
        };
        let empty = StreamOverview {
            id: "empty".into(),
            name: "empty".into(),
            stream_type: StreamType::LOGS,
            status: HealthStatus::NoData,
            rows: 0,
            stored_bytes: 0,
            first_received_at_micros: None,
            last_received_at_micros: None,
        };

        let signals = signal_overviews(&[active, empty]);
        let logs = signals
            .iter()
            .find(|signal| signal.stream_type == StreamType::LOGS)
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
