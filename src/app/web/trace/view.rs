// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace 视图共享工具：`rows_to_spans` + `Span` / `SpanEvent` / `TraceResponse` DTO。
//!
//! 同源数据来自 traces 流（DataFusion 查 `SELECT ... FROM traces WHERE trace_id=...`）；
//! - web handler (`src/api/http/routes/web/trace.rs`) 调它生成 `/api/v1/web/trace/{id}` 响应；
//! - intelligence MCP `get_trace` tool（ dispatcher）调它把行转 span 树后 wrap 成 `ToolContent::Json`。
//!
//! 阈值（与原 handler 保持一致）：
//! - `SPAN_LIMIT = 100_000`：DataFusion LIMIT + 1 探测溢出；溢出 → `truncated: true`
//! - `RESPONSE_HARD_CAP = 32 MiB`：序列化累积超阈值时停止追加 spans，flag truncated

use std::collections::HashSet;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    domain::query::QueryResult,
    shared::trace_normalization::{
        CANONICAL_SPAN_SCHEMA_VERSION, CanonicalLink, CanonicalResource, CanonicalScope,
        PartialReason, SEMCONV_VERSION, SamplingReason,
    },
};

#[derive(Debug, Serialize)]
pub struct Span {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub service: String,
    pub operation: String,
    pub start_ns: i64,
    pub end_ns: i64,
    pub duration_ns: u64,
    pub kind: i32,
    pub status: String,
    pub status_message: Option<String>,
    pub trace_flags: u32,
    pub trace_state: String,
    pub resource: CanonicalResource,
    pub scope: CanonicalScope,
    pub attributes: Value,
    pub events: Vec<SpanEvent>,
    pub links: Vec<CanonicalLink>,
    pub dropped_attributes_count: u32,
    pub dropped_events_count: u32,
    pub dropped_links_count: u32,
    pub schema_version: u16,
    pub semantic_conventions_version: String,
    pub sampling_reason: SamplingReason,
    pub partial: bool,
    pub partial_reasons: Vec<PartialReason>,
    pub late: bool,
    pub duplicate: bool,
    pub conflict: bool,
}

#[derive(Debug, Serialize)]
pub struct SpanEvent {
    pub ts_ns: i64,
    pub name: String,
    pub attributes: Value,
    pub dropped_attributes_count: u32,
}

#[derive(Debug, Serialize)]
pub struct TraceResponse {
    pub trace_id: String,
    pub root_span_id: String,
    pub spans: Vec<Span>,
    pub truncated: bool,
    pub partial: bool,
    pub partial_reasons: Vec<PartialReason>,
    pub sampling_reasons: Vec<SamplingReason>,
    pub late_span_count: usize,
    pub duplicate_span_count: usize,
    pub conflict_span_count: usize,
}

impl TraceResponse {
    pub fn new(trace_id: String, spans: Vec<Span>, truncated: bool) -> Self {
        let root_span_id = spans
            .iter()
            .find(|span| {
                span.parent_span_id.is_none() || span.parent_span_id.as_deref() == Some("")
            })
            .map(|span| span.span_id.clone())
            .unwrap_or_else(|| {
                spans
                    .first()
                    .map(|span| span.span_id.clone())
                    .unwrap_or_default()
            });
        let mut partial_reasons = Vec::new();
        let mut sampling_reasons = Vec::new();
        for span in &spans {
            for reason in &span.partial_reasons {
                if !partial_reasons.contains(reason) {
                    partial_reasons.push(*reason);
                }
            }
            if !sampling_reasons.contains(&span.sampling_reason) {
                sampling_reasons.push(span.sampling_reason);
            }
        }
        Self {
            trace_id,
            root_span_id,
            partial: spans.iter().any(|span| span.partial),
            late_span_count: spans.iter().filter(|span| span.late).count(),
            duplicate_span_count: spans.iter().filter(|span| span.duplicate).count(),
            conflict_span_count: spans.iter().filter(|span| span.conflict).count(),
            spans,
            truncated,
            partial_reasons,
            sampling_reasons,
        }
    }
}

pub const SPAN_LIMIT: usize = 100_000;
pub const RESPONSE_HARD_CAP: usize = 32 * 1024 * 1024;

/// 把 `traces` 流的 `QueryResult` 行转成 `Span` 列表。
///
/// 返回 `(spans, truncated)`：行数 > SPAN_LIMIT，或累计字节 > RESPONSE_HARD_CAP 时
/// 截断并把 `truncated` 标 true。
pub fn rows_to_spans(out: &QueryResult) -> (Vec<Span>, bool) {
    let col_idx = |name: &str| out.columns.iter().position(|c| c == name);
    let span_id = col_idx("span_id");
    let parent = col_idx("parent_span_id");
    // 标准 OTEL：operation 是 `name`，service 是带点的 resource 属性 `service.name`。
    let svc = col_idx("service.name");
    let op = col_idx("name");
    let start = col_idx("start_time_unix_nano");
    let end = col_idx("end_time_unix_nano");
    let duration = col_idx("duration_ns");
    let kind = col_idx("kind");
    let status = col_idx("status_code");
    let status_message = col_idx("status_message");
    let trace_flags = col_idx("trace_flags");
    let trace_state = col_idx("trace_state");
    let resource_col = col_idx("resource");
    let scope_col = col_idx("scope");
    let attributes_col = col_idx("attributes");
    let events_col = col_idx("events");
    let links_col = col_idx("links");
    let dropped_attributes = col_idx("dropped_attributes_count");
    let dropped_events = col_idx("dropped_events_count");
    let dropped_links = col_idx("dropped_links_count");
    let schema_version = col_idx("schema_version");
    let semconv_version = col_idx("semantic_conventions_version");
    let sampling_reason = col_idx("sampling_reason");
    let partial = col_idx("partial");
    let partial_reasons = col_idx("partial_reasons");
    let late = col_idx("late");
    let duplicate = col_idx("duplicate");
    let conflict = col_idx("conflict");
    // Canonical rows contain nested `attributes`; older/public streams may only have
    // flattened attribute columns. Merge both without duplicating canonical metadata.
    let attr_cols: Vec<(usize, &str)> = out
        .columns
        .iter()
        .enumerate()
        .filter(|(_, name)| !is_core_span_column(name))
        .map(|(i, name)| (i, name.as_str()))
        .collect();

    let total = out.rows.len();
    let mut spans = Vec::with_capacity(total.min(SPAN_LIMIT));
    let mut seen_span_ids = HashSet::new();
    let mut byte_total = 0usize;
    let mut truncated = total > SPAN_LIMIT;

    for r in out.rows.iter().take(SPAN_LIMIT) {
        let pick = |idx: Option<usize>| -> Value {
            idx.and_then(|i| r.get(i).cloned()).unwrap_or(Value::Null)
        };
        let pick_str =
            |idx: Option<usize>| -> String { pick(idx).as_str().unwrap_or_default().to_string() };
        let pick_i64 = |idx: Option<usize>| -> i64 { value_i64(&pick(idx)) };
        let pick_u64 = |idx: Option<usize>| -> u64 { value_u64(&pick(idx)) };
        let pick_bool = |idx: Option<usize>| -> bool { pick(idx).as_bool().unwrap_or_default() };

        let span_id_value = pick_str(span_id);
        if span_id_value.is_empty() || !seen_span_ids.insert(span_id_value.clone()) {
            continue;
        }

        let attributes = {
            let mut map = parse_json::<serde_json::Map<String, Value>>(pick(attributes_col))
                .unwrap_or_default();
            for (i, name) in &attr_cols {
                match r.get(*i) {
                    Some(v) if !v.is_null() => {
                        map.insert((*name).to_string(), v.clone());
                    }
                    _ => {}
                }
            }
            Value::Object(map)
        };

        let span = Span {
            span_id: span_id_value,
            parent_span_id: {
                let v = pick(parent);
                v.as_str().filter(|s| !s.is_empty()).map(String::from)
            },
            service: pick_str(svc),
            operation: pick_str(op),
            start_ns: pick_i64(start),
            end_ns: pick_i64(end),
            duration_ns: pick_u64(duration).max(pick_u64(end).saturating_sub(pick_u64(start))),
            kind: pick_i64(kind) as i32,
            status: pick(status).as_str().unwrap_or("UNSET").to_string(),
            status_message: pick(status_message).as_str().map(str::to_owned),
            trace_flags: pick_u64(trace_flags) as u32,
            trace_state: pick_str(trace_state),
            resource: parse_json(pick(resource_col)).unwrap_or_default(),
            scope: parse_json(pick(scope_col)).unwrap_or_default(),
            attributes,
            events: parse_events(pick(events_col)),
            links: parse_json(pick(links_col)).unwrap_or_default(),
            dropped_attributes_count: pick_u64(dropped_attributes) as u32,
            dropped_events_count: pick_u64(dropped_events) as u32,
            dropped_links_count: pick_u64(dropped_links) as u32,
            schema_version: match pick_u64(schema_version) {
                0 => CANONICAL_SPAN_SCHEMA_VERSION,
                value => value.try_into().unwrap_or(CANONICAL_SPAN_SCHEMA_VERSION),
            },
            semantic_conventions_version: {
                let value = pick_str(semconv_version);
                if value.is_empty() {
                    SEMCONV_VERSION.into()
                } else {
                    value
                }
            },
            sampling_reason: parse_json(pick(sampling_reason)).unwrap_or_default(),
            partial: pick_bool(partial),
            partial_reasons: parse_json(pick(partial_reasons)).unwrap_or_default(),
            late: pick_bool(late),
            duplicate: pick_bool(duplicate),
            conflict: pick_bool(conflict),
        };
        // 粗估序列化大小，避免对每条 span 调一次 serde_json::to_string（昂贵）
        byte_total += span.span_id.len()
            + span.operation.len()
            + span.service.len()
            + serde_json::to_string(&span.attributes)
                .map(|s| s.len())
                .unwrap_or(0)
            + span.events.len() * 64
            + span.links.len() * 96
            + 128;
        if byte_total > RESPONSE_HARD_CAP {
            truncated = true;
            break;
        }
        spans.push(span);
    }
    (spans, truncated)
}

/// 已经映射到 `Span` 专属字段（或属于查询时间戳 / events）的列，不再重复进 attributes。
/// 其余列（`kind`、`duration_ns`、`status_message`、`http.*`、`service.version`…）作为 Tags。
fn is_core_span_column(name: &str) -> bool {
    matches!(
        name,
        "_timestamp"
            | "trace_id"
            | "span_id"
            | "parent_span_id"
            | "name"
            | "service.name"
            | "start_time_unix_nano"
            | "end_time_unix_nano"
            | "duration_ns"
            | "kind"
            | "status_code"
            | "status_message"
            | "trace_flags"
            | "trace_state"
            | "resource"
            | "scope"
            | "attributes"
            | "events"
            | "links"
            | "dropped_attributes_count"
            | "dropped_events_count"
            | "dropped_links_count"
            | "schema_version"
            | "semantic_conventions_version"
            | "sampling_reason"
            | "partial"
            | "partial_reasons"
            | "late"
            | "duplicate"
            | "conflict"
    )
}

fn parse_events(v: Value) -> Vec<SpanEvent> {
    let v = decode_json(v);
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|e| {
            let obj = e.as_object()?;
            Some(SpanEvent {
                ts_ns: obj
                    .get("time_unix_nano")
                    .or_else(|| obj.get("ts_ns"))
                    .map(value_i64)
                    .unwrap_or_default(),
                name: obj
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                attributes: obj.get("attributes").cloned().unwrap_or(Value::Null),
                dropped_attributes_count: obj
                    .get("dropped_attributes_count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default() as u32,
            })
        })
        .collect()
}

fn decode_json(value: Value) -> Value {
    match value {
        Value::String(raw) => serde_json::from_str(&raw).unwrap_or(Value::String(raw)),
        value => value,
    }
}

fn parse_json<T: DeserializeOwned>(value: Value) -> Option<T> {
    serde_json::from_value(decode_json(value)).ok()
}

fn value_i64(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .unwrap_or_default()
}

fn value_u64(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mk_result(n: usize) -> QueryResult {
        // 标准 OTEL 列：`SELECT *` 出来的全列，含核心字段 + 扁平属性（`http.method`）。
        let cols = vec![
            "_timestamp",
            "span_id",
            "parent_span_id",
            "service.name",
            "name",
            "start_time_unix_nano",
            "end_time_unix_nano",
            "status_code",
            "http.method",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let rows = (0..n)
            .map(|i| {
                vec![
                    json!(i as i64),
                    json!(format!("span{i}")),
                    json!(if i == 0 { "" } else { "span0" }),
                    json!("svc"),
                    json!("op"),
                    json!(i as i64 * 1_000),
                    json!(i as i64 * 1_000 + 500),
                    json!("OK"),
                    json!("GET"),
                ]
            })
            .collect();
        QueryResult {
            columns: cols,
            rows,
            scanned_rows: n as u64,
            took_ms: 1,
            federation: None,
        }
    }

    #[test]
    fn rows_to_spans_basic() {
        let (spans, truncated) = rows_to_spans(&mk_result(3));
        assert_eq!(spans.len(), 3);
        assert!(!truncated);
        assert_eq!(spans[0].parent_span_id, None);
        assert_eq!(spans[1].parent_span_id.as_deref(), Some("span0"));
        // canonical 列名映射到 Span 字段。
        assert_eq!(spans[0].service, "svc");
        assert_eq!(spans[0].operation, "op");
        assert_eq!(spans[0].status, "OK");
        // 非核心的扁平列聚回 attributes；核心列不重复进去。
        assert_eq!(spans[0].attributes["http.method"], json!("GET"));
        assert!(spans[0].attributes.get("service.name").is_none());
        assert!(spans[0].attributes.get("_timestamp").is_none());
    }

    #[test]
    fn rows_to_spans_dedupes_duplicate_span_ids() {
        let mut result = mk_result(2);
        result.rows.push(result.rows[0].clone());
        let (spans, truncated) = rows_to_spans(&result);
        assert_eq!(spans.len(), 2);
        assert!(!truncated);
        assert_eq!(spans.iter().filter(|s| s.span_id == "span0").count(), 1);
    }

    #[test]
    fn canonical_nested_fields_and_trace_diagnostics_round_trip() {
        let mut result = mk_result(1);
        let canonical = [
            (
                "resource",
                json!({
                    "attributes": {"service.name": "svc", "service.version": "1"},
                    "dropped_attributes_count": 1,
                    "schema_url": "https://resource"
                }),
            ),
            (
                "scope",
                json!({
                    "name": "fixture.scope",
                    "version": "2",
                    "attributes": {"scope.key": "scope.value"},
                    "dropped_attributes_count": 2,
                    "schema_url": "https://scope"
                }),
            ),
            ("attributes", json!({"db.system": "postgresql"})),
            (
                "events",
                json!([{
                    "time_unix_nano": 1234,
                    "name": "exception",
                    "attributes": {"exception.type": "fixture"},
                    "dropped_attributes_count": 3
                }]),
            ),
            (
                "links",
                json!([{
                    "trace_id": "11111111111111111111111111111111",
                    "span_id": "2222222222222222",
                    "trace_state": "vendor=value",
                    "flags": 1,
                    "attributes": {"link.kind": "retry"},
                    "dropped_attributes_count": 4
                }]),
            ),
            ("dropped_attributes_count", json!(5)),
            ("dropped_events_count", json!(6)),
            ("dropped_links_count", json!(7)),
            ("schema_version", json!(1)),
            ("semantic_conventions_version", json!("1.43.0")),
            ("sampling_reason", json!("error")),
            ("partial", json!(true)),
            ("partial_reasons", json!(["late_span"])),
            ("late", json!(true)),
            ("duplicate", json!(false)),
            ("conflict", json!(true)),
        ];
        for (name, value) in canonical {
            result.columns.push(name.into());
            result.rows[0].push(value);
        }

        let (spans, truncated) = rows_to_spans(&result);
        assert!(!truncated);
        let span = &spans[0];
        assert_eq!(span.scope.name, "fixture.scope");
        assert_eq!(span.events[0].ts_ns, 1234);
        assert_eq!(span.events[0].dropped_attributes_count, 3);
        assert_eq!(span.links[0].trace_state, "vendor=value");
        assert_eq!(span.dropped_links_count, 7);
        assert_eq!(span.sampling_reason, SamplingReason::Error);
        assert_eq!(span.partial_reasons, vec![PartialReason::LateSpan]);
        assert!(span.late);
        assert!(span.conflict);
        assert_eq!(span.attributes["db.system"], json!("postgresql"));

        let response = TraceResponse::new("trace".into(), spans, false);
        assert!(response.partial);
        assert_eq!(response.late_span_count, 1);
        assert_eq!(response.conflict_span_count, 1);
        assert_eq!(response.sampling_reasons, vec![SamplingReason::Error]);
    }

    #[test]
    fn rows_to_spans_overflows_to_truncated() {
        let (spans, truncated) = rows_to_spans(&mk_result(SPAN_LIMIT + 5));
        assert_eq!(spans.len(), SPAN_LIMIT);
        assert!(truncated);
    }
}
