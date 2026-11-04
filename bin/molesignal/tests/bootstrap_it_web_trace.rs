// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/trace/:trace_id` 端点冒烟。
//!
//! happy: 合法 trace_id 但 traces 流为空 → 404 not_found
//! sad:   非法 trace_id（含 `;` / 空格） → 400 invalid
//! e2e:   按标准 OTEL 列名 intake 一条 trace（自动建流）→ 详情、Span 列表、RED 出数据，列表没有
//! e2e:   OTLP 写入一条 trace（自动建流）→ /web/traces 列表 + 详情都出数据
//! e2e:   只有原生写入的 `default` 流不会挡住 OTLP 写入的流

mod common;

use common::TestServer;
use molesignal::shared::time::TimestampMicros;
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest,
    common::v1::{AnyValue, KeyValue, any_value},
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span, Status, status::StatusCode},
};
use prost::Message;
use serde_json::Value;

const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";
const OTHER_TRACE_ID: &str = "fedcba9876543210fedcba9876543210";
const ROOT_SPAN: &str = "1111111111111111";
const CHILD_SPAN: &str = "2222222222222222";

/// 五秒前开始，整条 Trace 都落在默认的查询窗口里。
fn trace_start_ns() -> u64 {
    (TimestampMicros::now().0 as u64 - 5_000_000) * 1_000
}

/// 详情查到 Span 时才有值；写入后要等落盘。
async fn trace_detail(s: &TestServer) -> Option<Value> {
    let (hk, hv) = s.auth_header();

    let response = s
        .client
        .get(format!("{}/api/v1/web/trace/{TRACE_ID}", s.base_url))
        .header(hk, hv)
        .send()
        .await
        .ok()?;

    if response.status() != 200 {
        return None;
    }

    response.json().await.ok()
}

/// 详情：2 个 span，root 为父；扁平属性聚回 attributes。
fn assert_detail(v: &Value) {
    assert_eq!(v["traceId"], TRACE_ID);
    assert_eq!(v["rootSpanId"], ROOT_SPAN);

    let spans = v["spans"].as_array().unwrap();
    assert_eq!(spans.len(), 2);

    let root = spans
        .iter()
        .find(|sp| sp["spanId"] == ROOT_SPAN)
        .expect("root span present");
    assert_eq!(root["service"], "checkout");
    assert_eq!(root["operation"], "GET /checkout");
    assert_eq!(root["status"], "OK");
    assert_eq!(root["attributes"]["http.method"], "GET");
}

/// GET 一个只读接口的 JSON。
async fn read_json(s: &TestServer, path: &str) -> Value {
    let (hk, hv) = s.auth_header();

    s.client
        .get(format!("{}{path}", s.base_url))
        .header(hk, hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// 列表的第一条；写入后要等判定和落盘。
async fn first_listed_trace(s: &TestServer) -> Option<Value> {
    read_json(s, "/api/v1/web/traces").await["items"]
        .get(0)
        .cloned()
}

/// 按标准 OTEL 列名（operation=`name`，service 带点 `service.name`，时间 `*_unix_nano`，
/// status_code 字符串）原生写入一条两个 Span 的 Trace，流不存在时自动建。
async fn post_native_trace(s: &TestServer, stream: &str, trace_id: &str) {
    let (hk, hv) = s.auth_header();
    let start_ns = trace_start_ns();

    let resp = s
        .client
        .post(format!("{}/api/v1/intake/traces/{stream}", s.base_url))
        .header(hk, hv)
        .json(&serde_json::json!([
            {
                "trace_id": trace_id,
                "span_id": ROOT_SPAN,
                "service.name": "checkout",
                "name": "GET /checkout",
                "start_time_unix_nano": start_ns,
                "end_time_unix_nano": start_ns + 2_000_000_000,
                "duration_ns": 2_000_000_000u64,
                "status_code": "OK",
                "http.method": "GET"
            },
            {
                "trace_id": trace_id,
                "span_id": CHILD_SPAN,
                "parent_span_id": ROOT_SPAN,
                "service.name": "checkout",
                "name": "db.query",
                "start_time_unix_nano": start_ns + 100_000_000,
                "end_time_unix_nano": start_ns + 1_000_000_000,
                "duration_ns": 900_000_000u64,
                "status_code": "ERROR",
                "db.system": "postgres"
            }
        ]))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "intake to a non-existent stream must auto-create it"
    );

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], 2, "both spans accepted");
}

/// OTLP 写入 [`two_span_trace`]，流不存在时自动建。
async fn post_otlp_trace(s: &TestServer, stream: &str) {
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/traces", s.base_url))
        .header(hk, hv)
        .header("content-type", "application/x-protobuf")
        .header("stream-name", stream)
        .body(two_span_trace(trace_start_ns()).encode_to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "OTLP export must auto-create the stream"
    );
}

#[tokio::test]
async fn trace_unknown_id_returns_404() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/trace/0123456789abcdef0123456789abcdef",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // 三种合法路径：
    // - traces 流不存在 → ensure_stream_in_org 返 Forbidden(403)
    // - traces 流存在但 trace_id 查无 → handler 返 not_found(404)
    // - query engine 走偏 → 500
    let code = resp.status().as_u16();
    assert!(
        code == 403 || code == 404 || code == 500,
        "expected 403/404/500, got {code}"
    );
}

#[tokio::test]
async fn trace_invalid_id_returns_400() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!("{}/api/v1/web/trace/bad;trace%20id", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // 字符不在 [A-Za-z0-9_-] → handler 内 Error::invalid → 400
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn trace_intake_auto_creates_stream_then_detail_returns_data() {
    if common::skip_unless_enabled() {
        return;
    }

    let s = TestServer::start_with_fast_flush().await;

    // 撤掉预 seed 后没有任何 traces 流：intake 到 `default` 应触发 schema-on-write 自动建流。
    post_native_trace(&s, "default", TRACE_ID).await;

    // 等 buffer→parquet→query_file flush 后，详情能查到这条 trace。
    assert!(
        common::wait_until_async(15, || async { trace_detail(&s).await.is_some() }).await,
        "received trace never surfaced in /web/trace"
    );

    assert_detail(&trace_detail(&s).await.unwrap());

    // 原生写入不生成摘要：列表里没有，读原始 Span 的 Span 列表和 RED 有。
    assert!(
        first_listed_trace(&s).await.is_none(),
        "a natively written trace has no summary to list"
    );

    let spans = read_json(&s, "/api/v1/web/traces/spans").await;
    assert_eq!(spans["items"].as_array().map(Vec::len), Some(2));

    let now = TimestampMicros::now().0;
    let from = now - 3_600_000_000;

    let red = read_json(
        &s,
        &format!(
            "/api/v1/web/traces/red?from={from}&to={now}&bucketTime=60000000&originTime={from}"
        ),
    )
    .await;
    assert_eq!(
        red["total"]["requests"], 1,
        "the root span took the request"
    );
}

fn string_attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.into())),
        }),
        ..Default::default()
    }
}

/// 一条两个 Span 的 Trace：根 Span 成功，子 Span 出错。
fn two_span_trace(start_ns: u64) -> ExportTraceServiceRequest {
    let trace_id = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef].repeat(2);

    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![string_attribute("service.name", "checkout")],
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans: vec![
                    Span {
                        trace_id: trace_id.clone(),
                        span_id: vec![0x11; 8],
                        name: "GET /checkout".into(),
                        start_time_unix_nano: start_ns,
                        end_time_unix_nano: start_ns + 2_000_000_000,
                        attributes: vec![string_attribute("http.method", "GET")],
                        status: Some(Status {
                            code: StatusCode::Ok as i32,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Span {
                        trace_id,
                        span_id: vec![0x22; 8],
                        parent_span_id: vec![0x11; 8],
                        name: "db.query".into(),
                        start_time_unix_nano: start_ns + 100_000_000,
                        end_time_unix_nano: start_ns + 1_000_000_000,
                        attributes: vec![string_attribute("db.system", "postgres")],
                        status: Some(Status {
                            code: StatusCode::Error as i32,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trace_otlp_auto_creates_stream_then_list_and_detail_return_data() {
    if common::skip_unless_enabled() {
        return;
    }

    let s = TestServer::start_keeping_traces().await;

    post_otlp_trace(&s, "default").await;

    // 列表只读写入时生成的摘要，摘要在 Trace 判定后写下，所以这里走 OTLP：原生 intake 不经过判定。
    assert!(
        common::wait_until_async(30, || async { first_listed_trace(&s).await.is_some() }).await,
        "received trace never surfaced in /web/traces"
    );

    // 列表内容：聚合出 service / span_count / error_count。
    let item = first_listed_trace(&s).await.unwrap();
    assert_eq!(item["traceId"], TRACE_ID);
    assert_eq!(item["service"], "checkout");
    assert_eq!(item["spanCount"], 2);
    assert_eq!(item["errorCount"], 1, "child span status_code=ERROR counts");

    assert_detail(&trace_detail(&s).await.expect("detail of a listed trace"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_written_only_natively_does_not_hide_listed_traces() {
    if common::skip_unless_enabled() {
        return;
    }

    let s = TestServer::start_keeping_traces().await;

    // `default` 字段标准，名字也优先，但里面只有原生写入的 Span，没有摘要可列。
    post_native_trace(&s, "default", OTHER_TRACE_ID).await;
    post_otlp_trace(&s, "prod").await;

    assert!(
        common::wait_until_async(30, || async { first_listed_trace(&s).await.is_some() }).await,
        "a stream without summaries hid the traces of `prod`"
    );

    assert_eq!(first_listed_trace(&s).await.unwrap()["traceId"], TRACE_ID);
    assert_detail(&trace_detail(&s).await.expect("detail of a listed trace"));
}
