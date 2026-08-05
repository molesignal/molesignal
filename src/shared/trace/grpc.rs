// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared Tonic client boundary with W3C propagation and bounded RPC attributes.

use std::future::Future;

use tonic::{Code, Request, Response, Status};
use tracing::{Instrument, field};

use crate::shared::{
    ids::Id,
    trace_context::{TraceContext, current_trace_context, with_current_trace_context},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrpcTarget {
    Internal,
    ThirdParty,
}

impl GrpcTarget {
    fn is_internal(self) -> bool {
        matches!(self, Self::Internal)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::ThirdParty => "third_party",
        }
    }
}

/// Inject a child context and execute a generated Tonic client method.
///
/// `service` and `method` must be static protocol names, never data-derived values.
pub async fn call<T, U, F, Fut>(
    mut request: Request<T>,
    service: &'static str,
    method: &'static str,
    target: GrpcTarget,
    invoke: F,
) -> Result<Response<U>, Status>
where
    F: FnOnce(Request<T>) -> Fut,
    Fut: Future<Output = Result<Response<U>, Status>>,
{
    let context = current_trace_context()
        .map(|context| context.child())
        .unwrap_or_else(|| TraceContext::new_root(Id::new().0));
    context.inject_grpc(request.metadata_mut(), target.is_internal());

    let span = tracing::info_span!(
        "rpc.client",
        otel.name = %format!("{service}/{method}"),
        otel.kind = "client",
        otel.trace_id = %context.trace_id,
        otel.span_id = %context.span_id,
        otel.parent_span_id = context.parent_span_id.as_deref().unwrap_or(""),
        rpc.system = "grpc",
        rpc.service = service,
        rpc.method = method,
        rpc.grpc.status_code = field::Empty,
        molesignal.rpc.target = target.label(),
        error.type = field::Empty,
    );
    let instrument_span = span.clone();
    let result = with_current_trace_context(context, invoke(request))
        .instrument(instrument_span)
        .await;
    match &result {
        Ok(_) => {
            span.record("rpc.grpc.status_code", Code::Ok as i32);
        }
        Err(status) => {
            span.record("rpc.grpc.status_code", status.code() as i32);
            span.record("error.type", grpc_error_type(status.code()));
        }
    }
    result
}

fn grpc_error_type(code: Code) -> &'static str {
    match code {
        Code::Ok => "ok",
        Code::Cancelled => "grpc_cancelled",
        Code::Unknown => "grpc_unknown",
        Code::InvalidArgument => "grpc_invalid_argument",
        Code::DeadlineExceeded => "grpc_deadline_exceeded",
        Code::NotFound => "grpc_not_found",
        Code::AlreadyExists => "grpc_already_exists",
        Code::PermissionDenied => "grpc_permission_denied",
        Code::ResourceExhausted => "grpc_resource_exhausted",
        Code::FailedPrecondition => "grpc_failed_precondition",
        Code::Aborted => "grpc_aborted",
        Code::OutOfRange => "grpc_out_of_range",
        Code::Unimplemented => "grpc_unimplemented",
        Code::Internal => "grpc_internal",
        Code::Unavailable => "grpc_unavailable",
        Code::DataLoss => "grpc_data_loss",
        Code::Unauthenticated => "grpc_unauthenticated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::trace_context::{
        BAGGAGE, TRACE_DEBUG_TOKEN, TRACE_FORCE, TRACEPARENT, with_current_trace_context,
    };

    fn context() -> TraceContext {
        let mut context = TraceContext::new_root("request-123");
        context.trace_id = "0af7651916cd43dd8448eb211c80319c".into();
        context.span_id = "b7ad6b7169203331".into();
        context.baggage.insert("org.id".into(), "org-1".into());
        context
            .baggage
            .insert("request.id".into(), "request-123".into());
        context
    }

    #[tokio::test]
    async fn internal_call_injects_only_whitelisted_baggage() {
        with_current_trace_context(context(), async {
            let mut request = Request::new(());
            request
                .metadata_mut()
                .insert(BAGGAGE, "user.id=secret".parse().unwrap());
            let response = call(
                request,
                "test.Service",
                "Call",
                GrpcTarget::Internal,
                |request| async move {
                    assert!(request.metadata().contains_key(TRACEPARENT));
                    assert_eq!(
                        request.metadata().get(BAGGAGE).unwrap().to_str().unwrap(),
                        "org.id=org-1,request.id=request-123"
                    );
                    Ok(Response::new(()))
                },
            )
            .await
            .unwrap();
            assert_eq!(response.into_inner(), ());
        })
        .await;
    }

    #[tokio::test]
    async fn third_party_call_strips_internal_control_metadata() {
        with_current_trace_context(context(), async {
            let mut request = Request::new(());
            request
                .metadata_mut()
                .insert(BAGGAGE, "org.id=secret".parse().unwrap());
            request
                .metadata_mut()
                .insert(TRACE_FORCE, "true".parse().unwrap());
            request
                .metadata_mut()
                .insert(TRACE_DEBUG_TOKEN, "secret".parse().unwrap());
            call(
                request,
                "test.Service",
                "Call",
                GrpcTarget::ThirdParty,
                |request| async move {
                    assert!(request.metadata().contains_key(TRACEPARENT));
                    assert!(!request.metadata().contains_key(BAGGAGE));
                    assert!(!request.metadata().contains_key(TRACE_FORCE));
                    assert!(!request.metadata().contains_key(TRACE_DEBUG_TOKEN));
                    Ok(Response::new(()))
                },
            )
            .await
            .unwrap();
        })
        .await;
    }
}
