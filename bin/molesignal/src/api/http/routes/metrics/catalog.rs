// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/metrics/catalog`：列出当前 org 下所有 `stream_type = metrics`
//! 的 stream，给前端 Metrics 页面填左侧的 metric 选择面板。
//!
//! 与顶层的 Prometheus scrape 端点 [`super::scrape_routes`] 不同：那是吐进程自身的
//! 监控指标，这里是用户能用 PromQL 查询的 metric 目录。

use std::collections::HashSet;

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
    domain::{
        iam::permission,
        metrics::{
            METRIC_NAME_FIELD, is_metric_identity_storage_field,
            is_prometheus_exemplar_storage_field,
        },
        query::{QueryLanguage, QueryRequest, StreamHint},
        storage::PhysicalDatasetKind,
        stream::{FieldDef, FieldType, MOLESIGNAL_SYSTEM_STREAM, StreamDefinition, StreamType},
    },
    shared::{
        Result,
        metrics::gather_structured,
        time::{TimeRange, TimestampMicros},
    },
};

const SELF_METRIC_DISCOVERY_WINDOW_US: i64 = 60 * 60 * 1_000_000;
const MAX_SELF_METRIC_NAMES: usize = 10_000;
const DEFAULT_PAGE_SIZE: usize = 20;
pub(super) const MAX_PAGE_SIZE: usize = 100;

mod cursor;

pub fn routes() -> Router<AppState> {
    Router::new().route("/metrics/catalog", get(list))
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MetricCatalogEntry {
    /// PromQL metric identifier. Self telemetry entries are logical names
    /// resolved by the PromQL engine to the protected `_molesignal` stream.
    name: String,
    /// Prometheus metric kind。存量数据没有单独的 metadata 表，因此根据
    /// stream schema、classic histogram sibling 与命名约定推断。
    metric_type: MetricType,
    /// 可用作 PromQL label 的 UTF-8 字段名。
    labels: Vec<String>,
    /// 该字段的可用列总数（含未索引），方便前端展示。
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

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(request): Query<MetricCatalogQuery>,
) -> Result<Json<CursorPage<MetricCatalogEntry>>> {
    let (query, page_size, boundary) = resolve_page_context(&state, &ctx, request)?;
    // 复用 StreamRead 权限：能看 streams 就能看 metric 目录。
    let metric_streams = state
        .telemetry
        .streams
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|s| s.stream_type == StreamType::Metrics)
        .collect::<Vec<_>>();
    let self_metric_stream = metric_streams
        .iter()
        .find(|stream| stream.name == MOLESIGNAL_SYSTEM_STREAM);
    let self_metric_names = if ctx.is_system_scope()
        && ctx.org_id == state.iam.system_org_id
        && self_metric_stream.is_some()
    {
        let persisted_schema_ready = self_metric_stream.is_some_and(|stream| {
            stream
                .schema
                .fields
                .iter()
                .any(|field| field.name == METRIC_NAME_FIELD)
        });
        discover_self_metric_names(&state, &ctx, persisted_schema_ready).await
    } else {
        HashSet::new()
    };
    let mut metrics = build_catalog(&metric_streams, &self_metric_names);
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

/// Self telemetry deliberately stores hundreds of logical metrics in one
/// protected physical stream. Discover active names from the recent persisted
/// window and merge the current process snapshot so the catalog also works
/// before the next parquet flush.
async fn discover_self_metric_names(
    state: &AppState,
    ctx: &IamContext,
    persisted_schema_ready: bool,
) -> HashSet<String> {
    let mut names = gather_structured()
        .into_iter()
        .map(|sample| sample.metric_name)
        .collect::<HashSet<_>>();
    if !persisted_schema_ready {
        return names;
    }
    let now = TimestampMicros::now();
    let request = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: format!(
            "SELECT DISTINCT {METRIC_NAME_FIELD} FROM \"{MOLESIGNAL_SYSTEM_STREAM}\" \
             WHERE {METRIC_NAME_FIELD} IS NOT NULL \
             ORDER BY {METRIC_NAME_FIELD} LIMIT {MAX_SELF_METRIC_NAMES}"
        ),
        time_range: TimeRange::new(
            TimestampMicros(now.0.saturating_sub(SELF_METRIC_DISCOVERY_WINDOW_US)),
            now,
        ),
        stream: Some(StreamHint {
            name: MOLESIGNAL_SYSTEM_STREAM.to_string(),
            stream_type: StreamType::Metrics,
        }),
        limit: Some(MAX_SELF_METRIC_NAMES),
        federation_clusters: Vec::new(),
    };
    match state
        .query
        .run_dataset(request, PhysicalDatasetKind::MetricCatalog)
        .await
    {
        Ok(result) => {
            if let Some(index) = result
                .columns
                .iter()
                .position(|column| column == METRIC_NAME_FIELD)
            {
                names.extend(result.rows.into_iter().filter_map(|row| {
                    row.get(index)
                        .and_then(serde_json::Value::as_str)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                }));
            }
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to discover persisted self telemetry metric names"
            );
        }
    }
    names
}

fn build_catalog(
    metric_streams: &[StreamDefinition],
    self_metric_names: &HashSet<String>,
) -> Vec<MetricCatalogEntry> {
    let expanded = metric_streams
        .iter()
        .flat_map(|stream| {
            if stream.name == MOLESIGNAL_SYSTEM_STREAM && !self_metric_names.is_empty() {
                self_metric_names
                    .iter()
                    .cloned()
                    .map(|name| (name, stream))
                    .collect::<Vec<_>>()
            } else {
                vec![(stream.name.clone(), stream)]
            }
        })
        .collect::<Vec<_>>();
    let metric_names = expanded
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<HashSet<_>>();
    let mut metrics = expanded
        .into_iter()
        .map(|(name, stream)| MetricCatalogEntry {
            metric_type: infer_metric_type(&name, &stream.schema.fields, &metric_names),
            name,
            labels: metric_labels(&stream.schema.fields),
            field_count: visible_field_count(&stream.schema.fields),
        })
        .collect::<Vec<_>>();
    metrics.sort_by(|left, right| left.name.cmp(&right.name));
    metrics
}

fn infer_metric_type(
    name: &str,
    fields: &[FieldDef],
    metric_names: &HashSet<String>,
) -> MetricType {
    let normalized = name.to_ascii_lowercase();
    let has_field = |field_name: &str| fields.iter().any(|field| field.name == field_name);

    // OTLP Histogram / ExponentialHistogram rows carry count + sum and no
    // scalar value. Summary has the same shape in today's normalized model,
    // so histogram is the closest useful Prometheus explorer presentation.
    if has_field("count") && has_field("sum") && !has_field("value") {
        return MetricType::Histogram;
    }

    // Classic Prometheus histograms arrive as three remote_write streams:
    // <family>_bucket, <family>_sum and <family>_count. Metadata is not
    // currently persisted, so recognize the siblings as one histogram family.
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

/// PromQL evaluator treats UTF-8 sample fields as labels whether or not they
/// have a search index. Internal container identity and exemplar sidecars are
/// storage metadata, not user-visible labels.
fn metric_labels(fields: &[FieldDef]) -> Vec<String> {
    let mut labels = fields
        .iter()
        .filter(|field| field.data_type == FieldType::Utf8)
        .map(|f| f.name.clone())
        .filter(|name| {
            name != "value"
                && name != "_timestamp"
                && !is_metric_identity_storage_field(name)
                && !is_prometheus_exemplar_storage_field(name)
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn visible_field_count(fields: &[FieldDef]) -> usize {
    fields
        .iter()
        .filter(|field| !is_prometheus_exemplar_storage_field(&field.name))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldType, Schema},
        shared::ids::Id,
    };

    fn field(name: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type: FieldType::Float64,
            nullable: false,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    #[test]
    fn infers_scalar_metric_types_from_prometheus_names() {
        let names = HashSet::new();
        let value = [field("value")];

        assert_eq!(
            infer_metric_type("http_requests_total", &value, &names),
            MetricType::Counter
        );
        assert_eq!(
            infer_metric_type("process_resident_memory_bytes", &value, &names),
            MetricType::Gauge
        );
    }

    #[test]
    fn infers_otlp_and_classic_histograms() {
        let names = [
            "http_request_duration_seconds_bucket".into(),
            "http_request_duration_seconds_sum".into(),
            "http_request_duration_seconds_count".into(),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            infer_metric_type(
                "db_query_duration_seconds",
                &[field("count"), field("sum")],
                &HashSet::new(),
            ),
            MetricType::Histogram
        );
        assert_eq!(
            infer_metric_type(
                "http_request_duration_seconds_sum",
                &[field("value")],
                &names,
            ),
            MetricType::Histogram
        );
    }

    #[test]
    fn hides_internal_exemplar_storage_fields_from_catalog_metadata() {
        let mut fields = vec![field("value")];
        for name in [
            crate::domain::metrics::PROMETHEUS_EXEMPLAR_MARKER_FIELD,
            crate::domain::metrics::PROMETHEUS_EXEMPLAR_VALUE_FIELD,
            crate::domain::metrics::PROMETHEUS_EXEMPLAR_LABELS_FIELD,
        ] {
            let mut internal = field(name);
            internal.indexed = true;
            fields.push(internal);
        }

        assert!(metric_labels(&fields).is_empty());
        assert_eq!(visible_field_count(&fields), 1);
    }

    #[test]
    fn exposes_unindexed_utf8_fields_as_promql_labels() {
        let mut service = field("service.name");
        service.data_type = FieldType::Utf8;
        let mut metric_name = field(METRIC_NAME_FIELD);
        metric_name.data_type = FieldType::Utf8;
        let mut metric_kind = field(crate::domain::metrics::METRIC_KIND_FIELD);
        metric_kind.data_type = FieldType::Utf8;

        assert_eq!(
            metric_labels(&[service, metric_name, metric_kind]),
            vec!["service.name"]
        );
    }

    #[test]
    fn expands_self_telemetry_stream_into_logical_metrics() {
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("_sys"),
            name: MOLESIGNAL_SYSTEM_STREAM.into(),
            stream_type: StreamType::Metrics,
            schema: Schema {
                fields: vec![field("value")],
            },
            retention: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };
        let names = HashSet::from([
            "process_resident_memory_bytes".to_string(),
            "http_requests_total".to_string(),
        ]);

        let catalog = build_catalog(&[stream], &names);

        assert_eq!(
            catalog
                .iter()
                .map(|metric| metric.name.as_str())
                .collect::<Vec<_>>(),
            vec!["http_requests_total", "process_resident_memory_bytes"]
        );
        assert_eq!(catalog[0].metric_type, MetricType::Counter);
        assert_eq!(catalog[1].metric_type, MetricType::Gauge);
    }
}
