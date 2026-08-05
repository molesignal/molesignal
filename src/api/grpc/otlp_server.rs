// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对外**标准 OTLP gRPC** receiver：`opentelemetry.proto.collector.{trace,logs,
//! metrics,profiles}.v1*` 四个 `Export` service。
//!
//! 与内部 `ingest.v1.IngestService`（[`super::ingest_server`]，router↔ingester
//! 私有分发、可信网络免鉴权）**不同协议、分端口**部署：本 server 由
//! [`super::serve_otlp_grpc`] 挂在 `otlp_grpc.bind:port`（默认 4317），可暴露给
//! 用户网络 —— 每个 `export` RPC 都要求 `authorization: Bearer`（与 HTTP 中间件
//! 同一套前缀分发：`ms_` → API token，其余 → JWT）+ `StreamWrite` 权限。
//!
//! 传输层之外的语义与 OTLP/HTTP（[`crate::api::http::routes::ingest::otlp`]）完全
//! 一致：复用同一批 `*_to_events` 转换 + `normalize_otlp_profiles` + 计费门禁 +
//! `IngestService::ingest`，只是 payload 由 tonic 解码而非手动 protobuf/JSON 解码。

use opentelemetry_proto::tonic::collector::{
    logs::v1::{
        ExportLogsServiceRequest, ExportLogsServiceResponse, logs_service_server::LogsService,
    },
    metrics::v1::{
        ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        metrics_service_server::MetricsService,
    },
    profiles::v1development::{
        ExportProfilesServiceRequest, ExportProfilesServiceResponse,
        profiles_service_server::ProfilesService,
    },
    trace::v1::{
        ExportTraceServiceRequest, ExportTraceServiceResponse, trace_service_server::TraceService,
    },
};
use prost::Message as _;
use tonic::{Request, Response, Status};

use crate::{
    api::{
        AppState,
        http::{
            middleware::{Permission, auth::authenticate_bearer},
            routes::{
                ingest::otlp::{
                    ingest, logs_to_events, metrics_to_events, submit_traces, traces_to_canonical,
                },
                profiles::{normalize_otlp_profiles, store_profile},
            },
        },
    },
    app::iam::IamContext,
    domain::{ingestion::RawEvent, stream::StreamType},
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{
        Error as MsError,
        ids::Id,
        time::TimestampMicros,
        trace_context::{
            TRACE_DEBUG_TOKEN, TraceContext, TraceTrust, update_current_trace_context,
        },
    },
};

/// 标准 OTLP gRPC 四 service 的共享实现（持 `AppState`，Arc 内部共享、clone 廉价）。
#[derive(Clone)]
pub struct OtlpGrpc {
    state: AppState,
}

impl OtlpGrpc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// per-RPC 鉴权：照 Flight SQL 同一套 metadata-Bearer 分发，权限要求 `StreamWrite`
    /// （摄取是写路径，自动拒只读的 Viewer token）。
    async fn authenticate<T>(&self, request: &Request<T>) -> Result<IamContext, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| {
                h.strip_prefix("Bearer ")
                    .or_else(|| h.strip_prefix("bearer "))
            })
            .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;
        let ctx = authenticate_bearer(
            bearer,
            self.state.iam.service.as_ref(),
            self.state.iam.api_tokens.clone(),
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;
        Permission::require_key(&ctx, "streams.write")
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        update_current_trace_context(|trace_context| {
            trace_context.set_authenticated_org(ctx.org_id.as_str());
        });
        Ok(ctx)
    }

    /// 计费门禁 + ingest（复用 OTLP/HTTP 的同名 helper）。
    async fn ingest_events(
        &self,
        ctx: &IamContext,
        stream_type: StreamType,
        stream: String,
        events: Vec<RawEvent>,
        bytes: usize,
    ) -> Result<(), Status> {
        ingest(
            &self.state,
            stream_type,
            ctx.org_id.clone(),
            stream,
            events,
            bytes,
        )
        .await
        .map_err(error_to_status)?;
        Ok(())
    }
}

/// `stream-name` metadata 头选目标 stream，缺省 `default`（与 OTLP/HTTP 一致）。
fn stream_name<T>(request: &Request<T>) -> String {
    request
        .metadata()
        .get("stream-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

/// `MsError` → `tonic::Status`（与 `flight_sql_server` 同一套映射；gRPC 无 402/413
/// 对应，订阅失效 → `permission_denied`、配额超限 → `resource_exhausted`）。
fn error_to_status(e: MsError) -> Status {
    match &e {
        MsError::NotFound(_) => Status::not_found(e.to_string()),
        MsError::InvalidArgument(_) | MsError::Validation { .. } => {
            Status::invalid_argument(e.to_string())
        }
        MsError::Unauthorized(_) => Status::unauthenticated(e.to_string()),
        MsError::Forbidden(_) | MsError::PaymentRequired(_) => {
            Status::permission_denied(e.to_string())
        }
        MsError::ResourceExhausted(_) | MsError::PayloadTooLarge(_) => {
            Status::resource_exhausted(e.to_string())
        }
        MsError::Unavailable(_) => Status::unavailable(e.to_string()),
        MsError::Cancelled(_) => Status::cancelled(e.to_string()),
        MsError::Conflict(_) => Status::aborted(e.to_string()),
        MsError::Internal(_) | MsError::Other(_) => {
            tracing::error!(error = ?e, "otlp grpc internal error");
            Status::internal("internal error")
        }
    }
}

#[tonic::async_trait]
impl TraceService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let stream = stream_name(&request);
        let mut trace_context = request
            .extensions()
            .get::<TraceContext>()
            .cloned()
            .unwrap_or_else(|| TraceContext::new_root(crate::shared::ids::Id::new().0));
        trace_context.set_authenticated_org(ctx.org_id.as_str());
        if let Some(debug_token) = request
            .metadata()
            .get(TRACE_DEBUG_TOKEN)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.starts_with("mstd_") && value.len() <= 160)
        {
            let token_hash = blake3::hash(debug_token.as_bytes()).to_hex().to_string();
            if let Some(grant) = self
                .state
                .telemetry
                .trace_debug_tokens
                .consume(
                    &token_hash,
                    Some(&ctx.org_id),
                    Some("/opentelemetry.proto.collector.trace.v1.TraceService/Export"),
                    TimestampMicros::now(),
                )
                .await
                .map_err(error_to_status)?
            {
                trace_context.trust = TraceTrust::DebugToken;
                trace_context.apply_trusted_force(true);
                update_current_trace_context(|active| {
                    active.trust = TraceTrust::DebugToken;
                    active.set_authenticated_org(ctx.org_id.as_str());
                    active.apply_trusted_force(true);
                });
                let _ = self
                    .state
                    .iam
                    .audit_events
                    .record(AuditEvent {
                        id: Id::new(),
                        org_id: self.state.iam.system_org_id.clone(),
                        actor_kind: "user".into(),
                        actor_id: ctx.user_id.0.clone(),
                        action: "trace_debug_token.use".into(),
                        target_kind: Some("trace_debug_token".into()),
                        target_id: Some(grant.id.0),
                        ip: None,
                        user_agent: None,
                        payload: serde_json::json!({
                            "organization_id": ctx.org_id.0,
                            "route": "/opentelemetry.proto.collector.trace.v1.TraceService/Export",
                            "used_count": grant.used_count,
                        }),
                        ts: TimestampMicros::now(),
                    })
                    .await;
            }
        }
        let suppress_external = request
            .metadata()
            .contains_key(crate::app::trace::export::SELF_EXPORT_MARKER);
        let req = request.into_inner();
        let bytes = req.encoded_len();
        let spans = traces_to_canonical(req);
        submit_traces(
            &self.state,
            ctx.org_id,
            stream,
            spans,
            bytes,
            &trace_context,
            suppress_external,
        )
        .await
        .map_err(error_to_status)?;
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl LogsService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let stream = stream_name(&request);
        let req = request.into_inner();
        let bytes = req.encoded_len();
        let events = logs_to_events(req);
        self.ingest_events(&ctx, StreamType::Logs, stream, events, bytes)
            .await?;
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl MetricsService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let stream = stream_name(&request);
        let req = request.into_inner();
        let bytes = req.encoded_len();
        let events = metrics_to_events(req);
        self.ingest_events(&ctx, StreamType::Metrics, stream, events, bytes)
            .await?;
        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl ProfilesService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportProfilesServiceRequest>,
    ) -> Result<Response<ExportProfilesServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let req = request.into_inner();
        let bytes = req.encoded_len();
        // 一次请求含共享 dictionary + N 个 profile → N 个 NormalizedProfile，逐个落盘。
        let normalized = normalize_otlp_profiles(&req);
        for profile in &normalized {
            let raw = crate::infra::profiles::encode_pprof_raw(profile).map_err(error_to_status)?;
            store_profile(&self.state, &ctx.org_id, profile, &raw, bytes)
                .await
                .map_err(error_to_status)?;
        }
        Ok(Response::new(ExportProfilesServiceResponse::default()))
    }
}
