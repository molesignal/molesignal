// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! HTTP versions, host bindings and client certificates, against servers started here.

use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
};
use rustls::{
    RootCertStore, ServerConfig,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    server::WebPkiClientVerifier,
};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::{
    executor::execute,
    protocol::v1::{self as wire, ProbeOutcome, probe_task, value::Source},
};

const H2: &[u8] = b"h2";
const HTTP1: &[u8] = b"http/1.1";

fn install_crypto_provider() {
    // Every test that needs TLS asks for the same provider, so losing the race to install is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn literal(value: impl Into<String>) -> wire::Value {
    wire::Value {
        source: Some(Source::Literal(value.into())),
    }
}

/// A TLS server for `localhost` that offers `alpn`, and asks for a client certificate issued
/// under `clients` when it is given one.
fn server_config(alpn: &[&[u8]], clients: Option<RootCertStore>) -> ServerConfig {
    install_crypto_provider();
    let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("issue");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der()));
    let builder = ServerConfig::builder();

    let builder = match clients {
        Some(roots) => builder.with_client_cert_verifier(
            WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .expect("client verifier"),
        ),
        None => builder.with_no_client_auth(),
    };

    let mut config = builder
        .with_single_cert(vec![issued.cert.der().clone()], key)
        .expect("server config");

    config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
    config
}

/// The HTTP version and the host a request was asked in.
fn said(request: &Request<Incoming>) -> String {
    let host = request
        .uri()
        .authority()
        .map(ToString::to_string)
        .or_else(|| {
            request
                .headers()
                .get("host")
                .and_then(|host| host.to_str().ok())
                .map(str::to_string)
        })
        .unwrap_or_default();

    format!("{:?} {host}", request.version())
}

/// A server that answers every request with what `answer` makes of it. Over TLS it speaks what
/// ALPN settled on; without TLS it speaks HTTP/2 by prior knowledge.
async fn serve(
    config: Option<ServerConfig>,
    answer: fn(&Request<Incoming>) -> String,
) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let acceptor = config.map(|config| TlsAcceptor::from(Arc::new(config)));

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();

            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| async move {
                    let body = Bytes::from(answer(&request));
                    Ok::<_, Infallible>(Response::new(Full::new(body)))
                });

                let Some(acceptor) = acceptor else {
                    let http2 = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
                    let _ = http2.serve_connection(TokioIo::new(stream), service).await;
                    return;
                };

                // A client that turns the handshake down ends the connection, not the server.
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };

                if tls.get_ref().1.alpn_protocol() == Some(H2) {
                    let http2 = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
                    let _ = http2.serve_connection(TokioIo::new(tls), service).await;
                } else {
                    let http1 = hyper::server::conn::http1::Builder::new();
                    let _ = http1.serve_connection(TokioIo::new(tls), service).await;
                }
            });
        }
    });

    address
}

/// A one-step check of `url` whose body has to contain `said`, changed by `configure`.
fn check(
    url: String,
    said: &str,
    configure: impl FnOnce(&mut wire::HttpJourneySpec),
) -> probe_task::Spec {
    asserting(url, "body", "contains", said, configure)
}

/// A one-step check of `url` with one assertion, changed by `configure`.
fn asserting(
    url: String,
    source: &str,
    operator: &str,
    expected: &str,
    configure: impl FnOnce(&mut wire::HttpJourneySpec),
) -> probe_task::Spec {
    let mut spec = wire::HttpJourneySpec {
        steps: vec![wire::HttpStep {
            id: "step".into(),
            name: "request".into(),
            method: "GET".into(),
            url: Some(literal(url)),
            assertions: vec![wire::Assertion {
                id: "assertion".into(),
                name: "assertion".into(),
                severity: wire::AssertionSeverity::Critical as i32,
                source: source.into(),
                operator: operator.into(),
                expected: Some(literal(expected)),
            }],
            ..Default::default()
        }],
        follow_redirects: true,
        max_redirects: 5,
        ..Default::default()
    };

    configure(&mut spec);
    probe_task::Spec::Http(spec)
}

async fn run(spec: probe_task::Spec) -> wire::ProbeResult {
    let task = wire::ProbeTask {
        task_id: "task".into(),
        timeout_millis: 5_000,
        max_attempts: 1,
        inject_trace_context: true,
        egress_policy: Some(wire::EgressPolicy {
            allow_loopback: true,
            ..Default::default()
        }),
        spec: Some(spec),
        ..Default::default()
    };

    execute(task, 1).await.expect("result")
}

fn step_metadata<'a>(result: &'a wire::ProbeResult, key: &str) -> Option<&'a str> {
    result.attempts[0].evidence[0]
        .metadata
        .get(key)
        .map(String::as_str)
}

#[tokio::test]
async fn speaks_http2_when_the_server_picks_it() {
    let address = serve(Some(server_config(&[H2, HTTP1], None)), said).await;
    let url = format!("https://localhost:{}/", address.port());
    let result = run(check(url, "HTTP/2.0", |_| {})).await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.attempts[0]
    );
    assert_eq!(step_metadata(&result, "http_version"), Some("HTTP/2"));
}

#[tokio::test]
async fn speaks_http1_to_a_server_that_offers_nothing_else() {
    let address = serve(Some(server_config(&[HTTP1], None)), said).await;
    let url = format!("https://localhost:{}/", address.port());
    let result = run(check(url, "HTTP/1.1", |_| {})).await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.attempts[0]
    );
    assert_eq!(step_metadata(&result, "http_version"), Some("HTTP/1.1"));
}

#[tokio::test]
async fn fails_when_http2_is_required_and_the_server_will_not() {
    let address = serve(Some(server_config(&[HTTP1], None)), said).await;
    let url = format!("https://localhost:{}/", address.port());

    let result = run(check(url, "HTTP/2.0", |spec| {
        spec.http_version = wire::HttpVersion::Http2 as i32;
    }))
    .await;

    assert_eq!(result.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(result.attempts[0].error_category, "http2_refused");
}

#[tokio::test]
async fn speaks_http2_over_plain_http_by_prior_knowledge() {
    let address = serve(None, said).await;
    let url = format!("http://localhost:{}/", address.port());

    let result = run(check(url, "HTTP/2.0", |spec| {
        spec.http_version = wire::HttpVersion::Http2 as i32;
    }))
    .await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.attempts[0]
    );
}

/// A bound host is reached at its address and still asked for by its own name.
#[tokio::test]
async fn reaches_a_bound_host_at_the_address_it_is_bound_to() {
    let address = serve(Some(server_config(&[HTTP1], None)), said).await;
    let url = format!("https://origin.example.test:{}/", address.port());

    let result = run(check(url, "origin.example.test", |spec| {
        spec.hosts = vec![wire::HostBinding {
            name: "origin.example.test".into(),
            addresses: vec!["127.0.0.1".into()],
        }];
    }))
    .await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.attempts[0]
    );
    assert_eq!(
        step_metadata(&result, "remote_address"),
        Some(address.to_string().as_str())
    );
}

#[tokio::test]
async fn shows_its_certificate_to_a_server_that_asks_for_one() {
    install_crypto_provider();

    let mut authority = CertificateParams::new(Vec::new()).expect("authority");
    authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    authority
        .distinguished_name
        .push(DnType::CommonName, "client authority");
    let authority_key = KeyPair::generate().expect("authority key");
    let authority_certificate = authority.self_signed(&authority_key).expect("authority");
    let issuer = Issuer::new(authority, authority_key);

    let mut client = CertificateParams::new(Vec::new()).expect("client");
    client.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    client.distinguished_name.push(DnType::CommonName, "probe");
    let client_key = KeyPair::generate().expect("client key");
    let client_certificate = client.signed_by(&client_key, &issuer).expect("client");

    let mut roots = RootCertStore::empty();
    roots
        .add(authority_certificate.der().clone())
        .expect("trust the authority");

    let address = serve(Some(server_config(&[HTTP1], Some(roots))), said).await;
    let url = format!("https://localhost:{}/", address.port());

    let shown = run(check(url.clone(), "HTTP/1.1", |spec| {
        spec.client_certificate = Some(wire::ClientCertificate {
            certificate: Some(literal(client_certificate.pem())),
            private_key: Some(literal(client_key.serialize_pem())),
        });
    }))
    .await;

    assert_eq!(
        shown.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        shown.attempts[0]
    );

    let withheld = run(check(url, "HTTP/1.1", |_| {})).await;

    assert_eq!(withheld.outcome, ProbeOutcome::Failing as i32);
}

/// JSON is read from far more of the body than the excerpt a result keeps.
#[tokio::test]
async fn reads_json_beyond_the_excerpt() {
    fn large(_: &Request<Incoming>) -> String {
        format!(
            r#"{{"padding":"{}","status":"up"}}"#,
            "x".repeat(100 * 1024)
        )
    }

    let address = serve(Some(server_config(&[HTTP1], None)), large).await;
    let url = format!("https://localhost:{}/", address.port());
    let result = run(asserting(url, "json:$.status", "equals", "up", |_| {})).await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.assertions
    );
}

/// The traceparent a request came with.
fn traceparent(request: &Request<Incoming>) -> String {
    request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none")
        .to_string()
}

/// A request is a span of the attempt's trace, which the step and the run name.
#[tokio::test]
async fn sends_a_span_of_the_attempts_trace() {
    let address = serve(Some(server_config(&[HTTP1], None)), traceparent).await;
    let url = format!("https://localhost:{}/", address.port());

    // A failed assertion keeps the body: the traceparent the server got.
    let result = run(asserting(url, "body", "equals", "anything else", |_| {})).await;
    let trace_id = step_metadata(&result, "trace_id").expect("trace id");
    let span_id = step_metadata(&result, "span_id").expect("span id");
    let excerpt = &result.attempts[0].bounded_response_excerpt;

    assert_eq!(
        String::from_utf8_lossy(excerpt),
        format!("00-{trace_id}-{span_id}-01")
    );

    assert_eq!(
        result.metadata.get("trace_id").map(String::as_str),
        Some(trace_id)
    );
}

/// A step that sends its own traceparent keeps it, and it names the step's trace.
#[tokio::test]
async fn keeps_a_traceparent_the_step_sends() {
    let own = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let address = serve(Some(server_config(&[HTTP1], None)), traceparent).await;
    let url = format!("https://localhost:{}/", address.port());

    let result = run(asserting(url, "body", "equals", own, |spec| {
        spec.steps[0].headers.push(wire::HeaderValue {
            name: "Traceparent".into(),
            value: Some(literal(own)),
        });
    }))
    .await;

    assert_eq!(
        result.outcome,
        ProbeOutcome::Healthy as i32,
        "{:?}",
        result.assertions
    );

    assert_eq!(
        step_metadata(&result, "trace_id"),
        Some("4bf92f3577b34da6a3ce929d0e0e4736")
    );

    assert_eq!(step_metadata(&result, "span_id"), None);
}

/// A run that cannot reach its target traces the route to the address it tried, as a step of
/// its last attempt.
#[tokio::test]
async fn traces_the_route_to_a_target_it_cannot_reach() {
    // A port nothing listens on any more.
    let closed = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("address");

    let task = wire::ProbeTask {
        task_id: "task".into(),
        timeout_millis: 5_000,
        max_attempts: 1,
        traceroute_on_failure: true,
        egress_policy: Some(wire::EgressPolicy {
            allow_loopback: true,
            ..Default::default()
        }),
        spec: Some(check(format!("http://{closed}/"), "anything", |_| {})),
        ..Default::default()
    };

    let result = execute(task, 1).await.expect("result");
    let attempt = &result.attempts[0];
    let traced = attempt.evidence.last().expect("evidence");

    assert_eq!(attempt.error_category, "connect_failed");
    assert_eq!(traced.action, "traceroute");
    assert_eq!(
        traced.metadata.get("target").map(String::as_str),
        Some("127.0.0.1")
    );
    assert!(attempt.finished_at_micros >= traced.finished_at_micros);
}
