// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::read_model::RumReadModelReader,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_POINT_LIMIT: usize = 200;
const MAX_POINT_LIMIT: usize = 5_000;
const VITAL_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "session_id",
    "type",
    "page",
    "url",
    "application",
    "environment",
    "version",
    "browser",
    "country",
    "device",
    "lcp_ms",
    "fid_ms",
    "inp_ms",
    "cls",
    "ttfb_ms",
];
const API_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "type",
    "url",
    "duration_ms",
    "status",
];
const ERROR_COLUMNS: &[&str] = &["_timestamp", "fingerprint"];

#[derive(Debug, Default, Deserialize)]
pub(super) struct PerformanceQuery {
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct WebVitalPoint {
    ts_micros: i64,
    session_id: Option<String>,
    page: Option<String>,
    application: Option<String>,
    environment: Option<String>,
    version: Option<String>,
    browser: Option<String>,
    country: Option<String>,
    device: Option<String>,
    lcp_ms: Option<f64>,
    fid_ms: Option<f64>,
    inp_ms: Option<f64>,
    cls: Option<f64>,
    ttfb_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiPerformanceRow {
    url: String,
    count: i64,
    p50_ms: f64,
    p95_ms: f64,
    err_rate: f64,
}

#[derive(Debug, Serialize)]
pub(super) struct ErrorRatePoint {
    ts_micros: i64,
    count: i64,
}

#[derive(Default)]
struct ApiAccumulator {
    count: i64,
    errors: i64,
    durations: Vec<f64>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn vitals(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<PerformanceQuery>,
) -> Result<Json<Vec<WebVitalPoint>>> {
    let (range, limit) = resolve(query)?;
    let reader = reader(&state);
    let mut points = Vec::new();
    let stats = reader
        .visit_actions(&iam.org_id, range, VITAL_COLUMNS, |record| {
            if record.event_type != "view" {
                return;
            }
            let page = record.page_key().map(str::to_string);
            points.push(WebVitalPoint {
                ts_micros: record.timestamp_micros,
                session_id: non_empty(record.session_id),
                page,
                application: record.application,
                environment: record.environment,
                version: record.version,
                browser: record.browser,
                country: record.country,
                device: record.device,
                lcp_ms: finite(record.lcp_ms),
                fid_ms: finite(record.fid_ms),
                inp_ms: finite(record.inp_ms),
                cls: finite(record.cls),
                ttfb_ms: finite(record.ttfb_ms),
            });
        })
        .await?;
    points.sort_by_key(|point| point.ts_micros);
    points.truncate(limit);
    trace_stats("vitals", stats.files, stats.rows, points.len());
    Ok(Json(points))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn apis(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<PerformanceQuery>,
) -> Result<Json<Vec<ApiPerformanceRow>>> {
    let (range, limit) = resolve(query)?;
    let reader = reader(&state);
    let mut groups = HashMap::<String, ApiAccumulator>::new();
    let stats = reader
        .visit_actions(&iam.org_id, range, API_COLUMNS, |record| {
            if record.event_type != "resource" {
                return;
            }
            let Some(url) = record.url.filter(|url| !url.is_empty()) else {
                return;
            };
            let group = groups.entry(url).or_default();
            group.count += 1;
            group.errors += i64::from(record.status.unwrap_or_default() >= 400);
            if let Some(duration) = finite(record.duration_ms) {
                group.durations.push(duration);
            }
        })
        .await?;
    let mut rows = groups
        .into_iter()
        .map(|(url, mut group)| ApiPerformanceRow {
            url,
            count: group.count,
            p50_ms: percentile(&mut group.durations, 0.50),
            p95_ms: percentile(&mut group.durations, 0.95),
            err_rate: if group.count == 0 {
                0.0
            } else {
                group.errors as f64 / group.count as f64
            },
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.url.cmp(&right.url))
    });
    rows.truncate(limit);
    trace_stats("apis", stats.files, stats.rows, rows.len());
    Ok(Json(rows))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn errors(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<PerformanceQuery>,
) -> Result<Json<Vec<ErrorRatePoint>>> {
    let (range, limit) = resolve(query)?;
    let reader = reader(&state);
    let mut groups = HashMap::<i64, i64>::new();
    let stats = reader
        .visit_errors(&iam.org_id, range, ERROR_COLUMNS, |record| {
            *groups.entry(record.timestamp_micros).or_default() += 1;
        })
        .await?;
    let mut points = groups
        .into_iter()
        .map(|(ts_micros, count)| ErrorRatePoint { ts_micros, count })
        .collect::<Vec<_>>();
    points.sort_by_key(|point| point.ts_micros);
    points.truncate(limit);
    trace_stats("errors", stats.files, stats.rows, points.len());
    Ok(Json(points))
}

fn resolve(query: PerformanceQuery) -> Result<(TimeRange, usize)> {
    let now = TimestampMicros::now().0;
    let from = query
        .from
        .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
    let to = query.to.unwrap_or(now);
    if to <= from {
        return Err(Error::invalid("RUM range end must be greater than start"));
    }
    Ok((
        TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        query
            .limit
            .unwrap_or(DEFAULT_POINT_LIMIT)
            .clamp(1, MAX_POINT_LIMIT),
    ))
}

fn reader(state: &AppState) -> RumReadModelReader {
    RumReadModelReader::new(
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    )
}

fn percentile(values: &mut [f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn trace_stats(endpoint: &str, files: usize, rows: usize, returned: usize) {
    tracing::debug!(
        endpoint,
        scanned_files = files,
        scanned_rows = rows,
        returned,
        "RUM performance read model completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&mut [10.0, 20.0, 30.0, 40.0], 0.95), 40.0);
    }
}
