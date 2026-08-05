// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OTLP HTTP + gRPC → candidate owner → pre-sampling APM minute flush → API query.

mod common;

use molesignal::{api::grpc::otlp_server::OtlpGrpc, shared::time::TimestampMicros};
use opentelemetry_proto::tonic::{
    collector::trace::v1::{
        ExportTraceServiceRequest, trace_service_client::TraceServiceClient,
        trace_service_server::TraceServiceServer,
    },
    common::v1::{AnyValue, KeyValue, any_value},
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span},
};
use prost::Message;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, transport::Server};

fn string_attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.into())),
        }),
        ..Default::default()
    }
}

fn trace_request(
    service: &str,
    version: &str,
    trace_byte: u8,
    span_byte: u8,
    started_at_micros: i64,
) -> ExportTraceServiceRequest {
    let start_time_unix_nano =
        u64::try_from(started_at_micros.saturating_mul(1_000)).expect("positive timestamp");
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![
                    string_attribute("service.namespace", "shop"),
                    string_attribute("service.name", service),
                    string_attribute("service.version", version),
                    string_attribute("deployment.environment.name", "test"),
                    string_attribute("telemetry.sdk.name", "apm-it"),
                ],
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    trace_id: vec![trace_byte; 16],
                    span_id: vec![span_byte; 8],
                    name: "GET /orders/{id}".into(),
                    kind: 2,
                    start_time_unix_nano,
                    end_time_unix_nano: start_time_unix_nano + 20_000_000,
                    attributes: vec![
                        string_attribute("http.request.method", "GET"),
                        string_attribute("http.route", "/orders/{id}"),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

async fn spawn_grpc(state: molesignal::api::AppState) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind gRPC");
    let address = listener.local_addr().expect("gRPC address");
    tokio::spawn(async move {
        Server::builder()
            .add_service(TraceServiceServer::new(OtlpGrpc::new(state)))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("serve gRPC");
    });
    address
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otlp_http_and_grpc_project_once_and_are_queryable_through_apm_api() {
    if common::skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let server = common::TestServer::start_with_apm().await;
    let now = TimestampMicros::now().0;
    let http_request = trace_request("checkout", "1.0.0", 0x11, 0x22, now);

    for _ in 0..2 {
        let response = server
            .client
            .post(format!("{}/api/v1/traces", server.base_url))
            .header(server.auth_header().0, server.auth_header().1)
            .header("content-type", "application/x-protobuf")
            .header("stream-name", "apm_http")
            .body(http_request.encode_to_vec())
            .send()
            .await
            .expect("OTLP HTTP export");
        assert!(response.status().is_success(), "OTLP HTTP response");
    }

    let address = spawn_grpc(server.state.clone()).await;
    let mut grpc = TraceServiceClient::connect(format!("http://{address}"))
        .await
        .expect("connect gRPC");
    let mut request = Request::new(trace_request("inventory", "2.0.0", 0x33, 0x44, now + 1_000));
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", server.root_token)
            .parse()
            .expect("authorization metadata"),
    );
    request
        .metadata_mut()
        .insert("stream-name", "apm_grpc".parse().expect("stream metadata"));
    grpc.export(request).await.expect("OTLP gRPC export");

    let from = now - 60_000_000;
    let to = now + 60_000_000;
    let client = server.client.clone();
    let url = format!(
        "{}/api/v1/apm/overview?from={from}&to={to}",
        server.base_url
    );
    let auth = server.auth_header().1;
    let visible = common::wait_until_async(10, || {
        let client = client.clone();
        let url = url.clone();
        let auth = auth.clone();
        async move {
            let Ok(response) = client.get(url).header("authorization", auth).send().await else {
                return false;
            };
            let Ok(body) = response.json::<serde_json::Value>().await else {
                return false;
            };
            body["red"]["request_count"].as_u64() == Some(2)
                && body["services"]
                    .as_array()
                    .is_some_and(|services| services.len() == 2)
        }
    })
    .await;
    assert!(
        visible,
        "HTTP retry must be deduplicated while HTTP and gRPC services both project"
    );

    let health = server
        .client
        .get(format!(
            "{}/api/v1/apm/health?from={from}&to={to}",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .expect("APM health");
    assert!(health.status().is_success());
    let health: serde_json::Value = health.json().await.expect("APM health body");
    assert_eq!(health["enabled"], true);
    assert_eq!(health["degraded"], false);
}
