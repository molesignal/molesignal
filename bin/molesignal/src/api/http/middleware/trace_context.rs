// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use http::{HeaderName, HeaderValue, StatusCode};
use tracing::{Instrument, field};

use crate::shared::trace_context::{
    REQUEST_ID, TRACE_ID, TraceContext, TraceTrust, with_current_trace_context,
};

const PROBE_PATHS: &[&str] = &["/api/v1/healthz", "/healthz", "/readyz"];

/// 提取 W3C 上下文、创建低基数 HTTP server Span，并为所有响应回写关联 ID。
pub async fn trace_context_layer(mut request: Request, next: Next) -> Response {
    let context = TraceContext::extract_http(request.headers(), TraceTrust::External);
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched")
        .to_string();
    let method = request.method().clone();
    request.extensions_mut().insert(context.clone());

    let mut response = if PROBE_PATHS.contains(&request.uri().path()) {
        with_current_trace_context(context.clone(), next.run(request)).await
    } else {
        let span = tracing::info_span!(
            "http.server",
            otel.name = %format!("{} {}", method, route),
            otel.kind = "server",
            otel.trace_id = %context.trace_id,
            otel.span_id = %context.span_id,
            otel.parent_span_id = context.parent_span_id.as_deref().unwrap_or(""),
            http.request.method = %method,
            http.route = %route,
            http.response.status_code = field::Empty,
            request.id = %context.request_id,
            molesignal.org.id = field::Empty,
            molesignal.user.id = field::Empty,
            error.type = field::Empty,
        );
        let response = with_current_trace_context(context.clone(), next.run(request))
            .instrument(span.clone())
            .await;
        let status = response.status();
        span.record("http.response.status_code", status.as_u16());
        if is_server_error(status) {
            span.record(
                "error.type",
                status.canonical_reason().unwrap_or("server_error"),
            );
        }
        response
    };

    insert_response_header(&mut response, REQUEST_ID, &context.request_id);
    insert_response_header(&mut response, TRACE_ID, &context.trace_id);
    response
}

fn insert_response_header(response: &mut Response, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(name), value);
    }
}

/// 服务端 5xx/error 置错，普通 4xx 仅记录 HTTP 状态。
pub fn is_server_error(status: StatusCode) -> bool {
    status.is_server_error()
}

#[cfg(test)]
mod tests {
    use axum::{Router, body::Body, routing::get};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn echoes_valid_request_and_trace_ids() {
        let app = Router::new()
            .route("/ok", get(|| async { StatusCode::NO_CONTENT }))
            .layer(axum::middleware::from_fn(trace_context_layer));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header(REQUEST_ID, "request-123")
                    .header(
                        "traceparent",
                        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[REQUEST_ID], "request-123");
        assert_eq!(
            response.headers()[TRACE_ID],
            "0af7651916cd43dd8448eb211c80319c"
        );
    }

    #[tokio::test]
    async fn malformed_ids_are_replaced_without_rejecting_request() {
        let app = Router::new()
            .route("/ok", get(|| async { StatusCode::NO_CONTENT }))
            .layer(axum::middleware::from_fn(trace_context_layer));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header(REQUEST_ID, "contains space")
                    .header("traceparent", "malformed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(response.headers()[REQUEST_ID], "contains space");
        assert_eq!(response.headers()[TRACE_ID].as_bytes().len(), 32);
    }

    #[test]
    fn ordinary_client_errors_do_not_mark_server_span_failed() {
        assert!(!is_server_error(StatusCode::BAD_REQUEST));
        assert!(is_server_error(StatusCode::INTERNAL_SERVER_ERROR));
    }
}
