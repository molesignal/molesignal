// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/metrics/catalog`：按指标目录列出当前 org 的全部指标（一个指标流装一个来源的
//! 全部指标），给前端 Metrics 页面填左侧的 metric 选择面板。
//!
//! 与顶层的 Prometheus scrape 端点 [`super::scrape_routes`] 不同：那是吐进程自身的
//! 监控指标，这里是用户能用 PromQL 查询的 metric 目录。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{iam::permission, metrics::MetricDirectoryEntry, stream::MOLESIGNAL_SYSTEM_STREAM},
    shared::Result,
};

const DEFAULT_PAGE_SIZE: usize = 20;
pub(super) const MAX_PAGE_SIZE: usize = 100;

mod cursor;

pub fn routes() -> Router<AppState> {
    Router::new().route("/metrics/catalog", get(list))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MetricCatalogEntry {
    /// PromQL metric identifier: the `metric_name` of its samples.
    name: String,
    /// Prometheus metric kind：按样本带的 `metric_kind` 推断；Prometheus 指标没有它，按
    /// classic histogram sibling 与命名约定推断。
    metric_type: MetricType,
    /// 这个指标的样本带过的标签名，覆盖持有它的每个流。
    labels: Vec<String>,
    /// 这个指标的列数：标签、值与时间，方便前端展示。
    field_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MetricType {
    Counter,
    Histogram,
    Gauge,
}

#[derive(Debug, Default, Deserialize)]
struct MetricCatalogQuery {
    q: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

type MetricCatalogPageContext = (Option<String>, usize, Option<(CursorDirection, String)>);

#[permission(any("metrics.read", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(request): Query<MetricCatalogQuery>,
) -> Result<Json<CursorPage<MetricCatalogEntry>>> {
    let (query, page_size, boundary) = resolve_page_context(&state, &ctx, request)?;

    // 自身遥测只给平台范围看：`_sys` 组织里的其他身份看不到 `_molesignal` 的指标。
    let self_telemetry = ctx.is_system_scope() && ctx.org_id == state.iam.system_org_id;

    let entries = state
        .telemetry
        .metric_directory
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|entry| self_telemetry || entry.stream_name != MOLESIGNAL_SYSTEM_STREAM)
        .collect::<Vec<_>>();

    let mut metrics = build_catalog(&entries);

    if let Some(query) = query.as_deref() {
        metrics.retain(|metric| metric_matches(metric, query));
    }

    let direction = boundary.as_ref().map(|boundary| boundary.0);

    if let Some((direction, metric_name)) = boundary.as_ref() {
        metrics.retain(|metric| match direction {
            CursorDirection::After => metric.name > *metric_name,
            CursorDirection::Before => metric.name < *metric_name,
        });

        if *direction == CursorDirection::Before {
            metrics.reverse();
        }
    }

    metrics.truncate(page_size.saturating_add(1));
    let page = trim_cursor_page(metrics, page_size, direction);

    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|metric| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &ctx.org_id,
                    query.clone(),
                    page_size,
                    CursorDirection::Before,
                    metric.name.clone(),
                )
            })
            .transpose()?
    } else {
        None
    };

    let next_cursor = if page.has_next {
        page.items
            .last()
            .map(|metric| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &ctx.org_id,
                    query.clone(),
                    page_size,
                    CursorDirection::After,
                    metric.name.clone(),
                )
            })
            .transpose()?
    } else {
        None
    };

    Ok(Json(CursorPage {
        items: page.items,
        has_more: next_cursor.is_some(),
        next_cursor,
        previous_cursor,
    }))
}

fn resolve_page_context(
    state: &AppState,
    ctx: &IamContext,
    request: MetricCatalogQuery,
) -> Result<MetricCatalogPageContext> {
    if let Some(token) = request.cursor.as_deref() {
        let payload = cursor::decode(state.iam.service.as_ref(), &ctx.org_id, token)?;
        let requested_query = normalize_query(request.q);

        if (requested_query.is_some() && requested_query != payload.query)
            || request
                .limit
                .is_some_and(|limit| limit.clamp(1, MAX_PAGE_SIZE) != payload.page_size)
        {
            return Err(crate::shared::Error::invalid(
                "metric catalog cursor does not match active query",
            ));
        }

        return Ok((
            payload.query,
            payload.page_size,
            Some((payload.direction, payload.metric_name)),
        ));
    }

    Ok((
        normalize_query(request.q),
        request
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
        None,
    ))
}

fn normalize_query(query: Option<String>) -> Option<String> {
    query.and_then(|query| {
        let query = query.trim().to_lowercase();
        (!query.is_empty()).then(|| query.chars().take(256).collect())
    })
}

fn metric_matches(metric: &MetricCatalogEntry, query: &str) -> bool {
    metric.name.to_lowercase().contains(query)
        || metric.metric_type.as_str().contains(query)
        || metric
            .labels
            .iter()
            .any(|label| label.to_lowercase().contains(query))
}

impl MetricType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Histogram => "histogram",
            Self::Gauge => "gauge",
        }
    }
}

/// One entry per metric, with the labels of every stream holding it, in name order.
fn build_catalog(entries: &[MetricDirectoryEntry]) -> Vec<MetricCatalogEntry> {
    let names = entries
        .iter()
        .map(|entry| entry.metric_name.as_str())
        .collect::<HashSet<_>>();

    let mut metrics = BTreeMap::<&str, (Option<&str>, BTreeSet<&str>)>::new();

    for entry in entries {
        let (kind, labels) = metrics.entry(entry.metric_name.as_str()).or_default();

        if kind.is_none() {
            *kind = entry.metric_kind.as_deref();
        }

        labels.extend(entry.labels.iter().map(String::as_str));
    }

    metrics
        .into_iter()
        .map(|(name, (kind, labels))| MetricCatalogEntry {
            metric_type: infer_metric_type(name, kind, &names),
            name: name.to_string(),
            field_count: labels.len() + 2,
            labels: labels.into_iter().map(str::to_string).collect(),
        })
        .collect()
}

fn infer_metric_type(name: &str, kind: Option<&str>, metric_names: &HashSet<&str>) -> MetricType {
    // OTLP and self telemetry samples say what they are. Summary has the shape of a histogram
    // in today's normalized model, so histogram is the closest useful presentation.
    match kind {
        Some("histogram" | "exponential_histogram" | "summary") => return MetricType::Histogram,
        Some("counter") => return MetricType::Counter,
        Some("gauge" | "up_down_counter") => return MetricType::Gauge,
        _ => {}
    }

    let normalized = name.to_ascii_lowercase();

    // Classic Prometheus histograms arrive as three metrics: <family>_bucket, <family>_sum and
    // <family>_count. Metadata is not persisted, so recognize the siblings as one family.
    if normalized.ends_with("_bucket") {
        return MetricType::Histogram;
    }

    if let Some(family) = normalized
        .strip_suffix("_sum")
        .or_else(|| normalized.strip_suffix("_count"))
    {
        let bucket_name = format!("{family}_bucket");

        if metric_names
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&bucket_name))
        {
            return MetricType::Histogram;
        }
    }

    if normalized.ends_with("_total")
        || normalized.ends_with("_count")
        || normalized.ends_with("_sum")
    {
        return MetricType::Counter;
    }

    MetricType::Gauge
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn entry(
        stream: &str,
        metric: &str,
        kind: Option<&str>,
        labels: &[&str],
    ) -> MetricDirectoryEntry {
        MetricDirectoryEntry {
            stream_id: Id::from_string(format!("id-{stream}")),
            stream_name: stream.into(),
            metric_name: metric.into(),
            metric_kind: kind.map(str::to_string),
            labels: labels.iter().map(|label| label.to_string()).collect(),
            first_seen: TimestampMicros(0),
            last_seen: TimestampMicros(3_600_000_000),
        }
    }

    #[test]
    fn infers_scalar_metric_types_from_prometheus_names() {
        let names = HashSet::new();

        assert_eq!(
            infer_metric_type("http_requests_total", None, &names),
            MetricType::Counter
        );
        assert_eq!(
            infer_metric_type("process_resident_memory_bytes", None, &names),
            MetricType::Gauge
        );
    }

    #[test]
    fn infers_otlp_kinds_and_classic_histograms() {
        let names = HashSet::from([
            "http_request_duration_seconds_bucket",
            "http_request_duration_seconds_sum",
            "http_request_duration_seconds_count",
        ]);

        assert_eq!(
            infer_metric_type("db.query.duration", Some("histogram"), &HashSet::new()),
            MetricType::Histogram
        );
        assert_eq!(
            infer_metric_type("queue.depth_total", Some("gauge"), &HashSet::new()),
            MetricType::Gauge
        );
        assert_eq!(
            infer_metric_type("http_request_duration_seconds_sum", None, &names),
            MetricType::Histogram
        );
    }

    #[test]
    fn one_entry_per_metric_with_the_labels_of_every_stream() {
        let catalog = build_catalog(&[
            entry("prod", "up", None, &["instance", "job"]),
            entry("staging", "up", None, &["job", "region"]),
            entry("prod", "http_requests_total", None, &["path"]),
        ]);

        assert_eq!(
            catalog
                .iter()
                .map(|metric| metric.name.as_str())
                .collect::<Vec<_>>(),
            ["http_requests_total", "up"]
        );
        assert_eq!(catalog[0].metric_type, MetricType::Counter);
        assert_eq!(catalog[1].labels, ["instance", "job", "region"]);
        assert_eq!(catalog[1].field_count, 5);
    }
}
