// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集群内部 CanonicalSpan candidate 接收端。
//!
//! 该 service 只挂在内部 gRPC listener；即便误暴露到公共网络，调用方仍必须同时
//! 提供内部 origin 标记和集群 bearer。接收端不修改 producer 的 Resource/Scope，
//! 只把完整 CanonicalSpan 放入本节点的有界 tail-sampler queue。

use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::{
    app::trace::{TracePipeline, TraceSubmitError},
    protocol::cluster::v1::{
        SubmitTraceCandidateRequest, SubmitTraceCandidateResponse, TraceCandidateDisposition,
        TraceForceKeep,
        trace_candidate_service_server::{TraceCandidateService, TraceCandidateServiceServer},
    },
    shared::{
        tail_sampling::{ForceKeep, TraceCandidate},
        trace_normalization::{CANONICAL_SPAN_SCHEMA_VERSION, CanonicalSpan},
    },
};

pub const TRACE_CANDIDATE_ORIGIN_HEADER: &str = "x-molesignal-internal-origin";
pub const TRACE_CANDIDATE_ORIGIN_VALUE: &str = "trace-candidate";
pub const MAX_TRACE_CANDIDATE_BYTES: usize = 2 * 1024 * 1024;

pub struct TraceCandidateGrpc {
    pipeline: Arc<TracePipeline>,
    cluster_token: Option<Arc<str>>,
}

impl TraceCandidateGrpc {
    pub fn new(pipeline: Arc<TracePipeline>, cluster_token: Option<Arc<str>>) -> Self {
        Self {
            pipeline,
            cluster_token,
        }
    }

    pub fn into_server(self) -> TraceCandidateServiceServer<Self> {
        TraceCandidateServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl TraceCandidateService for TraceCandidateGrpc {
    async fn submit(
        &self,
        request: Request<SubmitTraceCandidateRequest>,
    ) -> Result<Response<SubmitTraceCandidateResponse>, Status> {
        authenticate_cluster_candidate(&request, self.cluster_token.as_deref())?;
        let candidate = decode_candidate(request.into_inner())?;
        let disposition = match self.pipeline.try_submit(candidate) {
            Ok(()) => TraceCandidateDisposition::Accepted,
            Err(TraceSubmitError::Full) => TraceCandidateDisposition::Overloaded,
            Err(TraceSubmitError::Stopped) => TraceCandidateDisposition::Stopped,
        };
        Ok(Response::new(SubmitTraceCandidateResponse {
            disposition: disposition as i32,
        }))
    }
}

fn authenticate_cluster_candidate<T>(
    request: &Request<T>,
    expected_token: Option<&str>,
) -> Result<(), Status> {
    let origin = request
        .metadata()
        .get(TRACE_CANDIDATE_ORIGIN_HEADER)
        .and_then(|value| value.to_str().ok());
    if origin != Some(TRACE_CANDIDATE_ORIGIN_VALUE) {
        return Err(Status::permission_denied(
            "internal Trace candidate origin required",
        ));
    }
    let expected = expected_token
        .ok_or_else(|| Status::unauthenticated("cluster candidate transport is not configured"))?;
    let presented = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| Status::unauthenticated("missing cluster bearer"))?;
    if !crate::app::self_telemetry::cluster_token_matches(expected, presented) {
        return Err(Status::unauthenticated("invalid cluster bearer"));
    }
    Ok(())
}

fn decode_candidate(request: SubmitTraceCandidateRequest) -> Result<TraceCandidate, Status> {
    if request.org_id.is_empty() || request.org_id.len() > 256 {
        return Err(Status::invalid_argument("invalid org_id"));
    }
    if request.producer_node_id.is_empty() || request.producer_node_id.len() > 256 {
        return Err(Status::invalid_argument("invalid producer_node_id"));
    }
    if request.canonical_span_json.is_empty()
        || request.canonical_span_json.len() > MAX_TRACE_CANDIDATE_BYTES
    {
        return Err(Status::invalid_argument(
            "CanonicalSpan candidate size is out of bounds",
        ));
    }
    let stream = if request.system_self_trace {
        if !request.stream.is_empty() {
            return Err(Status::invalid_argument(
                "system Trace candidate must not carry a tenant stream",
            ));
        }
        None
    } else {
        let stream = request.stream.trim();
        if stream.is_empty() || stream.len() > 256 {
            return Err(Status::invalid_argument("invalid tenant Trace stream"));
        }
        Some(stream.to_owned())
    };
    let span: CanonicalSpan = serde_json::from_slice(&request.canonical_span_json)
        .map_err(|_| Status::invalid_argument("invalid CanonicalSpan candidate"))?;
    if span.schema_version != CANONICAL_SPAN_SCHEMA_VERSION
        || !valid_hex_id(&span.trace_id, 32)
        || !valid_hex_id(&span.span_id, 16)
        || span
            .parent_span_id
            .as_deref()
            .is_some_and(|value| !valid_hex_id(value, 16))
    {
        return Err(Status::invalid_argument(
            "unsupported or malformed CanonicalSpan candidate",
        ));
    }
    let force_keep =
        match TraceForceKeep::try_from(request.force_keep).unwrap_or(TraceForceKeep::Unspecified) {
            TraceForceKeep::Unspecified => ForceKeep::None,
            TraceForceKeep::TrustedInternal => ForceKeep::TrustedInternal,
            TraceForceKeep::DebugToken => ForceKeep::DebugToken,
        };
    Ok(TraceCandidate {
        org_id: request.org_id,
        stream,
        span,
        force_keep,
    })
}

fn valid_hex_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(origin: Option<&str>, bearer: Option<&str>) -> Request<()> {
        let mut request = Request::new(());
        if let Some(origin) = origin {
            request
                .metadata_mut()
                .insert(TRACE_CANDIDATE_ORIGIN_HEADER, origin.parse().unwrap());
        }
        if let Some(bearer) = bearer {
            request
                .metadata_mut()
                .insert("authorization", format!("Bearer {bearer}").parse().unwrap());
        }
        request
    }

    #[test]
    fn public_or_spoofed_callers_are_rejected() {
        assert!(authenticate_cluster_candidate(&request(None, None), Some("secret")).is_err());
        assert!(
            authenticate_cluster_candidate(
                &request(Some(TRACE_CANDIDATE_ORIGIN_VALUE), Some("wrong")),
                Some("secret")
            )
            .is_err()
        );
        authenticate_cluster_candidate(
            &request(Some(TRACE_CANDIDATE_ORIGIN_VALUE), Some("secret")),
            Some("secret"),
        )
        .unwrap();
    }

    #[test]
    fn candidate_decoder_preserves_resource_and_scope() {
        let mut span = crate::shared::trace_fixtures::canonical_http_trace()
            .into_iter()
            .next()
            .unwrap();
        span.resource
            .attributes
            .insert("molesignal.node.id".into(), serde_json::json!("producer-a"));
        span.scope.name = "producer.instrumentation".into();
        let decoded = decode_candidate(SubmitTraceCandidateRequest {
            org_id: "org-a".into(),
            stream: "default".into(),
            system_self_trace: false,
            canonical_span_json: serde_json::to_vec(&span).unwrap().into(),
            force_keep: TraceForceKeep::TrustedInternal as i32,
            producer_node_id: "producer-a".into(),
            produced_at_micros: 1,
        })
        .unwrap();
        assert_eq!(decoded.span.resource, span.resource);
        assert_eq!(decoded.span.scope, span.scope);
        assert_eq!(decoded.force_keep, ForceKeep::TrustedInternal);
    }
}
