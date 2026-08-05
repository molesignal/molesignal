// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! gRPC server 装配入口。
//!
//! 当前挂载的 services：
//! - `ingest.v1.IngestService`（5.x） — 已上线
//! - `arrow_flight.FlightService`（9.x） — 已上线（do_get 实装，其余 RPC 返 Unimplemented）
//! - `cluster.v1.NodeService`（11.x） — 已上线（Heartbeat / List）
//!
//! 另有独立端口的对外 Flight SQL server（spec flight-sql）：[`serve_flight_sql`]，
//! 与上述可信网络端口隔离，默认关闭。

pub mod cluster_server;
pub mod event_server;
pub mod flight;
pub mod ingest_server;
pub mod otlp_server;
pub mod trace;

use std::{net::SocketAddr, sync::Arc};

use tonic::transport::Server;

use crate::{
    api::{
        AppState,
        grpc::{
            cluster_server::ClusterGrpc,
            event_server::EventServiceGrpc,
            flight::{FlightGrpc, sql::server::FlightSqlGrpc},
            ingest_server::IngestGrpc,
        },
    },
    config::{FlightSqlSettings, GrpcSettings, OtlpGrpcSettings},
    infra::persistence::repositories::cluster::nodes::ClusterNodesRepository,
};

/// 起 tonic server。`grpc.bind:port` 来自 settings；与 HTTP server 在 `bootstrap`
/// 中通过 `tokio::try_join!` 同进程并行。
pub async fn serve_grpc(
    state: AppState,
    grpc: &GrpcSettings,
    alive_window_secs: i64,
) -> anyhow::Result<()> {
    let _ = std::marker::PhantomData::<Arc<dyn ClusterNodesRepository>>;
    let addr: SocketAddr = format!("{}:{}", grpc.bind, grpc.port).parse()?;
    let max_recv = (grpc.max_message_size_mb as usize).saturating_mul(1024 * 1024);

    let ingest = IngestGrpc::new(state.ingestion.clone())
        .with_self_telemetry_token(state.telemetry.self_telemetry_cluster_token.clone())
        .into_server()
        .max_decoding_message_size(max_recv);
    // Flight server 收到 do_get 后用集群默认 store 读分片。
    let flight = FlightGrpc::new(state.storage.object_store.clone())
        .with_files(state.storage.parquet_file_meta.clone())
        .with_api_tokens(state.iam.api_tokens.clone())
        .with_iam(state.iam.service.clone())
        .with_cancel_registry(state.cluster.federation_cancel.clone())
        .into_server();
    let cluster =
        ClusterGrpc::new(state.cluster.repository.clone(), alive_window_secs).into_server();
    let trace_candidates = trace::candidate_server::TraceCandidateGrpc::new(
        state.telemetry.trace_pipeline.clone(),
        state.telemetry.self_telemetry_cluster_token.clone(),
    )
    .into_server()
    .max_decoding_message_size(max_recv);
    // 跨集群事件总线接收端：远端推 CloudEvent 进来，handler 自做 per-org token 鉴权
    // （与联邦 Flight `do_get` 同模型——可信网络端口上对联邦/跨集群调用自校验 bearer）。
    let events = EventServiceGrpc::new(state.clone())
        .into_server()
        .max_decoding_message_size(max_recv);

    tracing::info!(addr = %addr, "grpc server listening");
    Server::builder()
        .layer(trace::layer::GrpcTraceLayer)
        .add_service(ingest)
        .add_service(flight)
        .add_service(cluster)
        .add_service(trace_candidates)
        .add_service(events)
        .serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("grpc serve: {e}"))
}

/// 起对外**标准 OTLP gRPC** server（traces/logs/metrics/profiles 四 service）。
///
/// 与 [`serve_grpc`]（内部可信网络：`ingest.v1` shard 协议 + NodeService）分端口 ——
/// 本端口只挂 OTLP collector service，每个 `export` RPC 强制 Bearer 鉴权 + `StreamWrite`，
/// 可暴露给用户网络，并由包含 HTTP 或 Ingester 能力的节点始终启动。
pub async fn serve_otlp_grpc(state: AppState, settings: &OtlpGrpcSettings) -> anyhow::Result<()> {
    use opentelemetry_proto::tonic::collector::{
        logs::v1::logs_service_server::LogsServiceServer,
        metrics::v1::metrics_service_server::MetricsServiceServer,
        profiles::v1development::profiles_service_server::ProfilesServiceServer,
        trace::v1::trace_service_server::TraceServiceServer,
    };

    use crate::api::grpc::otlp_server::OtlpGrpc;

    let addr: SocketAddr = format!("{}:{}", settings.bind, settings.port).parse()?;
    let max_recv = (settings.max_message_size_mb as usize).saturating_mul(1024 * 1024);
    let svc = OtlpGrpc::new(state);

    tracing::info!(addr = %addr, "otlp grpc server listening");
    Server::builder()
        .layer(trace::layer::GrpcTraceLayer)
        .add_service(TraceServiceServer::new(svc.clone()).max_decoding_message_size(max_recv))
        .add_service(LogsServiceServer::new(svc.clone()).max_decoding_message_size(max_recv))
        .add_service(MetricsServiceServer::new(svc.clone()).max_decoding_message_size(max_recv))
        .add_service(ProfilesServiceServer::new(svc).max_decoding_message_size(max_recv))
        .serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("otlp grpc serve: {e}"))
}

/// 起对外 Flight SQL server（spec flight-sql）。
///
/// 与 [`serve_grpc`]（可信网络：免鉴权 shard 协议 + NodeService）分端口 ——
/// 本端口仅挂 Flight SQL service，每个 RPC 强制 API token 鉴权，可暴露给
/// 用户网络。caller（bootstrap）按 `flight_sql.enabled` 决定是否调用。
pub async fn serve_flight_sql(state: AppState, settings: &FlightSqlSettings) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", settings.bind, settings.port).parse()?;
    let max_recv = (settings.max_message_size_mb as usize).saturating_mul(1024 * 1024);

    let flight_sql = FlightSqlGrpc::new(
        state.query.clone(),
        state.iam.api_tokens.clone(),
        state.telemetry.streams.clone(),
        state.iam.service.clone(),
        state.iam.access.clone(),
        settings.clone(),
    )
    .into_server()
    .max_decoding_message_size(max_recv);

    tracing::info!(addr = %addr, "flight sql server listening");
    Server::builder()
        .layer(trace::layer::GrpcTraceLayer)
        .add_service(flight_sql)
        .serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("flight sql serve: {e}"))
}
