// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/trace/:trace_id` —— 返回完整 span 树（web-investigation-shell）。
//!
//! 走现有 `QueryService` 在 `traces` 流上执行 SQL；行映射到 `Span` 结构。
//! 行映射 + 截断逻辑提到 `crate::app::web::trace::view`，与 intelligence MCP `get_trace`
//! tool 共用同一份实现（intelligence-mcp-dispatcher）。

use axum::{
    Extension, Router,
    extract::{Path, State},
    response::Json,
    routing::get,
};

use self::{
    request::validate_trace_id,
    response::{SPAN_LIMIT, TraceResponse},
};
use crate::{
    api::AppState,
    app::{iam::IamContext, web::trace::view::rows_to_spans},
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

mod list;
mod request;
mod response;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/trace/{trace_id}", get(trace))
        .route("/traces", get(list::list_traces))
}

/// 默认查询窗口：过去 24h（trace_id 是稀疏键，窗口仅做分区裁剪）。
const DEFAULT_WINDOW_SECS: i64 = 24 * 3600;

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
        .filter(|s| s.stream_type == StreamType::Traces)
        .collect();
    choose_traces_stream(traces)
}

/// The web trace endpoints query the standard OTEL column contract. Prefer a
/// stream that actually carries that contract instead of relying on repository
/// row order: an older or sample traces stream may use `span.name`, or omit
/// explicit start/end columns, and would make every trace query fail to plan.
fn choose_traces_stream(mut traces: Vec<StreamDefinition>) -> Option<StreamDefinition> {
    traces.sort_by(|left, right| {
        trace_stream_rank(right)
            .cmp(&trace_stream_rank(left))
            .then_with(|| left.name.cmp(&right.name))
    });
    traces.into_iter().next()
}

fn trace_stream_rank(stream: &StreamDefinition) -> u8 {
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
    match (canonical, stream.name.as_str() == "default") {
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn trace(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(trace_id): Path<String>,
) -> Result<Json<TraceResponse>, Error> {
    validate_trace_id(&trace_id).map_err(Error::invalid)?;

    let now = TimestampMicros::now();
    let range = TimeRange::new(
        TimestampMicros(now.0 - DEFAULT_WINDOW_SECS * 1_000_000),
        now,
    );
    let Some(stream) = resolve_traces_stream(&state, &ctx.org_id).await else {
        return Err(Error::not_found(format!("trace {trace_id} not found")));
    };
    // `SELECT *`：OTLP ingest 把 span/resource 属性扁平成各自的带点列（无单独的
    // `attributes`/`events` JSON 列），所以取全列让 `rows_to_spans` 按 canonical 名
    // 提取核心字段、把其余扁平列聚成 attributes。按 `_timestamp`（恒在）排序做 LIMIT 裁剪。
    let sql = format!(
        "SELECT *
         FROM \"{stream_ident}\"
         WHERE trace_id = '{trace_id}'
         ORDER BY _timestamp ASC
         LIMIT {limit}",
        stream_ident = escape_sql_ident(&stream),
        limit = SPAN_LIMIT + 1,
    );

    let req = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql,
        time_range: range,
        stream: Some(StreamHint {
            name: stream,
            stream_type: StreamType::Traces,
        }),
        limit: Some(SPAN_LIMIT + 1),
        federation_clusters: Vec::new(),
    };

    let out = state.query.run(req).await?;
    let (spans, truncated) = rows_to_spans(&out);

    if spans.is_empty() {
        return Err(Error::not_found(format!("trace {trace_id} not found")));
    }

    Ok(Json(TraceResponse::new(trace_id, spans, truncated)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::{FieldDef, FieldType, Schema};

    fn trace_stream(name: &str, fields: &[&str]) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(name),
            org_id: Id::from_string("org-1"),
            name: name.to_string(),
            stream_type: StreamType::Traces,
            schema: Schema {
                fields: fields
                    .iter()
                    .map(|name| FieldDef {
                        name: (*name).to_string(),
                        data_type: FieldType::Utf8,
                        nullable: true,
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
        let required = [
            "trace_id",
            "span_id",
            "service.name",
            "name",
            "start_time_unix_nano",
            "end_time_unix_nano",
            "status_code",
        ];
        let selected = choose_traces_stream(vec![
            trace_stream("default", &["trace_id", "span_id", "span.name"]),
            trace_stream(
                "sample_app_traces",
                &["trace_id", "span_id", "service.name", "span.name"],
            ),
            trace_stream("topology_traces", &required),
        ]);

        assert_eq!(
            selected.as_ref().map(|stream| stream.name.as_str()),
            Some("topology_traces")
        );
    }
}
