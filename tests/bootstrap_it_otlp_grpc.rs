// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对外 OTLP gRPC 端到端：起 `OtlpGrpc` 四 service 的 tonic server（随机端口）→ 用标准
//! OTLP `TraceServiceClient` export →
//! 1. 无 `authorization` metadata → `Unauthenticated`；
//! 2. 带 Bearer（root JWT，Owner → StreamWrite）→ ok；
//! 3. 等 IngesterWorker flush → `/api/v1/query` 验证 span 落到 traces 流。
//!
//! 与 `it_grpc_ingest`（内部 ingest.v1，免鉴权、绕过 AppState）不同：OTLP gRPC 走完整
//! `AppState`（鉴权 + 计费门禁 + ingestion），故复用 `common::TestServer` 的真实栈。
//! 需 `MS_RUN_IT=1` + docker（postgres testcontainer）才跑。

mod common;

use common::{TestServer, skip_unless_enabled};
use molesignal::api::grpc::otlp_server::OtlpGrpc;
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::logs_service_server::LogsServiceServer,
        metrics::v1::metrics_service_server::MetricsServiceServer,
        profiles::v1development::profiles_service_server::ProfilesServiceServer,
        trace::v1::{
            ExportTraceServiceRequest, trace_service_client::TraceServiceClient,
            trace_service_server::TraceServiceServer,
        },
    },
    common::v1::{AnyValue, KeyValue, any_value},
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span},
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, transport::Server};

/// 起 OTLP gRPC server（四 service，随机端口），返回监听地址。装配与
/// `molesignal::api::grpc::serve_otlp_grpc` 一致，只是换成 `serve_with_incoming`
/// 以拿到随机端口（避免固定 :4317 在 CI 上冲突）。
async fn spawn_otlp_grpc(state: molesignal::api::AppState) -> std::net::SocketAddr {
    let svc = OtlpGrpc::new(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    tokio::spawn(async move {
        Server::builder()
            .add_service(TraceServiceServer::new(svc.clone()))
            .add_service(LogsServiceServer::new(svc.clone()))
            .add_service(MetricsServiceServer::new(svc.clone()))
            .add_service(ProfilesServiceServer::new(svc))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    addr
}

fn str_kv(key: &str, val: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(val.into())),
        }),
        ..Default::default()
    }
}

/// 一条 span 的最小 OTLP trace 请求（service.name=checkout，start=1ms）。
fn one_span_request() -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![str_kv("service.name", "checkout")],
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    trace_id: vec![0x11; 16],
                    span_id: vec![0x22; 8],
                    name: "GET /checkout".into(),
                    start_time_unix_nano: 1_000_000,
                    end_time_unix_nano: 2_000_000,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otlp_grpc_traces_authz_and_lands() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;
    let addr = spawn_otlp_grpc(s.state.clone()).await;

    // 等 server ready（重试 connect）。
    let mut client_opt = None;
    for _ in 0..50 {
        match TraceServiceClient::connect(format!("http://{addr}")).await {
            Ok(c) => {
                client_opt = Some(c);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    let mut client = client_opt.expect("connect otlp grpc server");

    // 1) 无 authorization metadata → Unauthenticated。
    let unauth = client.export(Request::new(one_span_request())).await;
    assert_eq!(
        unauth.expect_err("missing bearer must be rejected").code(),
        tonic::Code::Unauthenticated,
    );

    // 2) 带 Bearer（root JWT，Owner → StreamWrite）+ stream-name → ok。
    let mut req = Request::new(one_span_request());
    req.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", s.root_token).parse().unwrap(),
    );
    req.metadata_mut()
        .insert("stream-name", "otel_traces".parse().unwrap());
    client
        .export(req)
        .await
        .expect("authed export must succeed");

    // 3) 等 flush 出 parquet_file_meta，再 query 验证 span 落到 traces/otel_traces 流。
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&s.settings.store.meta.dsn)
        .await
        .expect("test pool");
    let flushed = {
        let pool = pool.clone();
        common::wait_until_async(10, move || {
            let pool = pool.clone();
            async move {
                let row: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM parquet_file_meta WHERE deleted = FALSE")
                        .fetch_one(&pool)
                        .await
                        .unwrap_or((0,));
                row.0 >= 1
            }
        })
        .await
    };
    assert!(
        flushed,
        "otlp traces never flushed to parquet_file_meta within timeout"
    );

    let resp = s
        .client
        .post(format!("{}/api/v1/query", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&serde_json::json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT COUNT(*) AS n FROM otel_traces",
            "time_range": { "start": 0, "end": 10_000_000 },
            "stream": { "name": "otel_traces", "stream_type": "traces" }
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body_text = resp.text().await.unwrap();
    assert_eq!(status, 200, "query status; body={body_text}");
    let body: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    assert!(
        body["rows"][0][0].as_i64().unwrap_or(0) >= 1,
        "exported span must be queryable; body={body_text}"
    );
}
