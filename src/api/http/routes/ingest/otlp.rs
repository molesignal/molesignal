// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OTLP/HTTP receiver：`POST /api/v1/{logs,metrics,traces}`。
//!
//! 接收**标准 OTLP**：`application/x-protobuf`（OTel SDK / Collector 默认）或
//! `application/json`（OTLP/JSON 编码）。把 `ResourceLogs/ResourceMetrics/ResourceSpans`
//! 扁平化为内部 [`RawEvent`]（resource + scope + record 属性 flatten 进 `fields`，
//! 时间戳 ns→micros），再走 ingestion。
//!
//! stream 名：OTLP 本身无 stream 概念 —— 取请求头 `stream-name`，缺省回退到
//! `default`（默认约定；不存在时由 ingestion 按需建流）。

use std::collections::BTreeMap;

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::post,
};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
        metrics::v1::{ExportMetricsServiceRequest, ExportMetricsServiceResponse},
        trace::v1::{ExportTraceServiceRequest, ExportTraceServiceResponse},
    },
    common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value},
    metrics::v1::{Metric, metric, number_data_point},
    resource::v1::Resource,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    api::AppState,
    domain::{
        ingestion::{IngestBatch, RawEvent},
        metrics::METRIC_NAME_FIELD,
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::TimestampMicros,
        trace_normalization::{
            CanonicalEvent, CanonicalLink, CanonicalResource, CanonicalScope, CanonicalSpan,
            TraceLimits, sanitize_and_limit_span,
        },
    },
};

/// 无 `stream-name` 头时 OTLP 各信号类型落入的默认流名（默认约定）；
/// 与前端 explore / send-test-event 的 `default` 流对齐，不存在时 ingestion 自动建流。
const DEFAULT_STREAM: &str = "default";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/logs", post(otlp_logs))
        .route("/metrics", post(otlp_metrics))
        .route("/traces", post(otlp_traces))
}

// ===== content negotiation =====

#[derive(Clone, Copy)]
pub(crate) enum OtlpEncoding {
    Protobuf,
    Json,
}

/// OTLP/HTTP：默认 protobuf；`application/json` 为 OTLP/JSON。
pub(crate) fn detect_encoding(headers: &HeaderMap) -> Result<OtlpEncoding> {
    let ct = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ct.contains("application/json") {
        Ok(OtlpEncoding::Json)
    } else if ct.is_empty() || ct.contains("protobuf") || ct.contains("application/octet-stream") {
        Ok(OtlpEncoding::Protobuf)
    } else {
        Err(Error::invalid(format!(
            "unsupported OTLP content-type: {ct}"
        )))
    }
}

pub(crate) fn decode_otlp<T>(enc: OtlpEncoding, body: &Bytes) -> Result<T>
where
    T: prost::Message + Default + serde::de::DeserializeOwned,
{
    match enc {
        OtlpEncoding::Protobuf => T::decode(body.as_ref())
            .map_err(|e| Error::invalid(format!("OTLP protobuf decode: {e}"))),
        OtlpEncoding::Json => serde_json::from_slice(body.as_ref())
            .map_err(|e| Error::invalid(format!("OTLP/JSON decode: {e}"))),
    }
}

fn stream_name(headers: &HeaderMap, default: &str) -> String {
    headers
        .get("stream-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// OTLP/HTTP 成功响应：200 + 对应编码的（空）Export*ServiceResponse。
fn otlp_ok<T>(enc: OtlpEncoding, resp: T) -> Response
where
    T: prost::Message + Serialize,
{
    match enc {
        OtlpEncoding::Protobuf => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/x-protobuf")],
            resp.encode_to_vec(),
        )
            .into_response(),
        OtlpEncoding::Json => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec()),
        )
            .into_response(),
    }
}

// ===== OTLP → RawEvent 转换 =====

fn nanos_to_micros(ns: u64) -> TimestampMicros {
    if ns == 0 {
        TimestampMicros::now()
    } else {
        TimestampMicros((ns / 1_000) as i64)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub(crate) fn any_value_to_json(v: &AnyValue) -> Value {
    match &v.value {
        None => Value::Null,
        Some(any_value::Value::StringValue(s)) => Value::String(s.clone()),
        Some(any_value::Value::BoolValue(b)) => Value::Bool(*b),
        Some(any_value::Value::IntValue(i)) => Value::from(*i),
        Some(any_value::Value::DoubleValue(d)) => Value::from(*d),
        Some(any_value::Value::ArrayValue(arr)) => {
            Value::Array(arr.values.iter().map(any_value_to_json).collect())
        }
        Some(any_value::Value::KvlistValue(kv)) => {
            let mut m = Map::new();
            for k in &kv.values {
                let val = k
                    .value
                    .as_ref()
                    .map(any_value_to_json)
                    .unwrap_or(Value::Null);
                m.insert(k.key.clone(), val);
            }
            Value::Object(m)
        }
        Some(any_value::Value::BytesValue(b)) => Value::String(hex(b)),
        // 未解析的字符串驻留索引 / 未来新增变体 → Null（无字符串表上下文）。
        Some(_) => Value::Null,
    }
}

fn put_attrs(map: &mut Map<String, Value>, attrs: &[KeyValue]) {
    for kv in attrs {
        let v = kv
            .value
            .as_ref()
            .map(any_value_to_json)
            .unwrap_or(Value::Null);
        map.insert(kv.key.clone(), v);
    }
}

fn attrs_to_btree(attrs: &[KeyValue]) -> BTreeMap<String, Value> {
    attrs
        .iter()
        .map(|kv| {
            (
                kv.key.clone(),
                kv.value
                    .as_ref()
                    .map(any_value_to_json)
                    .unwrap_or(Value::Null),
            )
        })
        .collect()
}

/// resource + scope 公共字段（每条记录复制一份做基底）。
fn base_fields(
    resource: Option<&Resource>,
    scope: Option<&InstrumentationScope>,
) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(r) = resource {
        put_attrs(&mut m, &r.attributes);
    }
    if let Some(s) = scope {
        if !s.name.is_empty() {
            m.insert("scope_name".into(), Value::String(s.name.clone()));
        }
        if !s.version.is_empty() {
            m.insert("scope_version".into(), Value::String(s.version.clone()));
        }
        put_attrs(&mut m, &s.attributes);
    }
    m
}

pub(crate) fn logs_to_events(req: ExportLogsServiceRequest) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for rl in &req.resource_logs {
        for sl in &rl.scope_logs {
            let base = base_fields(rl.resource.as_ref(), sl.scope.as_ref());
            for lr in &sl.log_records {
                let mut f = base.clone();
                if lr.severity_number != 0 {
                    f.insert("severity_number".into(), Value::from(lr.severity_number));
                }
                if !lr.severity_text.is_empty() {
                    f.insert(
                        "severity_text".into(),
                        Value::String(lr.severity_text.clone()),
                    );
                }
                if let Some(body) = &lr.body {
                    f.insert("body".into(), any_value_to_json(body));
                }
                if !lr.trace_id.is_empty() {
                    f.insert("trace_id".into(), Value::String(hex(&lr.trace_id)));
                }
                if !lr.span_id.is_empty() {
                    f.insert("span_id".into(), Value::String(hex(&lr.span_id)));
                }
                put_attrs(&mut f, &lr.attributes);
                let ts = if lr.time_unix_nano != 0 {
                    lr.time_unix_nano
                } else {
                    lr.observed_time_unix_nano
                };
                out.push(RawEvent {
                    timestamp: nanos_to_micros(ts),
                    fields: f,
                });
            }
        }
    }
    out
}

/// OTLP `Status.code`（i32 enum）→ canonical 字符串标签。
fn status_code_label(code: i32) -> String {
    match code {
        1 => "OK",
        2 => "ERROR",
        // STATUS_CODE_UNSET=0，未知值同样归 UNSET。
        _ => "UNSET",
    }
    .to_string()
}

pub(crate) fn traces_to_canonical(req: ExportTraceServiceRequest) -> Vec<CanonicalSpan> {
    let mut out = Vec::new();
    for rs in &req.resource_spans {
        for ss in &rs.scope_spans {
            for sp in &ss.spans {
                let status = sp.status.as_ref();
                let mut canonical = CanonicalSpan::new(
                    hex(&sp.trace_id),
                    hex(&sp.span_id),
                    sp.name.clone(),
                    sp.kind,
                    sp.start_time_unix_nano,
                    sp.end_time_unix_nano,
                );
                canonical.parent_span_id =
                    (!sp.parent_span_id.is_empty()).then(|| hex(&sp.parent_span_id));
                canonical.trace_flags = sp.flags;
                canonical.trace_state = sp.trace_state.clone();
                canonical.status_code =
                    status_code_label(status.map(|value| value.code).unwrap_or(0));
                canonical.status_message = status
                    .map(|value| value.message.clone())
                    .filter(|message| !message.is_empty());
                canonical.resource = CanonicalResource {
                    attributes: rs
                        .resource
                        .as_ref()
                        .map(|resource| attrs_to_btree(&resource.attributes))
                        .unwrap_or_default(),
                    dropped_attributes_count: rs
                        .resource
                        .as_ref()
                        .map(|resource| resource.dropped_attributes_count)
                        .unwrap_or_default(),
                    schema_url: (!rs.schema_url.is_empty()).then(|| rs.schema_url.clone()),
                };
                canonical.scope = CanonicalScope {
                    name: ss
                        .scope
                        .as_ref()
                        .map(|scope| scope.name.clone())
                        .unwrap_or_default(),
                    version: ss
                        .scope
                        .as_ref()
                        .map(|scope| scope.version.clone())
                        .unwrap_or_default(),
                    attributes: ss
                        .scope
                        .as_ref()
                        .map(|scope| attrs_to_btree(&scope.attributes))
                        .unwrap_or_default(),
                    dropped_attributes_count: ss
                        .scope
                        .as_ref()
                        .map(|scope| scope.dropped_attributes_count)
                        .unwrap_or_default(),
                    schema_url: (!ss.schema_url.is_empty()).then(|| ss.schema_url.clone()),
                };
                canonical.attributes = attrs_to_btree(&sp.attributes);
                canonical.events = sp
                    .events
                    .iter()
                    .map(|event| CanonicalEvent {
                        time_unix_nano: event.time_unix_nano,
                        name: event.name.clone(),
                        attributes: attrs_to_btree(&event.attributes),
                        dropped_attributes_count: event.dropped_attributes_count,
                    })
                    .collect();
                canonical.links = sp
                    .links
                    .iter()
                    .map(|link| CanonicalLink {
                        trace_id: hex(&link.trace_id),
                        span_id: hex(&link.span_id),
                        trace_state: link.trace_state.clone(),
                        flags: link.flags,
                        attributes: attrs_to_btree(&link.attributes),
                        dropped_attributes_count: link.dropped_attributes_count,
                    })
                    .collect();
                canonical.dropped_attributes_count = sp.dropped_attributes_count;
                canonical.dropped_events_count = sp.dropped_events_count;
                canonical.dropped_links_count = sp.dropped_links_count;
                sanitize_and_limit_span(&mut canonical, TraceLimits::default());
                out.push(canonical);
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn traces_to_events(req: ExportTraceServiceRequest) -> Vec<RawEvent> {
    traces_to_canonical(req)
        .into_iter()
        .map(CanonicalSpan::into_raw_event)
        .collect()
}

fn metric_meta(f: &mut Map<String, Value>, m: &Metric) {
    f.insert(METRIC_NAME_FIELD.into(), Value::String(m.name.clone()));
    if !m.unit.is_empty() {
        f.insert("metric_unit".into(), Value::String(m.unit.clone()));
    }
}

fn put_number(f: &mut Map<String, Value>, v: &Option<number_data_point::Value>) {
    match v {
        Some(number_data_point::Value::AsDouble(d)) => {
            f.insert("value".into(), Value::from(*d));
        }
        Some(number_data_point::Value::AsInt(i)) => {
            f.insert("value".into(), Value::from(*i));
        }
        None => {}
    }
}

pub(crate) fn metrics_to_events(req: ExportMetricsServiceRequest) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for rm in &req.resource_metrics {
        for sm in &rm.scope_metrics {
            let base = base_fields(rm.resource.as_ref(), sm.scope.as_ref());
            for m in &sm.metrics {
                let push = |out: &mut Vec<RawEvent>,
                            attrs: &[KeyValue],
                            ts: u64,
                            fill: &dyn Fn(&mut Map<String, Value>)| {
                    let mut f = base.clone();
                    metric_meta(&mut f, m);
                    put_attrs(&mut f, attrs);
                    fill(&mut f);
                    out.push(RawEvent {
                        timestamp: nanos_to_micros(ts),
                        fields: f,
                    });
                };
                match &m.data {
                    Some(metric::Data::Gauge(g)) => {
                        for dp in &g.data_points {
                            push(&mut out, &dp.attributes, dp.time_unix_nano, &|f| {
                                put_number(f, &dp.value)
                            });
                        }
                    }
                    Some(metric::Data::Sum(s)) => {
                        for dp in &s.data_points {
                            push(&mut out, &dp.attributes, dp.time_unix_nano, &|f| {
                                put_number(f, &dp.value)
                            });
                        }
                    }
                    Some(metric::Data::Histogram(h)) => {
                        for dp in &h.data_points {
                            push(&mut out, &dp.attributes, dp.time_unix_nano, &|f| {
                                f.insert("count".into(), Value::from(dp.count));
                                if let Some(sum) = dp.sum {
                                    f.insert("sum".into(), Value::from(sum));
                                }
                                if let Some(min) = dp.min {
                                    f.insert("min".into(), Value::from(min));
                                }
                                if let Some(max) = dp.max {
                                    f.insert("max".into(), Value::from(max));
                                }
                            });
                        }
                    }
                    Some(metric::Data::ExponentialHistogram(h)) => {
                        for dp in &h.data_points {
                            push(&mut out, &dp.attributes, dp.time_unix_nano, &|f| {
                                f.insert("count".into(), Value::from(dp.count));
                                if let Some(sum) = dp.sum {
                                    f.insert("sum".into(), Value::from(sum));
                                }
                            });
                        }
                    }
                    Some(metric::Data::Summary(s)) => {
                        for dp in &s.data_points {
                            push(&mut out, &dp.attributes, dp.time_unix_nano, &|f| {
                                f.insert("count".into(), Value::from(dp.count));
                                f.insert("sum".into(), Value::from(dp.sum));
                            });
                        }
                    }
                    None => {}
                }
            }
        }
    }
    out
}

// ===== ingest =====

pub(crate) async fn ingest(
    state: &AppState,
    stream_type: StreamType,
    org_id: Id,
    stream: String,
    events: Vec<RawEvent>,
    bytes: usize,
) -> Result<usize> {
    // 计费门禁 + 计量：原始字节数用于配额/计量；license 过期 / 订阅停服 / 超 cap → 402。
    crate::api::http::billing::ensure_ingest_allowed(
        state,
        &org_id,
        bytes as u64,
        TimestampMicros::now().0,
    )
    .await?;
    if events.is_empty() {
        return Ok(0);
    }
    let batch = IngestBatch {
        batch_id: Id::new(),
        org_id,
        stream,
        stream_type,
        events,
        received_at: TimestampMicros::now(),
    };
    let r = state.ingestion.ingest(batch).await?;
    Ok(r.accepted)
}

// ===== handlers =====

async fn otlp_logs(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::app::iam::IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let enc = detect_encoding(&headers)?;
    let req: ExportLogsServiceRequest = decode_otlp(enc, &body)?;
    let stream = stream_name(&headers, DEFAULT_STREAM);
    let events = logs_to_events(req);
    ingest(
        &state,
        StreamType::Logs,
        ctx.org_id,
        stream,
        events,
        body.len(),
    )
    .await?;
    Ok(otlp_ok(enc, ExportLogsServiceResponse::default()))
}

async fn otlp_metrics(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::app::iam::IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let enc = detect_encoding(&headers)?;
    let req: ExportMetricsServiceRequest = decode_otlp(enc, &body)?;
    let stream = stream_name(&headers, DEFAULT_STREAM);
    let events = metrics_to_events(req);
    ingest(
        &state,
        StreamType::Metrics,
        ctx.org_id,
        stream,
        events,
        body.len(),
    )
    .await?;
    Ok(otlp_ok(enc, ExportMetricsServiceResponse::default()))
}

async fn otlp_traces(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::app::iam::IamContext>,
    axum::Extension(trace_context): axum::Extension<crate::shared::trace_context::TraceContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let enc = detect_encoding(&headers)?;
    let req: ExportTraceServiceRequest = decode_otlp(enc, &body)?;
    let stream = stream_name(&headers, DEFAULT_STREAM);
    let spans = traces_to_canonical(req);
    submit_traces(
        &state,
        ctx.org_id,
        stream,
        spans,
        body.len(),
        &trace_context,
        headers.contains_key(crate::app::trace::export::SELF_EXPORT_MARKER),
    )
    .await?;
    Ok(otlp_ok(enc, ExportTraceServiceResponse::default()))
}

pub(crate) async fn submit_traces(
    state: &AppState,
    org_id: Id,
    stream: String,
    mut spans: Vec<CanonicalSpan>,
    bytes: usize,
    trace_context: &crate::shared::trace_context::TraceContext,
    suppress_external: bool,
) -> Result<usize> {
    crate::api::http::billing::ensure_ingest_allowed(
        state,
        &org_id,
        bytes as u64,
        TimestampMicros::now().0,
    )
    .await?;
    let force_keep = match trace_context.trust {
        crate::shared::trace_context::TraceTrust::DebugToken if trace_context.force_keep => {
            crate::shared::tail_sampling::ForceKeep::DebugToken
        }
        crate::shared::trace_context::TraceTrust::Internal if trace_context.force_keep => {
            crate::shared::tail_sampling::ForceKeep::TrustedInternal
        }
        _ => crate::shared::tail_sampling::ForceKeep::None,
    };
    let count = spans.len();
    for span in &mut spans {
        if suppress_external {
            span.attributes.insert(
                "molesignal.trace.suppress_external".into(),
                Value::Bool(true),
            );
        }
    }
    for span in spans {
        // Queue exhaustion is an explicitly measured fail-open drop; OTLP producer success
        // must not backpressure the caller.
        let _ = state.telemetry.trace_candidates.try_submit(
            crate::shared::tail_sampling::TraceCandidate {
                org_id: org_id.0.clone(),
                stream: Some(stream.clone()),
                span,
                force_keep,
            },
        );
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use opentelemetry_proto::tonic::{
        logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
        metrics::v1::{Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric},
        trace::v1::{ResourceSpans, ScopeSpans, Span, Status, span::SpanKind, status::StatusCode},
    };

    use super::*;

    fn str_kv(k: &str, s: &str) -> KeyValue {
        KeyValue {
            key: k.into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(s.into())),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn logs_flatten_resource_scope_and_record() {
        let req = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(Resource {
                    attributes: vec![str_kv("service.name", "checkout")],
                    ..Default::default()
                }),
                scope_logs: vec![ScopeLogs {
                    scope: Some(InstrumentationScope {
                        name: "lib".into(),
                        ..Default::default()
                    }),
                    log_records: vec![LogRecord {
                        time_unix_nano: 2_000,
                        severity_text: "INFO".into(),
                        body: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("hello".into())),
                        }),
                        attributes: vec![str_kv("http.method", "GET")],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let events = logs_to_events(req);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.timestamp.0, 2, "2000ns -> 2us");
        assert_eq!(e.fields.get("service.name").unwrap(), "checkout");
        assert_eq!(e.fields.get("scope_name").unwrap(), "lib");
        assert_eq!(e.fields.get("severity_text").unwrap(), "INFO");
        assert_eq!(e.fields.get("body").unwrap(), "hello");
        assert_eq!(e.fields.get("http.method").unwrap(), "GET");
    }

    #[test]
    fn traces_flatten_uses_canonical_otel_columns() {
        let req = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(Resource {
                    attributes: vec![str_kv("service.name", "checkout")],
                    ..Default::default()
                }),
                scope_spans: vec![ScopeSpans {
                    scope: Some(InstrumentationScope {
                        name: "lib".into(),
                        ..Default::default()
                    }),
                    spans: vec![Span {
                        trace_id: vec![0x01; 16],
                        span_id: vec![0x02; 8],
                        parent_span_id: vec![0x03; 8],
                        name: "GET /checkout".into(),
                        kind: SpanKind::Server as i32,
                        start_time_unix_nano: 5_000,
                        end_time_unix_nano: 9_000,
                        status: Some(Status {
                            code: StatusCode::Error as i32,
                            message: "boom".into(),
                        }),
                        attributes: vec![str_kv("http.method", "GET")],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let events = traces_to_events(req);
        assert_eq!(events.len(), 1);
        let f = &events[0].fields;
        // OTEL 原生 / OTLP proto 字段名（operation 叫 `name`，service 带点原样保留）。
        assert_eq!(f.get("name").unwrap(), "GET /checkout");
        assert_eq!(f.get("service.name").unwrap(), "checkout");
        assert_eq!(f.get("trace_id").unwrap(), &Value::String("01".repeat(16)));
        assert_eq!(f.get("span_id").unwrap(), &Value::String("02".repeat(8)));
        assert_eq!(
            f.get("parent_span_id").unwrap(),
            &Value::String("03".repeat(8))
        );
        assert_eq!(
            f.get("start_time_unix_nano").unwrap(),
            &Value::from(5_000u64)
        );
        assert_eq!(f.get("end_time_unix_nano").unwrap(), &Value::from(9_000u64));
        assert_eq!(f.get("duration_ns").unwrap(), &Value::from(4_000u64));
        // status_code 落成可读字符串，配合查询侧 `status_code = 'ERROR'`。
        assert_eq!(f.get("status_code").unwrap(), "ERROR");
        assert_eq!(f.get("status_message").unwrap(), "boom");
        assert_eq!(f.get("http.method").unwrap(), "GET");
        // _timestamp = start ns→us。
        assert_eq!(events[0].timestamp.0, 5);
    }

    #[test]
    fn traces_emit_status_code_and_service_name_even_when_absent() {
        // span 不带 Status、resource 不带 service.name → 仍要写出这两列（否则按需
        // 建流推断的 schema 缺列，list 查询会整页 400）。
        let req = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![Span {
                        trace_id: vec![0x0a; 16],
                        span_id: vec![0x0b; 8],
                        name: "anon".into(),
                        start_time_unix_nano: 1_000,
                        end_time_unix_nano: 2_000,
                        status: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let events = traces_to_events(req);
        let f = &events[0].fields;
        assert_eq!(f.get("status_code").unwrap(), "UNSET");
        assert_eq!(f.get("service.name").unwrap(), "unknown_service");
        assert!(f.get("status_message").is_none());
    }

    #[test]
    fn metrics_gauge_emits_one_event_per_point() {
        let req = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                scope_metrics: vec![ScopeMetrics {
                    metrics: vec![Metric {
                        name: "cpu.usage".into(),
                        unit: "1".into(),
                        data: Some(metric::Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                time_unix_nano: 5_000,
                                attributes: vec![str_kv("host", "a")],
                                value: Some(number_data_point::Value::AsDouble(0.5)),
                                ..Default::default()
                            }],
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let events = metrics_to_events(req);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.timestamp.0, 5);
        assert_eq!(e.fields.get("metric_name").unwrap(), "cpu.usage");
        assert_eq!(e.fields.get("host").unwrap(), "a");
        assert_eq!(e.fields.get("value").unwrap(), 0.5);
    }
}
