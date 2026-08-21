// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Runtime storage summary for logical streams.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    agent::telemetry::AGENT_STREAM,
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        storage::{QueryFile, logical_query_dataset_types},
        stream::{StreamDefinition, StreamType},
    },
    infra::query::catalog_source::StreamSnapshotSelection,
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
const INTERRUPTED_LAG_MICROS: i64 = 2 * 60 * 60 * 1_000_000;

#[derive(Debug, Default, Deserialize)]
pub(super) struct RuntimeParams {
    window_secs: Option<i64>,
    bucket_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeStatus {
    Healthy,
    Idle,
    Delayed,
    Interrupted,
    Unused,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
struct RuntimeBucket {
    start_micros: i64,
    end_micros: i64,
    rows: u64,
    stored_bytes: u64,
}

#[derive(Debug, Serialize)]
struct StreamRuntime {
    id: String,
    name: String,
    stream_type: StreamType,
    status: RuntimeStatus,
    rows: u64,
    stored_bytes: u64,
    current_stored_bytes: u64,
    first_received_at_micros: Option<i64>,
    last_received_at_micros: Option<i64>,
    stats_available: bool,
    buckets: Vec<RuntimeBucket>,
}

#[derive(Debug, Serialize)]
pub(super) struct StreamRuntimeResponse {
    generated_at_micros: i64,
    window_start_micros: i64,
    window_end_micros: i64,
    window_secs: i64,
    streams: Vec<StreamRuntime>,
}

#[permission(any("streams.read", "sys.telemetry.read"))]
pub(super) async fn handle(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<RuntimeParams>,
) -> Result<Json<StreamRuntimeResponse>> {
    let window_secs = params
        .window_secs
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(MIN_WINDOW_SECS, MAX_WINDOW_SECS);
    let bucket_count = params
        .bucket_count
        .unwrap_or(DEFAULT_BUCKET_COUNT)
        .clamp(MIN_BUCKET_COUNT, MAX_BUCKET_COUNT);
    let generated_at = TimestampMicros::now();
    let window = TimeRange::new(
        TimestampMicros(
            generated_at
                .0
                .saturating_sub(window_secs.saturating_mul(1_000_000)),
        ),
        generated_at,
    );

    let definitions = state
        .telemetry
        .streams
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|definition| definition.stream_type != StreamType::EXTEND)
        .collect::<Vec<_>>();
    let lifetime_start = definitions
        .iter()
        .map(|definition| definition.created_at.0.min(window.start.0))
        .min()
        .unwrap_or(window.start.0);
    let lifetime = TimeRange::new(TimestampMicros(lifetime_start), generated_at);
    let selections = definitions
        .iter()
        .map(|definition| {
            Ok(StreamSnapshotSelection::new(
                definition.clone(),
                logical_query_dataset_types(definition.stream_type)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut streams: Vec<StreamRuntime> = match state
        .storage
        .catalog_query
        .snapshot_streams(&selections, lifetime)
        .await
    {
        Ok(snapshots) => definitions
            .into_iter()
            .zip(snapshots)
            .map(|(definition, snapshot)| {
                summarize_runtime(
                    definition,
                    &snapshot.files(),
                    window,
                    generated_at.0,
                    bucket_count,
                )
            })
            .collect(),
        Err(error) => {
            tracing::warn!(
                org_id = %ctx.org_id.0,
                %error,
                "stream runtime catalog snapshot failed"
            );
            definitions
                .into_iter()
                .map(|definition| unavailable_runtime(definition, window, bucket_count))
                .collect()
        }
    };
    streams.sort_by(|a, b| {
        runtime_status_rank(a.status)
            .cmp(&runtime_status_rank(b.status))
            .then(b.rows.cmp(&a.rows))
            .then(a.name.cmp(&b.name))
    });

    Ok(Json(StreamRuntimeResponse {
        generated_at_micros: generated_at.0,
        window_start_micros: window.start.0,
        window_end_micros: window.end.0,
        window_secs,
        streams,
    }))
}

fn summarize_runtime(
    definition: StreamDefinition,
    files: &[QueryFile],
    window: TimeRange,
    generated_at_micros: i64,
    bucket_count: usize,
) -> StreamRuntime {
    let mut rows = 0_u64;
    let mut stored_bytes = 0_u64;
    let mut current_stored_bytes = 0_u64;
    let mut first_received_at_micros: Option<i64> = None;
    let mut last_received_at_micros: Option<i64> = None;
    let mut buckets = runtime_buckets(window, bucket_count);

    for file in files {
        current_stored_bytes = current_stored_bytes.saturating_add(file.size_bytes);
        first_received_at_micros = Some(
            first_received_at_micros.map_or(file.time_range.start.0, |current| {
                current.min(file.time_range.start.0)
            }),
        );
        last_received_at_micros = Some(
            last_received_at_micros.map_or(file.time_range.end.0, |current| {
                current.max(file.time_range.end.0)
            }),
        );

        let Some((file_rows, file_bytes, overlap_start, overlap_end)) =
            runtime_file_contribution(file, window)
        else {
            continue;
        };
        rows = rows.saturating_add(file_rows);
        stored_bytes = stored_bytes.saturating_add(file_bytes);
        let midpoint = overlap_start.saturating_add(overlap_end.saturating_sub(overlap_start) / 2);
        let index = runtime_bucket_index(window, bucket_count, midpoint);
        buckets[index].rows = buckets[index].rows.saturating_add(file_rows);
        buckets[index].stored_bytes = buckets[index].stored_bytes.saturating_add(file_bytes);
    }

    let status = runtime_status(
        &definition.name,
        last_received_at_micros,
        generated_at_micros,
    );

    StreamRuntime {
        id: definition.id.0,
        name: definition.name,
        stream_type: definition.stream_type,
        status,
        rows,
        stored_bytes,
        current_stored_bytes,
        first_received_at_micros,
        last_received_at_micros,
        stats_available: true,
        buckets,
    }
}

fn unavailable_runtime(
    definition: StreamDefinition,
    window: TimeRange,
    bucket_count: usize,
) -> StreamRuntime {
    StreamRuntime {
        id: definition.id.0,
        name: definition.name,
        stream_type: definition.stream_type,
        status: RuntimeStatus::Unknown,
        rows: 0,
        stored_bytes: 0,
        current_stored_bytes: 0,
        first_received_at_micros: None,
        last_received_at_micros: None,
        stats_available: false,
        buckets: runtime_buckets(window, bucket_count),
    }
}

fn runtime_status(
    stream_name: &str,
    last_received_at_micros: Option<i64>,
    generated_at_micros: i64,
) -> RuntimeStatus {
    match last_received_at_micros {
        Some(last) if last >= generated_at_micros.saturating_sub(HEALTHY_LAG_MICROS) => {
            RuntimeStatus::Healthy
        }
        // Agent traces are emitted once per completed agent response. A quiet
        // period is expected and must not be reported as a broken continuous feed.
        Some(_) if stream_name == AGENT_STREAM => RuntimeStatus::Idle,
        Some(last) if last >= generated_at_micros.saturating_sub(INTERRUPTED_LAG_MICROS) => {
            RuntimeStatus::Delayed
        }
        Some(_) => RuntimeStatus::Interrupted,
        None => RuntimeStatus::Unused,
    }
}

fn runtime_file_contribution(file: &QueryFile, window: TimeRange) -> Option<(u64, u64, i64, i64)> {
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
        runtime_prorate(file.rows, ratio),
        runtime_prorate(file.size_bytes, ratio),
        overlap_start,
        overlap_end,
    ))
}

fn runtime_prorate(value: u64, ratio: f64) -> u64 {
    if value == 0 || ratio <= 0.0 {
        return 0;
    }
    if ratio >= 1.0 {
        return value;
    }
    ((value as f64 * ratio).round() as u64).max(1).min(value)
}

fn runtime_buckets(window: TimeRange, bucket_count: usize) -> Vec<RuntimeBucket> {
    let duration = window.duration_micros().max(1);
    (0..bucket_count)
        .map(|index| {
            let start_micros = window
                .start
                .0
                .saturating_add(((duration as i128 * index as i128) / bucket_count as i128) as i64);
            let end_micros = if index + 1 == bucket_count {
                window.end.0
            } else {
                window.start.0.saturating_add(
                    ((duration as i128 * (index + 1) as i128) / bucket_count as i128) as i64,
                )
            };
            RuntimeBucket {
                start_micros,
                end_micros,
                rows: 0,
                stored_bytes: 0,
            }
        })
        .collect()
}

fn runtime_bucket_index(window: TimeRange, bucket_count: usize, timestamp_micros: i64) -> usize {
    let duration = window.duration_micros().max(1);
    let offset = timestamp_micros
        .saturating_sub(window.start.0)
        .clamp(0, duration.saturating_sub(1));
    (((offset as i128 * bucket_count as i128) / duration as i128) as usize)
        .min(bucket_count.saturating_sub(1))
}

fn runtime_status_rank(status: RuntimeStatus) -> u8 {
    match status {
        RuntimeStatus::Unknown => 0,
        RuntimeStatus::Interrupted => 1,
        RuntimeStatus::Delayed => 2,
        RuntimeStatus::Healthy => 3,
        RuntimeStatus::Idle => 4,
        RuntimeStatus::Unused => 5,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::{
        domain::{storage::primary_dataset_type, stream::Schema},
        shared::ids::Id,
    };

    fn definition() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("stream-1"),
            org_id: Id::from_string("org-1"),
            name: "app_logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields: Vec::new() },
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
    fn separates_window_storage_from_current_storage() {
        let window = TimeRange::new(TimestampMicros(100), TimestampMicros(200));
        let files = vec![file(0, 50, 10, 100), file(150, 250, 20, 200)];
        let summary = summarize_runtime(definition(), &files, window, 200, 4);

        assert_eq!(summary.rows, 10);
        assert_eq!(summary.stored_bytes, 100);
        assert_eq!(summary.current_stored_bytes, 300);
        assert_eq!(summary.first_received_at_micros, Some(0));
        assert_eq!(summary.last_received_at_micros, Some(250));
        assert_eq!(
            summary
                .buckets
                .iter()
                .map(|bucket| bucket.rows)
                .sum::<u64>(),
            10
        );
    }

    #[test]
    fn status_distinguishes_delayed_interrupted_and_unused() {
        let now = 10 * INTERRUPTED_LAG_MICROS;
        assert_eq!(
            runtime_status("app_logs", Some(now), now),
            RuntimeStatus::Healthy
        );
        assert_eq!(
            runtime_status("app_logs", Some(now - HEALTHY_LAG_MICROS - 1), now),
            RuntimeStatus::Delayed
        );
        assert_eq!(
            runtime_status("app_logs", Some(now - INTERRUPTED_LAG_MICROS - 1), now,),
            RuntimeStatus::Interrupted
        );
        assert_eq!(runtime_status("app_logs", None, now), RuntimeStatus::Unused);
    }

    #[test]
    fn event_driven_agent_stream_becomes_idle_instead_of_interrupted() {
        let now = 10 * INTERRUPTED_LAG_MICROS;
        assert_eq!(
            runtime_status(AGENT_STREAM, Some(now - HEALTHY_LAG_MICROS - 1), now),
            RuntimeStatus::Idle
        );
        assert_eq!(
            runtime_status(AGENT_STREAM, None, now),
            RuntimeStatus::Unused
        );
    }
}
