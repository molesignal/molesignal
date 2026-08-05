// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `ingest.v1.IngestService` 的 server 端实现。
//!
//! payload 约定：`serde_json::to_vec(&Vec<RawEvent>)`（client 把整批事件序列化为
//! JSON 数组；server 反序列化后包成 `IngestBatch` 调 [`IngestService::ingest`]）。
//!
//! 协议层错误（payload 不解码 / 缺字段）→ `tonic::Status::invalid_argument`；
//! 应用层错误（schema 不匹配 / sink fail）→ `internal`。

use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::{
    app::ingestion::IngestService,
    domain::{
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    protocol::ingest::v1::{
        IngestError as ProtoIngestError, PushRequest, PushResponse, StreamType as ProtoStreamType,
        ingest_service_server::{IngestService as IngestServiceTrait, IngestServiceServer},
    },
    shared::{ids::Id, time::TimestampMicros},
};

pub struct IngestGrpc {
    service: Arc<IngestService>,
    self_telemetry_token: Option<Arc<str>>,
}

impl IngestGrpc {
    pub fn new(service: Arc<IngestService>) -> Self {
        Self {
            service,
            self_telemetry_token: None,
        }
    }

    pub fn with_self_telemetry_token(mut self, token: Option<Arc<str>>) -> Self {
        self.self_telemetry_token = token;
        self
    }

    /// 包装成可挂到 tonic Server 的 service。
    pub fn into_server(self) -> IngestServiceServer<Self> {
        IngestServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl IngestServiceTrait for IngestGrpc {
    async fn push(&self, request: Request<PushRequest>) -> Result<Response<PushResponse>, Status> {
        let internal_self_telemetry =
            authenticate_internal_self_telemetry(&request, self.self_telemetry_token.as_deref())?;
        let PushRequest {
            batch_id,
            org_id,
            stream,
            stream_type,
            payload,
            received_at_micros,
        } = request.into_inner();

        let stream_type = proto_stream_type_to_domain(stream_type)
            .ok_or_else(|| Status::invalid_argument("stream_type unspecified"))?;

        let events: Vec<RawEvent> = serde_json::from_slice(&payload)
            .map_err(|e| Status::invalid_argument(format!("payload decode: {e}")))?;

        let batch_id = if batch_id.is_empty() {
            Id::new()
        } else {
            Id::from_string(batch_id)
        };
        let received_at = if received_at_micros == 0 {
            TimestampMicros::now()
        } else {
            TimestampMicros(received_at_micros)
        };
        let batch = IngestBatch {
            batch_id,
            org_id: Id::from_string(org_id),
            stream,
            stream_type,
            events,
            received_at,
        };

        let result = if internal_self_telemetry {
            self.service.ingest_self_telemetry(batch).await
        } else {
            self.service.ingest(batch).await
        }
        .map_err(error_to_status)?;

        let errors = result
            .errors
            .into_iter()
            .map(|e| ProtoIngestError {
                index: e.index as u32,
                reason: e.reason,
            })
            .collect();
        Ok(Response::new(PushResponse {
            accepted: result.accepted as u32,
            rejected: result.rejected as u32,
            errors,
        }))
    }
}

fn authenticate_internal_self_telemetry(
    request: &Request<PushRequest>,
    expected_token: Option<&str>,
) -> Result<bool, Status> {
    const ORIGIN_HEADER: &str = "x-molesignal-internal-origin";
    let Some(origin) = request.metadata().get(ORIGIN_HEADER) else {
        return Ok(false);
    };
    if origin.to_str().ok() != Some("self-telemetry") {
        return Err(Status::permission_denied("unsupported internal origin"));
    }
    let expected = expected_token
        .ok_or_else(|| Status::unauthenticated("internal self telemetry is not configured"))?;
    let presented = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| Status::unauthenticated("missing internal bearer"))?;
    if !crate::app::self_telemetry::cluster_token_matches(expected, presented) {
        return Err(Status::unauthenticated("invalid internal bearer"));
    }
    Ok(true)
}

fn error_to_status(error: crate::shared::Error) -> Status {
    match error {
        crate::shared::Error::InvalidArgument(message) => Status::invalid_argument(message),
        crate::shared::Error::Validation { message, .. } => Status::invalid_argument(message),
        crate::shared::Error::Unauthorized(message) => Status::unauthenticated(message),
        crate::shared::Error::Forbidden(message) => Status::permission_denied(message),
        crate::shared::Error::Unavailable(message) => Status::unavailable(message),
        other => Status::internal(format!("ingest: {other}")),
    }
}

fn proto_stream_type_to_domain(v: i32) -> Option<StreamType> {
    match ProtoStreamType::try_from(v).ok()? {
        ProtoStreamType::Logs => Some(StreamType::Logs),
        ProtoStreamType::Metrics => Some(StreamType::Metrics),
        ProtoStreamType::Traces => Some(StreamType::Traces),
        ProtoStreamType::Profiles => Some(StreamType::Profiles),
        ProtoStreamType::Unspecified => None,
    }
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    fn request(origin: Option<&str>, bearer: Option<&str>) -> Request<PushRequest> {
        let mut request = Request::new(PushRequest::default());
        if let Some(origin) = origin {
            request
                .metadata_mut()
                .insert("x-molesignal-internal-origin", origin.parse().unwrap());
        }
        if let Some(bearer) = bearer {
            request
                .metadata_mut()
                .insert("authorization", format!("Bearer {bearer}").parse().unwrap());
        }
        request
    }

    #[test]
    fn ordinary_grpc_ingest_does_not_gain_internal_privileges() {
        assert!(
            !authenticate_internal_self_telemetry(&request(None, None), Some("secret")).unwrap()
        );
    }

    #[test]
    fn internal_origin_requires_the_cluster_token() {
        let missing = authenticate_internal_self_telemetry(
            &request(Some("self-telemetry"), None),
            Some("secret"),
        )
        .unwrap_err();
        assert_eq!(missing.code(), Code::Unauthenticated);

        let wrong = authenticate_internal_self_telemetry(
            &request(Some("self-telemetry"), Some("wrong")),
            Some("secret"),
        )
        .unwrap_err();
        assert_eq!(wrong.code(), Code::Unauthenticated);

        assert!(
            authenticate_internal_self_telemetry(
                &request(Some("self-telemetry"), Some("secret")),
                Some("secret")
            )
            .unwrap()
        );
    }
}
