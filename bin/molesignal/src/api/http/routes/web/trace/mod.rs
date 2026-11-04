// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/trace/:trace_id` —— 返回完整 span 树（web-investigation-shell）。
//!
//! 走现有 `QueryService` 在 `traces` 流上执行 SQL；行映射到 `Span` 结构。
//! 行映射 + 截断逻辑提到 `crate::app::web::trace::view`，与 agent MCP `get_trace`
//! tool 共用同一份实现（agent-mcp-dispatcher）。

use axum::{
    Extension, Router,
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post},
};

use self::{
    request::{TraceWindow, longest_range, validate_trace_id},
    response::{SPAN_LIMIT, TraceResponse},
};
use crate::{
    api::AppState,
    app::{iam::IamContext, trace::summary::carries_summaries, web::trace::view::rows_to_spans},
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::{StreamDefinition, StreamType},
    },
    infra::query::escape_sql_ident,
    shared::{
        Error,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod expression;
mod field_stats;
mod filter;
mod latency;
mod linked;
mod list;
mod locate;
mod messaging_latency;
mod red;
mod request;
mod response;
mod spans;
mod time_spent;

/// Where a Span keeps the HTTP status it answered with, the current name first.
pub(super) const HTTP_STATUS_FIELDS: [&str; 2] = ["http.response.status_code", "http.status_code"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/trace/{traceId}", get(trace))
        .route("/trace/{traceId}/linked", get(linked::linked))
        .route("/traces", get(list::list_traces))
        .route("/traces/spans", get(spans::list_spans))
        .route("/traces/histogram", get(list::histogram::histogram))
        .route("/traces/field-stats", post(field_stats::field_stats))
        .route("/traces/red", get(red::red))
        .route("/traces/red/breakdown", get(red::breakdown))
        .route("/traces/time-spent", get(time_spent::time_spent))
        .route(
            "/traces/messaging-latency",
            get(messaging_latency::messaging_latency),
        )
}

/// 解析当前 org 要查的 traces 流名：优先选择符合标准 OTEL 字段契约的流，同等
/// 兼容性下再优先 `default`。没有任何 traces 流 → `None`，调用方应返空而非报
/// "stream not found"。返回的流名在 SQL `FROM` 处必须用双引号包裹（`default`
/// 是 SQL 保留字）。
pub(crate) async fn resolve_traces_stream(state: &AppState, org_id: &Id) -> Option<String> {
    resolve_traces_stream_definition(state, org_id)
        .await
        .map(|stream| stream.name)
}

pub(crate) async fn resolve_traces_stream_definition(
    state: &AppState,
    org_id: &Id,
) -> Option<StreamDefinition> {
    let traces: Vec<StreamDefinition> = state
        .telemetry
        .streams
        .list(org_id)
        .await
        .ok()?
        .into_iter()
        .filter(|s| s.stream_type == StreamType::TRACES)
        .collect();

    choose_traces_stream(traces)
}

/// The web trace endpoints query the standard OTEL column contract. Prefer a
/// stream that actually carries that contract instead of relying on repository
/// row order: an older or sample traces stream may use `span.name`, or omit
/// explicit start/end columns, and would make every trace query fail to plan.
/// Among those, prefer a stream with summaries: the trace list reads nothing
/// else, so a stream written only through native intake must not hide one.
fn choose_traces_stream(mut traces: Vec<StreamDefinition>) -> Option<StreamDefinition> {
    traces.sort_by(|left, right| {
        trace_stream_rank(right)
            .cmp(&trace_stream_rank(left))
            .then_with(|| left.name.cmp(&right.name))
    });

    traces.into_iter().next()
}

fn trace_stream_rank(stream: &StreamDefinition) -> (bool, bool, bool) {
    const REQUIRED_FIELDS: [&str; 7] = [
        "trace_id",
        "span_id",
        "service.name",
        "name",
        "start_time_unix_nano",
        "end_time_unix_nano",
        "status_code",
    ];

    let canonical = REQUIRED_FIELDS.iter().all(|required| {
        stream
            .schema
            .fields
            .iter()
            .any(|field| field.name == *required)
    });

    (
        canonical,
        carries_summaries(&stream.schema),
        stream.name == "default",
    )
}

#[permission(any("traces.read", "sys.telemetry.read"))]
async fn trace(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(trace_id): Path<String>,
    Query(window): Query<TraceWindow>,
) -> Result<Json<TraceResponse>, Error> {
    validate_trace_id(&trace_id).map_err(Error::invalid)?;

    let now = TimestampMicros::now();
    let range = window.range(now)?;

    let Some(stream) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Err(Error::not_found(format!("trace {trace_id} not found")));
    };

    let spans_in = |range: TimeRange| spans_request(&ctx, &stream.name, &trace_id, range);

    let out = state.query.run(spans_in(range)).await?;
    let (mut spans, mut truncated) = rows_to_spans(&out);

    // 没说在哪儿、最近一天里又没有：按摘要行找到它的起止，再读这段时间的 Span。
    if spans.is_empty()
        && !window.given()
        && carries_summaries(&stream.schema)
        && let Some(found) =
            locate::trace_range(&state, &ctx, &stream.name, &trace_id, longest_range(now)).await?
    {
        let out = state.query.run(spans_in(found)).await?;
        (spans, truncated) = rows_to_spans(&out);
    }

    if spans.is_empty() {
        return Err(Error::not_found(format!("trace {trace_id} not found")));
    }

    Ok(Json(TraceResponse::new(trace_id, spans, truncated)))
}

/// `stream` 里 `range` 中开始的 `trace_id` 的 Span。
///
/// `SELECT *`：OTLP intake 把 span/resource 属性扁平成各自的带点列（无单独的
/// `attributes`/`events` JSON 列），所以取全列让 `rows_to_spans` 按 canonical 名
/// 提取核心字段、把其余扁平列聚成 attributes。按 `_timestamp`（恒在）排序做 LIMIT 裁剪。
fn spans_request(ctx: &IamContext, stream: &str, trace_id: &str, range: TimeRange) -> QueryRequest {
    let sql = format!(
        "SELECT *
         FROM \"{stream_ident}\"
         WHERE trace_id = '{trace_id}'
         ORDER BY _timestamp ASC
         LIMIT {limit}",
        stream_ident = escape_sql_ident(stream),
        limit = SPAN_LIMIT + 1,
    );

    QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql,
        time_range: range,
        stream: Some(StreamHint {
            name: stream.to_owned(),
            stream_type: StreamType::TRACES,
        }),
        limit: Some(SPAN_LIMIT + 1),
        step: None,
        federation_clusters: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Schema},
        shared::trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD,
            TRACE_SUMMARY_START_NS_FIELD,
        },
    };

    const CANONICAL_FIELDS: [&str; 7] = [
        "trace_id",
        "span_id",
        "service.name",
        "name",
        "start_time_unix_nano",
        "end_time_unix_nano",
        "status_code",
    ];

    fn trace_stream(name: &str, fields: &[&str]) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(name),
            org_id: Id::from_string("org-1"),
            name: name.to_string(),
            stream_type: StreamType::TRACES,
            schema: Schema {
                fields: fields
                    .iter()
                    .map(|name| FieldDef {
                        name: (*name).to_string(),
                        data_type: FieldType::Utf8,
                        nullable: true,
                        index_type: None,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    })
                    .collect(),
            },
            retention: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn canonical_trace_stream_wins_over_incompatible_default_or_sample_streams() {
        let selected = choose_traces_stream(vec![
            trace_stream("default", &["trace_id", "span_id", "span.name"]),
            trace_stream(
                "sample_app_traces",
                &["trace_id", "span_id", "service.name", "span.name"],
            ),
            trace_stream("topology_traces", &CANONICAL_FIELDS),
        ]);

        assert_eq!(
            selected.as_ref().map(|stream| stream.name.as_str()),
            Some("topology_traces")
        );
    }

    #[test]
    fn a_stream_with_summaries_wins_over_a_default_stream_without_them() {
        let summarized: Vec<&str> = CANONICAL_FIELDS
            .into_iter()
            .chain([
                TRACE_SUMMARY_MARKER_FIELD,
                TRACE_SUMMARY_START_NS_FIELD,
                TRACE_SUMMARY_DURATION_NS_FIELD,
                TRACE_SUMMARY_SPAN_COUNT_FIELD,
                TRACE_SUMMARY_ERROR_COUNT_FIELD,
            ])
            .collect();

        let selected = choose_traces_stream(vec![
            trace_stream("default", &CANONICAL_FIELDS),
            trace_stream("prod", &summarized),
        ]);

        assert_eq!(
            selected.as_ref().map(|stream| stream.name.as_str()),
            Some("prod")
        );

        let selected = choose_traces_stream(vec![
            trace_stream("checkout", &summarized),
            trace_stream("default", &summarized),
        ]);

        assert_eq!(
            selected.as_ref().map(|stream| stream.name.as_str()),
            Some("default")
        );
    }
}
