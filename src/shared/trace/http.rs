// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared outbound HTTP boundary.
//!
//! The wrapper deliberately records only method, scheme, host, port, status, and
//! bounded error categories. It never records a URL path, query, request/response
//! body, headers, or credentials.

use reqwest::{Client, Request, RequestBuilder, Response};
use tracing::{Instrument, field};

use crate::shared::{
    ids::Id,
    trace_context::{TraceContext, current_trace_context, with_current_trace_context},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpTarget {
    /// A target whose identity was independently authenticated/configured as part
    /// of this MoleSignal cluster. Only the baggage whitelist is forwarded.
    Internal,
    /// Any SaaS, webhook, identity provider, model provider, or other third party.
    ThirdParty,
}

impl HttpTarget {
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

/// Build, sanitize propagation headers, execute, and instrument one logical HTTP
/// client request. Retry loops should call this once per *logical* operation and
/// represent attempts as bounded events rather than creating unbounded spans.
pub async fn send(
    client: &Client,
    builder: RequestBuilder,
    target: HttpTarget,
) -> Result<Response, reqwest::Error> {
    let (request, context) = prepare(builder, target)?;
    let method = request.method().as_str().to_owned();
    let (scheme, host, port) = sanitized_destination(&request);
    let timeout_ms = request
        .timeout()
        .map(|timeout| timeout.as_millis().min(u64::MAX as u128) as u64);

    let span = tracing::info_span!(
        "http.client",
        otel.name = %format!("HTTP {method}"),
        otel.kind = "client",
        otel.trace_id = %context.trace_id,
        otel.span_id = %context.span_id,
        otel.parent_span_id = context.parent_span_id.as_deref().unwrap_or(""),
        http.request.method = %method,
        url.scheme = %scheme,
        server.address = %host,
        server.port = field::Empty,
        http.response.status_code = field::Empty,
        molesignal.http.target = target.label(),
        molesignal.http.timeout_ms = field::Empty,
        error.type = field::Empty,
    );
    if let Some(port) = port {
        span.record("server.port", port);
    }
    if let Some(timeout_ms) = timeout_ms {
        span.record("molesignal.http.timeout_ms", timeout_ms);
    }

    let instrument_span = span.clone();
    let response = with_current_trace_context(context, client.execute(request))
        .instrument(instrument_span)
        .await;
    match &response {
        Ok(response) => {
            let status = response.status();
            span.record("http.response.status_code", status.as_u16());
            if status.is_client_error() || status.is_server_error() {
                span.record("error.type", format!("http_{}", status.as_u16()));
            }
        }
        Err(error) => {
            span.record("error.type", reqwest_error_type(error));
        }
    }
    response
}

fn sanitized_destination(request: &Request) -> (String, String, Option<u16>) {
    (
        request.url().scheme().to_owned(),
        request.url().host_str().unwrap_or("unknown").to_owned(),
        request.url().port_or_known_default(),
    )
}

fn prepare(
    builder: RequestBuilder,
    target: HttpTarget,
) -> Result<(Request, TraceContext), reqwest::Error> {
    let mut request = builder.build()?;
    let context = current_trace_context()
        .map(|context| context.child())
        .unwrap_or_else(|| TraceContext::new_root(Id::new().0));
    context.inject_http(request.headers_mut(), target.is_internal());
    Ok((request, context))
}

fn reqwest_error_type(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_redirect() {
        "redirect"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
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
    async fn third_party_keeps_trace_context_and_strips_internal_headers() {
        with_current_trace_context(context(), async {
            let client = Client::new();
            let (request, child) = prepare(
                client
                    .get("https://example.com/private?secret=value")
                    .header(BAGGAGE, "user.id=secret")
                    .header(TRACE_FORCE, "true")
                    .header(TRACE_DEBUG_TOKEN, "secret"),
                HttpTarget::ThirdParty,
            )
            .unwrap();

            assert_eq!(child.parent_span_id.as_deref(), Some("b7ad6b7169203331"));
            assert!(request.headers().contains_key(TRACEPARENT));
            assert!(!request.headers().contains_key(BAGGAGE));
            assert!(!request.headers().contains_key(TRACE_FORCE));
            assert!(!request.headers().contains_key(TRACE_DEBUG_TOKEN));
        })
        .await;
    }

    #[tokio::test]
    async fn internal_target_replaces_baggage_with_the_allowlist() {
        with_current_trace_context(context(), async {
            let client = Client::new();
            let (request, _) = prepare(
                client
                    .get("https://internal.invalid/rpc")
                    .header(BAGGAGE, "user.id=secret,api_token.id=secret"),
                HttpTarget::Internal,
            )
            .unwrap();

            assert_eq!(
                request.headers()[BAGGAGE],
                "org.id=org-1,request.id=request-123"
            );
        })
        .await;
    }

    #[test]
    fn hostile_urls_only_expose_scheme_host_and_port_property() {
        let client = Client::new();
        for index in 0..512 {
            let secret = format!("customer-{index:04}@example.net");
            let request = client
                .get(format!(
                    "https://collector.example:8443/private/{secret}/object.parquet?\
                     authorization=Bearer-{index:08}&email={secret}#prompt"
                ))
                .build()
                .expect("generated URL");
            let destination = sanitized_destination(&request);
            assert_eq!(
                destination,
                ("https".into(), "collector.example".into(), Some(8443))
            );
            let encoded = format!("{destination:?}");
            assert!(!encoded.contains(&secret));
            assert!(!encoded.contains("private"));
            assert!(!encoded.contains("authorization"));
            assert!(!encoded.contains("prompt"));
        }
    }
}
