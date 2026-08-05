// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 所有 Tonic/Arrow Flight server 共用的 W3C 上下文与关联 metadata 层。

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::{HeaderName, HeaderValue, Request, Response};
use tower::{Layer, Service};
use tracing::{Instrument, field};

use crate::shared::trace_context::{
    REQUEST_ID, TRACE_ID, TraceContext, TraceTrust, with_current_trace_context,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct GrpcTraceLayer;

impl<S> Layer<S> for GrpcTraceLayer {
    type Service = GrpcTraceService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcTraceService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct GrpcTraceService<S> {
    inner: S,
}

impl<S, B, R> Service<Request<B>> for GrpcTraceService<S>
where
    S: Service<Request<B>, Response = Response<R>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + 'static,
    B: Send + 'static,
    R: Send + 'static,
{
    type Response = Response<R>;
    type Error = S::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: Request<B>) -> Self::Future {
        let context = TraceContext::extract_http(request.headers(), TraceTrust::External);
        let path = request.uri().path().trim_start_matches('/');
        let (service, method) = path
            .rsplit_once('/')
            .map(|(service, method)| (service.to_string(), method.to_string()))
            .unwrap_or_else(|| ("unknown".into(), "unknown".into()));
        request.extensions_mut().insert(context.clone());

        let span = tracing::info_span!(
            "rpc.server",
            otel.name = %format!("{service}/{method}"),
            otel.kind = "server",
            otel.trace_id = %context.trace_id,
            otel.span_id = %context.span_id,
            otel.parent_span_id = context.parent_span_id.as_deref().unwrap_or(""),
            rpc.system = "grpc",
            rpc.service = %service,
            rpc.method = %method,
            rpc.grpc.status_code = field::Empty,
            request.id = %context.request_id,
            error.type = field::Empty,
        );
        let mut inner = self.inner.clone();
        let instrument_span = span.clone();
        Box::pin(
            with_current_trace_context(context.clone(), async move {
                match inner.call(request).await {
                    Ok(mut response) => {
                        insert_header(&mut response, TRACE_ID, &context.trace_id);
                        insert_header(&mut response, REQUEST_ID, &context.request_id);
                        if let Some(code) = response
                            .headers()
                            .get("grpc-status")
                            .and_then(|value| value.to_str().ok())
                        {
                            span.record("rpc.grpc.status_code", code);
                            if code != "0" {
                                span.record("error.type", format!("grpc_status_{code}"));
                            }
                        }
                        Ok(response)
                    }
                    Err(error) => {
                        // `Display` may contain request data or backend details. Server spans only
                        // carry a bounded error category; the concrete error remains in normal logs.
                        span.record("error.type", "service_error");
                        Err(error)
                    }
                }
            })
            .instrument(instrument_span),
        )
    }
}

fn insert_header<R>(response: &mut Response<R>, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(name), value);
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use tower::service_fn;

    use super::*;

    #[tokio::test]
    async fn extracts_context_and_adds_correlation_metadata() {
        let inner = service_fn(|request: Request<()>| async move {
            let context = request.extensions().get::<TraceContext>().unwrap();
            assert_eq!(context.trace_id, "0af7651916cd43dd8448eb211c80319c");
            Ok::<_, Infallible>(Response::new(()))
        });
        let mut service = GrpcTraceLayer.layer(inner);
        let response = service
            .call(
                Request::builder()
                    .uri("/ingest.v1.IngestService/Push")
                    .header(
                        "traceparent",
                        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00",
                    )
                    .body(())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.headers()[TRACE_ID],
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert!(response.headers().contains_key(REQUEST_ID));
    }
}
