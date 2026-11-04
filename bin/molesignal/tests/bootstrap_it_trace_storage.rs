// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 服务自遥测关着（`telemetry.self_collect.enabled = false`，配置不写这一段时就是这样）：租户发来的
//! 链路照常写入，进程自身的链路不写。
//!
//! 单独一个 binary：要捕获进程自身的链路，得装全局 self-telemetry subscriber。

mod common;

use common::{TestServer, skip_unless_enabled, wait_until, wait_until_async};
use molesignal::{
    domain::stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
    shared::time::TimestampMicros,
};
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest,
    common::v1::{AnyValue, KeyValue, any_value},
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span},
};
use prost::Message;
use serde_json::{Value, json};

/// 一条 Span 的 OTLP 请求。
fn one_span(started_at_micros: i64) -> ExportTraceServiceRequest {
    let start_time_unix_nano = started_at_micros as u64 * 1_000;

    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![KeyValue {
                    key: "service.name".into(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::StringValue("checkout".into())),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    trace_id: vec![0x11; 16],
                    span_id: vec![0x22; 8],
                    name: "GET /checkout".into(),
                    start_time_unix_nano,
                    end_time_unix_nano: start_time_unix_nano + 1_000_000,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenant_traces_are_stored_while_self_telemetry_is_off() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let server = TestServer::start_with_self_telemetry_off().await;
    let (header_name, header_value) = server.auth_header();
    let now = TimestampMicros::now().0;

    let response = server
        .client
        .post(format!("{}/api/v1/traces", server.base_url))
        .header(header_name, &header_value)
        .header("content-type", "application/x-protobuf")
        .header("stream-name", "tenant_traces")
        .body(one_span(now).encode_to_vec())
        .send()
        .await
        .expect("OTLP HTTP export");

    assert!(response.status().is_success(), "OTLP HTTP response");

    let client = server.client.clone();
    let url = format!("{}/api/v1/query", server.base_url);
    let org_id = server.root_org_id.0.clone();

    let stored = wait_until_async(30, || {
        let client = client.clone();
        let url = url.clone();
        let org_id = org_id.clone();
        let header_value = header_value.clone();

        async move {
            let Ok(response) = client
                .post(url)
                .header(header_name, header_value)
                .json(&json!({
                    "orgId": org_id,
                    "language": "sql",
                    "statement": "SELECT COUNT(*) AS n FROM tenant_traces",
                    "timeRange": { "start": now - 60_000_000, "end": now + 60_000_000 },
                    "stream": { "name": "tenant_traces", "streamType": "traces" }
                }))
                .send()
                .await
            else {
                return false;
            };

            let Ok(body) = response.json::<Value>().await else {
                return false;
            };

            body["rows"][0][0].as_i64().unwrap_or(0) >= 1
        }
    })
    .await;

    assert!(stored, "the tenant's span never became queryable");

    // The requests above have spans of their own: the sampler keeps them as well, with the tenant's.
    let sampler = server.state.telemetry.tail_sampler.clone();
    let own_kept = wait_until(30, move || sampler.metrics().kept >= 2).await;

    assert!(
        own_kept,
        "the process's own spans never reached the sampler"
    );

    // Anything of the process's own that got past the sink would be written or fail by now.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let health = server
        .state
        .telemetry
        .trace_pipeline
        .health()
        .self_intake
        .expect("the storing sink is there with self telemetry off");

    assert_eq!(
        health.exported_spans, 1,
        "only the tenant's span: {health:?}"
    );
    assert_eq!(health.failed_batches, 0, "{health:?}");

    let own_stream = server
        .state
        .telemetry
        .streams
        .get(
            &server.state.iam.system_org_id,
            MOLESIGNAL_SYSTEM_STREAM,
            StreamType::TRACES,
        )
        .await;

    assert!(
        own_stream.is_err(),
        "`_sys` has no traces stream of its own"
    );
}
