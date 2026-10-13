// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{net::SocketAddr, sync::Arc, time::Duration};

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

use super::{AwaitingReply, Phase, PhaseClock};
use crate::{
    executor::{execute, security::EgressGuard},
    protocol::v1::{self as wire, ProbeOutcome, TimingBreakdown, probe_task, value::Source},
};

/// How long a test server holds a reply back, so the wait for it can be told from noise.
const PAUSE: Duration = Duration::from_millis(60);

const OK: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";
const REDIRECT: &[u8] =
    b"HTTP/1.1 302 Found\r\nlocation: /next\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
const UNAVAILABLE: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

/// A 200 whose body is `unpacked`, sent as gzip.
const PACKED: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-encoding: gzip\r\ncontent-length: 28\r\n\
    connection: close\r\n\r\n\
    \x1f\x8b\x08\x00\x00\x00\x00\x00\x02\xff\x2b\xcd\x2b\x48\x4c\xce\x4e\x4d\x01\x00\
    \x96\x62\x2f\xca\x08\x00\x00\x00";

/// What a test server sends on one connection, and when.
struct Reply {
    /// The end of the request to wait for. Empty waits for the first bytes of anything.
    after: &'static [u8],
    pause: Duration,
    bytes: &'static [u8],
}

fn http_reply(pause: Duration, bytes: &'static [u8]) -> Reply {
    Reply {
        after: b"\r\n\r\n",
        pause,
        bytes,
    }
}

/// Serves one reply per connection, in order, and returns the address to reach it at.
async fn serve(replies: Vec<Reply>, tls: Option<TlsAcceptor>) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");

    tokio::spawn(async move {
        for reply in replies {
            let (stream, _) = listener.accept().await.expect("accept");

            let Some(tls) = &tls else {
                answer(stream, reply).await;
                continue;
            };

            // A client that turns the certificate down ends the connection, not the server.
            if let Ok(stream) = tls.accept(stream).await {
                answer(stream, reply).await;
            }
        }
    });

    address
}

async fn answer(mut stream: impl AsyncRead + AsyncWrite + Unpin, reply: Reply) {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];

    while request.is_empty() || !request.ends_with(reply.after) {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&chunk[..read]),
        }
    }

    tokio::time::sleep(reply.pause).await;
    let _ = stream.write_all(reply.bytes).await;
    let _ = stream.shutdown().await;
}

fn install_crypto_provider() {
    // Every test that needs TLS asks for the same provider, so losing the race to install is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn self_signed_tls() -> TlsAcceptor {
    install_crypto_provider();
    let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("issue");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der()));

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![issued.cert.der().clone()], key)
        .expect("server config");

    TlsAcceptor::from(Arc::new(config))
}

fn literal(value: impl Into<String>) -> wire::Value {
    wire::Value {
        source: Some(Source::Literal(value.into())),
    }
}

fn loopback() -> Option<wire::EgressPolicy> {
    Some(wire::EgressPolicy {
        allow_loopback: true,
        ..Default::default()
    })
}

fn task(spec: probe_task::Spec, max_attempts: u32) -> wire::ProbeTask {
    wire::ProbeTask {
        task_id: "task".into(),
        timeout_millis: 5_000,
        max_attempts,
        egress_policy: loopback(),
        spec: Some(spec),
        ..Default::default()
    }
}

/// A one-step HTTP check that expects a 200 from `url`, whatever certificate it is served with.
fn http(url: String) -> probe_task::Spec {
    http_check(url, expecting("status", "200"), false)
}

/// A one-step HTTP check of `url` with one thing to assert.
fn http_check(url: String, assertion: wire::Assertion, verify_tls: bool) -> probe_task::Spec {
    probe_task::Spec::Http(wire::HttpJourneySpec {
        steps: vec![wire::HttpStep {
            id: "step".into(),
            name: "Step".into(),
            method: "GET".into(),
            url: Some(literal(url)),
            assertions: vec![assertion],
            ..Default::default()
        }],
        follow_redirects: true,
        max_redirects: 3,
        verify_tls,
    })
}

fn expecting(source: &str, expected: &str) -> wire::Assertion {
    wire::Assertion {
        id: source.into(),
        name: source.into(),
        severity: wire::AssertionSeverity::Critical as i32,
        source: source.into(),
        operator: "equals".into(),
        expected: Some(literal(expected)),
    }
}

/// A TCP check that sends `PING` and expects `PONG` back.
fn tcp(address: SocketAddr, use_tls: bool) -> probe_task::Spec {
    probe_task::Spec::Tcp(wire::TcpSpec {
        host: Some(literal(address.ip().to_string())),
        port: u32::from(address.port()),
        use_tls,
        send: Some(literal("PING")),
        expect_regex: Some("PONG".into()),
        ..Default::default()
    })
}

fn timing(result: &wire::ProbeResult, attempt: usize) -> &TimingBreakdown {
    result.attempts[attempt].timing.as_ref().expect("timing")
}

fn at_least(measured: Option<u64>, floor: Duration) -> bool {
    measured.is_some_and(|micros| u128::from(micros) >= floor.as_micros())
}

fn phases_within_total(timing: &TimingBreakdown) -> bool {
    let phases = [
        timing.dns_micros,
        timing.connect_micros,
        timing.tls_micros,
        timing.first_byte_micros,
    ];

    phases.into_iter().flatten().sum::<u64>() <= timing.total_micros.expect("total")
}

#[tokio::test]
async fn a_phase_adds_up_over_connections() {
    let phases = PhaseClock::default();
    let pause = || tokio::time::sleep(PAUSE);
    phases.during(Phase::Connect, pause()).await;
    phases.during(Phase::Connect, pause()).await;
    let timing = phases.breakdown(Duration::from_secs(1));

    assert!(at_least(timing.connect_micros, PAUSE * 2));
    assert_eq!(timing.dns_micros, None);
    assert_eq!(timing.tls_micros, None);
    assert_eq!(timing.first_byte_micros, None);
    assert_eq!(timing.total_micros, Some(1_000_000));
}

#[tokio::test]
async fn a_phase_cut_short_still_counts() {
    let phases = PhaseClock::default();
    let stalled = phases.during(Phase::Tls, std::future::pending::<()>());

    assert!(tokio::time::timeout(PAUSE, stalled).await.is_err());
    assert!(at_least(
        phases.breakdown(Duration::ZERO).tls_micros,
        PAUSE / 2
    ));
}

#[tokio::test]
async fn the_wait_ends_with_the_first_byte() {
    let phases = PhaseClock::default();
    let (near, mut far) = tokio::io::duplex(64);
    let mut near = AwaitingReply::new(near, &phases);

    tokio::spawn(async move {
        tokio::time::sleep(PAUSE).await;
        far.write_all(b"reply").await.expect("write");
    });

    let mut first = [0_u8; 1];
    near.read_exact(&mut first).await.expect("first byte");
    let waited = phases.breakdown(Duration::ZERO).first_byte_micros;
    tokio::time::sleep(PAUSE).await;
    drop(near);

    assert!(at_least(waited, PAUSE));
    assert_eq!(phases.breakdown(Duration::ZERO).first_byte_micros, waited);
}

#[tokio::test]
async fn only_a_host_name_has_a_dns_phase() {
    let guard = EgressGuard::new(loopback()).expect("guard");
    let named = PhaseClock::default();
    let literal = PhaseClock::default();

    // The lookup is timed whether or not this machine knows the name.
    let _ = guard.resolve("localhost", 80, &named).await;
    guard
        .resolve("127.0.0.1", 80, &literal)
        .await
        .expect("address");

    assert!(named.breakdown(Duration::ZERO).dns_micros.is_some());
    assert_eq!(literal.breakdown(Duration::ZERO).dns_micros, None);
}

#[tokio::test]
async fn http_reports_connect_and_first_byte() {
    let address = serve(vec![http_reply(PAUSE, OK)], None).await;

    let result = execute(task(http(format!("http://{address}/")), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
    assert_eq!(timing.dns_micros, None);
    assert!(timing.connect_micros.is_some());
    assert_eq!(timing.tls_micros, None);
    assert!(at_least(timing.first_byte_micros, PAUSE));
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn http_phases_add_up_over_redirects() {
    let replies = vec![http_reply(PAUSE, REDIRECT), http_reply(PAUSE, OK)];
    let address = serve(replies, None).await;

    let result = execute(task(http(format!("http://{address}/")), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
    assert!(timing.connect_micros.is_some());
    assert!(at_least(timing.first_byte_micros, PAUSE * 2));
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn https_reports_connect_and_tls() {
    let address = serve(vec![http_reply(PAUSE, OK)], Some(self_signed_tls())).await;

    let result = execute(task(http(format!("https://{address}/")), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
    assert!(timing.connect_micros.is_some());
    assert!(timing.tls_micros.is_some());
    assert!(at_least(timing.first_byte_micros, PAUSE));
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn a_failed_https_handshake_still_counts() {
    install_crypto_provider();

    let reply = Reply {
        after: b"",
        pause: PAUSE,
        bytes: b"not a TLS record\r\n",
    };

    let address = serve(vec![reply], None).await;

    let result = execute(task(http(format!("https://{address}/")), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Unknown as i32);
    assert!(timing.connect_micros.is_some());
    assert!(at_least(timing.tls_micros, PAUSE));
    assert_eq!(timing.first_byte_micros, None);
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn only_a_check_that_verifies_minds_an_untrusted_certificate() {
    let replies = vec![
        http_reply(Duration::ZERO, OK),
        http_reply(Duration::ZERO, OK),
    ];

    let address = serve(replies, Some(self_signed_tls())).await;
    let url = format!("https://{address}/");
    let verifying = http_check(url.clone(), expecting("status", "200"), true);
    let trusting = http_check(url, expecting("status", "200"), false);
    let refused = execute(task(verifying, 1), 1).await.expect("result");
    let served = execute(task(trusting, 1), 2).await.expect("result");

    assert_eq!(refused.outcome, ProbeOutcome::Unknown as i32);
    assert!(refused.attempts[0].error_message.contains("certificate"));
    assert_eq!(served.outcome, ProbeOutcome::Healthy as i32);
}

#[tokio::test]
async fn http_unpacks_a_gzip_body() {
    let address = serve(vec![http_reply(Duration::ZERO, PACKED)], None).await;
    let check = http_check(
        format!("http://{address}/"),
        expecting("body", "unpacked"),
        false,
    );

    let result = execute(task(check, 1), 1).await.expect("result");

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
}

#[tokio::test]
async fn http_gives_up_when_its_time_runs_out() {
    let address = serve(vec![http_reply(PAUSE * 10, OK)], None).await;

    let hurried = wire::ProbeTask {
        timeout_millis: 2 * PAUSE.as_millis() as u32,
        ..task(http(format!("http://{address}/")), 1)
    };

    let result = execute(hurried, 1).await.expect("result");
    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Unknown as i32);
    assert!(result.attempts[0].error_message.contains("timed out"));
    assert!(at_least(timing.first_byte_micros, PAUSE));
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn each_attempt_is_timed_on_its_own() {
    let replies = vec![
        http_reply(PAUSE * 5, UNAVAILABLE),
        http_reply(Duration::ZERO, OK),
    ];

    let address = serve(replies, None).await;

    let result = execute(task(http(format!("http://{address}/")), 2), 1)
        .await
        .expect("result");

    assert_eq!(result.outcome, ProbeOutcome::Flaky as i32);
    assert_eq!(result.attempts.len(), 2);
    let (failed, recovered) = (timing(&result, 0), timing(&result, 1));

    assert!(at_least(failed.first_byte_micros, PAUSE * 5));
    assert!(recovered.first_byte_micros.is_some());
    assert!(recovered.first_byte_micros < failed.first_byte_micros);
    assert!(recovered.total_micros < failed.total_micros);
    assert!(phases_within_total(failed));
    assert!(phases_within_total(recovered));
}

#[tokio::test]
async fn tcp_reports_connect_and_first_byte() {
    let reply = Reply {
        after: b"PING",
        pause: PAUSE,
        bytes: b"PONG",
    };

    let address = serve(vec![reply], None).await;

    let result = execute(task(tcp(address, false), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
    assert!(timing.connect_micros.is_some());
    assert_eq!(timing.tls_micros, None);
    assert!(at_least(timing.first_byte_micros, PAUSE));
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn a_failed_tls_handshake_still_counts() {
    install_crypto_provider();

    let reply = Reply {
        after: b"",
        pause: PAUSE,
        bytes: b"not a TLS record\r\n",
    };

    let address = serve(vec![reply], None).await;

    let result = execute(task(tcp(address, true), 1), 1)
        .await
        .expect("result");

    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Unknown as i32);
    assert!(timing.connect_micros.is_some());
    assert!(at_least(timing.tls_micros, PAUSE));
    assert_eq!(timing.first_byte_micros, None);
    assert!(phases_within_total(timing));
}

#[tokio::test]
async fn ssh_reports_connect_and_first_byte() {
    let reply = Reply {
        after: b"\r\n",
        pause: PAUSE,
        bytes: b"SSH-2.0-Test\r\n",
    };

    let address = serve(vec![reply], None).await;

    let spec = probe_task::Spec::Ssh(wire::SshSpec {
        host: Some(literal(address.ip().to_string())),
        port: u32::from(address.port()),
        ..Default::default()
    });

    let result = execute(task(spec, 1), 1).await.expect("result");
    let timing = timing(&result, 0);

    assert_eq!(result.outcome, ProbeOutcome::Healthy as i32);
    assert!(timing.connect_micros.is_some());
    assert_eq!(timing.tls_micros, None);
    assert!(at_least(timing.first_byte_micros, PAUSE));
    assert!(phases_within_total(timing));
}
