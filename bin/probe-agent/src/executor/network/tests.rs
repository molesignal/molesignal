// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use rcgen::{CertificateParams, DistinguishedName, DnType, DnValue, Issuer, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_rustls::LazyConfigAcceptor;
use tonic::{
    body::Body,
    codegen::{
        Service,
        http::{HeaderMap, Request},
    },
    server::NamedService,
    transport::{Server, server::TcpIncoming},
};
use x509_parser::parse_x509_certificate;

use super::execute_grpc;
use crate::{
    executor::{
        ExecutionContext, execute, security::EgressGuard, timing::PhaseClock,
        trace_context::trace_id_of,
    },
    protocol::v1::{self as wire, ProbeOutcome, grpc_spec, probe_task, value::Source},
};

/// The timeout of a check that has to run into it: short, so that it does so soon.
const BRIEF: Duration = Duration::from_millis(250);

/// The timeout of a check that has to finish: long enough for a busy machine.
const AMPLE: Duration = Duration::from_secs(5);

/// How long a test waits for a check before it calls the check hung.
const HUNG: Duration = Duration::from_secs(10);

/// The types of the HTTP/2 frames a server here writes by hand.
const HEADERS: u8 = 1;
const SETTINGS: u8 = 4;

fn install_crypto_provider() {
    // Every test that needs TLS asks for the same provider, so losing the race to install is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
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

fn task(spec: probe_task::Spec, timeout: Duration) -> wire::ProbeTask {
    wire::ProbeTask {
        task_id: "task".into(),
        timeout_millis: u32::try_from(timeout.as_millis()).expect("milliseconds"),
        max_attempts: 1,
        egress_policy: loopback(),
        spec: Some(spec),
        ..Default::default()
    }
}

/// A health check of the whole server at `endpoint`.
fn grpc(endpoint: String, use_tls: bool) -> wire::GrpcSpec {
    wire::GrpcSpec {
        endpoint,
        use_tls,
        call: Some(grpc_spec::Call::Health(wire::GrpcHealthCall::default())),
        ..Default::default()
    }
}

/// Runs the one attempt of a check, which has to come back by itself.
async fn attempt(spec: probe_task::Spec, timeout: Duration) -> wire::ProbeAttempt {
    let result = tokio::time::timeout(HUNG, execute(task(spec, timeout), 1))
        .await
        .expect("the check never came back")
        .expect("result");

    result.attempts.into_iter().next().expect("attempt")
}

fn at_least(measured: Option<u64>, floor: Duration) -> bool {
    measured.is_some_and(|micros| u128::from(micros) >= floor.as_micros())
}

/// A server that takes connections and never says a word on them.
async fn silent() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");

    tokio::spawn(async move {
        let mut held = Vec::new();

        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });

    address
}

/// A service that hands every call on as it came, having told of the authority the call names
/// and of the headers it came with.
#[derive(Clone)]
struct Overheard<S> {
    service: S,
    named: mpsc::UnboundedSender<String>,
    heard: mpsc::UnboundedSender<HeaderMap>,
}

impl<S: Service<Request<Body>>> Service<Request<Body>> for Overheard<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(context)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let authority = request.uri().authority().map(ToString::to_string);
        let _ = self.named.send(authority.unwrap_or_default());
        let _ = self.heard.send(request.headers().clone());
        self.service.call(request)
    }
}

impl<S: NamedService> NamedService for Overheard<S> {
    const NAME: &'static str = S::NAME;
}

/// A gRPC server at `ip` whose health service reports it is serving, and the authority each call
/// to it names.
async fn serving(ip: IpAddr) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let (address, naming, _) = overheard(ip).await;
    (address, naming)
}

/// A serving gRPC server at `ip`, and the authority and the headers of each call to it.
async fn overheard(
    ip: IpAddr,
) -> (
    SocketAddr,
    mpsc::UnboundedReceiver<String>,
    mpsc::UnboundedReceiver<HeaderMap>,
) {
    let listener = TcpListener::bind((ip, 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let (_reporter, service) = tonic_health::server::health_reporter();
    let (named, naming) = mpsc::unbounded_channel();
    let (heard, hearing) = mpsc::unbounded_channel();

    tokio::spawn(
        Server::builder()
            .add_service(Overheard {
                service,
                named,
                heard,
            })
            .serve_with_incoming(TcpIncoming::from(listener)),
    );

    (address, naming, hearing)
}

/// A gRPC server that starts its reply to the first call and never finishes it.
async fn trailing_off() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");

        // An empty SETTINGS frame is all there is to the server's side of the HTTP/2 preface.
        let settings = [0, 0, 0, SETTINGS, 0, 0, 0, 0, 0];
        stream.write_all(&settings).await.expect("settings");
        let mut preface = [0_u8; 24];
        stream.read_exact(&mut preface).await.expect("preface");

        // The client's frames go by, header and payload, until the one that opens its call.
        loop {
            let mut header = [0_u8; 9];
            stream.read_exact(&mut header).await.expect("frame header");
            let length = u32::from_be_bytes([0, header[0], header[1], header[2]]);
            let mut payload = vec![0_u8; length as usize];
            stream.read_exact(&mut payload).await.expect("frame");

            if header[3] == HEADERS {
                break;
            }
        }

        // The reply opens on the call's stream, 1: a HEADERS frame of one byte, the index of
        // `:status: 200`, flagged END_HEADERS and not END_STREAM. So a body is due. None comes.
        let head = [0, 0, 1, HEADERS, 4, 0, 0, 0, 1, 0x88];
        stream.write_all(&head).await.expect("response head");
        std::future::pending::<()>().await;
    });

    address
}

/// Where a check looks for `localhost`: the first of its addresses the egress policy lets by.
async fn localhost() -> IpAddr {
    let guard = EgressGuard::new(loopback()).expect("guard");

    let addresses = guard
        .resolve("localhost", 1, &PhaseClock::default())
        .await
        .expect("localhost");

    addresses[0].ip()
}

/// A certificate for `localhost` that names a public root as its issuer, under a signature that
/// root never made.
fn forged() -> Arc<rustls::ServerConfig> {
    let printable = |text: &str| DnValue::PrintableString(text.try_into().expect("printable"));
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, printable("US"));
    name.push(DnType::OrganizationName, printable("Amazon"));
    name.push(DnType::CommonName, printable("Amazon Root CA 3"));
    let mut root = CertificateParams::new(Vec::new()).expect("root");
    root.distinguished_name = name;
    let root = Issuer::new(root, KeyPair::generate().expect("root key"));

    let key = KeyPair::generate().expect("key");

    let certificate = CertificateParams::new(vec!["localhost".to_string()])
        .expect("certificate")
        .signed_by(&key, &root)
        .expect("signature");

    let (_, parsed) = parse_x509_certificate(certificate.der()).expect("parse");

    // A name this short sits behind two bytes of tag and length, which a trust anchor leaves out.
    let issuer = &parsed.issuer().as_raw()[2..];
    assert!(
        webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .any(|trusted| trusted.subject.as_ref() == issuer),
        "no public root goes by the forged issuer's name any more"
    );

    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], key)
        .expect("server config");

    Arc::new(config)
}

/// A TLS server at `ip` that presents the forged certificate, and the host name its one client
/// asks for.
async fn impostor(ip: IpAddr) -> (SocketAddr, oneshot::Receiver<Option<String>>) {
    let config = forged();
    let listener = TcpListener::bind((ip, 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let (asked_for, asking) = oneshot::channel();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let acceptor = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream);
        let hello = acceptor.await.expect("client hello");
        let name = hello.client_hello().server_name().map(str::to_owned);
        let _ = asked_for.send(name);

        // The client is the one to end this handshake, by turning the certificate down.
        let _ = hello.into_stream(config).await;
    });

    (address, asking)
}

#[tokio::test]
async fn tcp_gives_up_on_a_tls_handshake_left_unanswered() {
    install_crypto_provider();
    let address = silent().await;

    let spec = probe_task::Spec::Tcp(wire::TcpSpec {
        host: Some(literal(address.ip().to_string())),
        port: u32::from(address.port()),
        use_tls: true,
        ..Default::default()
    });

    let attempt = attempt(spec, BRIEF).await;
    let timing = attempt.timing.expect("timing");

    assert_eq!(attempt.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(attempt.error_category, "timeout");
    assert_eq!(attempt.error_message, "TLS handshake timed out");
    assert!(timing.connect_micros.is_some());
    assert!(at_least(timing.tls_micros, BRIEF));
    assert_eq!(timing.first_byte_micros, None);
}

#[tokio::test]
async fn tls_gives_up_on_a_handshake_left_unanswered() {
    install_crypto_provider();
    let address = silent().await;

    let spec = probe_task::Spec::Tls(wire::TlsSpec {
        host: address.ip().to_string(),
        port: u32::from(address.port()),
        ..Default::default()
    });

    let attempt = attempt(spec, BRIEF).await;
    let timing = attempt.timing.expect("timing");

    assert_eq!(attempt.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(attempt.error_message, "TLS handshake timed out");
    assert!(timing.connect_micros.is_some());
    assert!(at_least(timing.tls_micros, BRIEF));
}

#[tokio::test]
async fn grpc_gives_up_on_a_tls_handshake_left_unanswered() {
    install_crypto_provider();
    let address = silent().await;
    let spec = grpc(format!("https://{address}"), true);
    let attempt = attempt(probe_task::Spec::Grpc(spec), BRIEF).await;

    assert_eq!(attempt.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(attempt.error_message, "gRPC check timed out");
}

#[tokio::test]
async fn grpc_gives_up_on_a_reply_left_unfinished() {
    let address = trailing_off().await;
    let spec = grpc(format!("http://{address}"), false);
    let attempt = attempt(probe_task::Spec::Grpc(spec), BRIEF).await;

    assert_eq!(attempt.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(attempt.error_message, "gRPC check timed out");
}

#[tokio::test]
async fn grpc_finds_a_serving_service_healthy() {
    let (address, _) = serving(Ipv4Addr::LOCALHOST.into()).await;
    let spec = grpc(format!("http://{address}"), false);
    let attempt = attempt(probe_task::Spec::Grpc(spec), AMPLE).await;

    assert_eq!(attempt.outcome, ProbeOutcome::Healthy as i32);
    assert_eq!(attempt.error_message, "");
}

/// The check dials the address the egress policy let by, and still has to call the server by the
/// endpoint's own name: a gateway that routes by host sends a call for an address astray.
#[tokio::test]
async fn grpc_names_the_endpoint_to_the_server_it_dials() {
    let (address, mut naming) = serving(localhost().await).await;
    let authority = format!("localhost:{}", address.port());
    let spec = grpc(format!("http://{authority}"), false);
    let attempt = attempt(probe_task::Spec::Grpc(spec), AMPLE).await;

    assert_eq!(attempt.outcome, ProbeOutcome::Healthy as i32);
    assert_eq!(naming.try_recv().ok(), Some(authority));
}

/// The check editor asks for an endpoint as a host and a port, with no scheme before them.
#[tokio::test]
async fn grpc_takes_an_endpoint_of_a_host_and_a_port() {
    let (address, mut naming) = serving(localhost().await).await;

    for endpoint in [format!("localhost:{}", address.port()), address.to_string()] {
        let spec = grpc(endpoint.clone(), false);
        let attempt = attempt(probe_task::Spec::Grpc(spec), AMPLE).await;

        assert_eq!(attempt.error_message, "", "{endpoint}");
        assert_eq!(attempt.outcome, ProbeOutcome::Healthy as i32, "{endpoint}");
        assert_eq!(naming.try_recv().ok(), Some(endpoint));
    }
}

/// A call carries the check's metadata, and a span of the trace the attempt names.
#[tokio::test]
async fn grpc_sends_its_metadata_and_a_trace_context() {
    let (address, _naming, mut hearing) = overheard(localhost().await).await;
    let mut spec = grpc(format!("localhost:{}", address.port()), false);

    spec.metadata = vec![wire::HeaderValue {
        name: "X-Tenant".into(),
        value: Some(literal("acme")),
    }];

    let task = wire::ProbeTask {
        inject_trace_context: true,
        ..task(probe_task::Spec::Grpc(spec), AMPLE)
    };

    let result = tokio::time::timeout(HUNG, execute(task, 1))
        .await
        .expect("the check never came back")
        .expect("result");

    let attempt = &result.attempts[0];
    assert_eq!(
        attempt.outcome,
        ProbeOutcome::Healthy as i32,
        "{}",
        attempt.error_message
    );

    let headers = hearing.try_recv().expect("the call's headers");
    let traceparent = headers["traceparent"].to_str().expect("ASCII");

    assert_eq!(headers["x-tenant"], "acme");
    assert_eq!(
        trace_id_of(traceparent).as_ref(),
        attempt.metadata.get("trace_id")
    );
    assert_eq!(
        result.metadata.get("trace_id"),
        attempt.metadata.get("trace_id")
    );
}

/// Whether a check goes over TLS is for its TLS setting to say. An endpoint needs no scheme for it.
#[tokio::test]
async fn grpc_secures_a_host_and_a_port_when_told_to() {
    install_crypto_provider();
    let (address, asking) = impostor(localhost().await).await;
    let spec = grpc(format!("localhost:{}", address.port()), true);
    let task = task(probe_task::Spec::Grpc(spec.clone()), AMPLE);

    let refused = tokio::time::timeout(
        HUNG,
        execute_grpc(
            &task,
            &spec,
            &ExecutionContext::new(&task),
            &PhaseClock::default(),
        ),
    )
    .await
    .expect("the check never came back")
    .err()
    .expect("a forged certificate is turned down");

    let cause = format!("{refused:?}");

    // Only a TLS handshake gets as far as the certificate, and it asks for the endpoint's host.
    assert!(cause.contains("BadSignature"), "{cause}");
    let asked_for = asking.await.expect("client hello");
    assert_eq!(asked_for.as_deref(), Some("localhost"));
}

/// An endpoint may carry a scheme. When the scheme and the TLS setting say different things, the
/// check sides with neither, least of all by dropping TLS without a word: it says what is wrong.
#[tokio::test]
async fn grpc_refuses_a_scheme_at_odds_with_the_tls_setting() {
    let (address, mut naming) = serving(localhost().await).await;

    for (scheme, use_tls) in [("http", true), ("https", false)] {
        let spec = grpc(format!("{scheme}://localhost:{}", address.port()), use_tls);
        let attempt = attempt(probe_task::Spec::Grpc(spec), AMPLE).await;

        assert_eq!(attempt.outcome, ProbeOutcome::Unknown as i32, "{scheme}");
        assert!(
            attempt.error_message.contains(&format!("{scheme}://")),
            "{scheme}: {}",
            attempt.error_message
        );
        assert!(naming.try_recv().is_err(), "{scheme}: a call went out");
    }
}

/// The server's certificate names a public root as its issuer without that root's signature. A
/// check that knows the public roots finds the one named and sees the signature is not its own;
/// one that knows no root cannot tell who the issuer is.
#[tokio::test]
async fn grpc_holds_a_certificate_to_the_public_roots() {
    install_crypto_provider();
    let (address, asking) = impostor(localhost().await).await;
    let spec = grpc(format!("https://localhost:{}", address.port()), true);
    let task = task(probe_task::Spec::Grpc(spec.clone()), AMPLE);

    let refused = tokio::time::timeout(
        HUNG,
        execute_grpc(
            &task,
            &spec,
            &ExecutionContext::new(&task),
            &PhaseClock::default(),
        ),
    )
    .await
    .expect("the check never came back")
    .err()
    .expect("a forged certificate is turned down");

    let asked_for = asking.await.expect("client hello");
    let cause = format!("{refused:?}");

    // The endpoint is dialled by address, and still asked for by its own name.
    assert_eq!(asked_for.as_deref(), Some("localhost"));
    assert!(cause.contains("BadSignature"), "{cause}");
}

/// A server that answers whatever it is sent with `PONG`.
async fn ponging() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut asked = [0u8; 64];
                let _ = stream.read(&mut asked).await;
                let _ = stream.write_all(b"PONG").await;
            });
        }
    });

    address
}

/// An address nothing listens on, so that a connection to it is refused.
async fn closed() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    listener.local_addr().expect("local address")
}

/// A DNS server that knows an A and an MX record of `example.test`, and no other name.
async fn naming_server() -> SocketAddr {
    use hickory_resolver::proto::{
        op::{Message, ResponseCode},
        rr::{
            Name, RData, Record, RecordType,
            rdata::{A, MX},
        },
    };

    let socket = tokio::net::UdpSocket::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let address = socket.local_addr().expect("local address");

    tokio::spawn(async move {
        let mut buffer = [0u8; 512];

        while let Ok((length, peer)) = socket.recv_from(&mut buffer).await {
            let Ok(query) = Message::from_vec(&buffer[..length]) else {
                continue;
            };

            let Some(question) = query.queries.first().cloned() else {
                continue;
            };

            let mut response = Message::response(query.metadata.id, query.metadata.op_code);
            response.metadata.recursion_desired = query.metadata.recursion_desired;
            response.metadata.recursion_available = true;

            let name = question.name().clone();
            let known = name
                .to_utf8()
                .trim_end_matches('.')
                .eq_ignore_ascii_case("example.test");

            match question.query_type() {
                RecordType::A if known => {
                    let address = RData::A(A(Ipv4Addr::new(192, 0, 2, 10)));
                    response.add_answer(Record::from_rdata(name, 60, address));
                }
                RecordType::MX if known => {
                    let exchange = Name::from_ascii("mail.example.test.").expect("name");
                    let mx = RData::MX(MX::new(10, exchange));
                    response.add_answer(Record::from_rdata(name, 60, mx));
                }
                _ if known => {}
                _ => response.metadata.response_code = ResponseCode::NXDomain,
            }

            response.add_query(question);

            if let Ok(bytes) = response.to_vec() {
                let _ = socket.send_to(&bytes, peer).await;
            }
        }
    });

    address
}

fn assertion(source: &str, operator: &str, expected: &str, critical: bool) -> wire::Assertion {
    wire::Assertion {
        id: format!("{source}-{operator}"),
        name: format!("{source} {operator}"),
        severity: if critical {
            wire::AssertionSeverity::Critical as i32
        } else {
            wire::AssertionSeverity::Warning as i32
        },
        source: source.into(),
        operator: operator.into(),
        expected: Some(literal(expected)),
    }
}

fn dns(resolver: SocketAddr, name: &str, record_type: &str) -> wire::DnsSpec {
    wire::DnsSpec {
        name: name.into(),
        record_type: record_type.into(),
        resolver: Some(resolver.to_string()),
        ..Default::default()
    }
}

/// A target that turns the connection down is the outage a check is there to catch.
#[tokio::test]
async fn a_refused_connection_fails_the_check() {
    let address = closed().await;

    let spec = probe_task::Spec::Tcp(wire::TcpSpec {
        host: Some(literal(address.ip().to_string())),
        port: u32::from(address.port()),
        ..Default::default()
    });

    let attempt = attempt(spec, AMPLE).await;

    assert_eq!(attempt.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(attempt.error_category, "connect_failed");
}

#[tokio::test]
async fn tcp_judges_the_reply_by_its_assertions() {
    let address = ponging().await;

    let checked = |assertions| {
        probe_task::Spec::Tcp(wire::TcpSpec {
            host: Some(literal(address.ip().to_string())),
            port: u32::from(address.port()),
            send: Some(literal("PING")),
            assertions,
            ..Default::default()
        })
    };

    let answered = attempt(
        checked(vec![
            assertion("response", "equals", "PONG", true),
            assertion("duration_ms", "less_than", "60000", true),
        ]),
        AMPLE,
    )
    .await;

    assert_eq!(answered.outcome, ProbeOutcome::Healthy as i32);
    assert!(answered.metadata.contains_key("remote_address"));

    let unexpected = attempt(
        checked(vec![assertion("response", "contains", "PANG", false)]),
        AMPLE,
    )
    .await;

    assert_eq!(unexpected.outcome, ProbeOutcome::Degraded as i32);
}

#[tokio::test]
async fn grpc_judges_the_serving_status_by_its_assertions() {
    let (address, _) = serving(Ipv4Addr::LOCALHOST.into()).await;

    let mut spec = grpc(address.to_string(), false);
    spec.assertions = vec![
        assertion("status", "equals", "SERVING", true),
        assertion("duration_ms", "less_than", "0", false),
    ];

    let attempt = attempt(probe_task::Spec::Grpc(spec), AMPLE).await;

    // Serving, and no call takes less than no time: the warning assertion degrades the check.
    assert_eq!(attempt.outcome, ProbeOutcome::Degraded as i32);
    assert_eq!(
        attempt.metadata.get("health_status").map(String::as_str),
        Some("SERVING")
    );
}

#[tokio::test]
async fn dns_asks_the_resolver_it_names_for_any_record() {
    let resolver = naming_server().await;

    let mut mx = dns(resolver, "example.test", "MX");
    mx.expected_values = vec!["mail.example.test".into()];
    mx.assertions = vec![assertion("rcode", "equals", "NOERROR", true)];

    let attempt = attempt(probe_task::Spec::Dns(mx), AMPLE).await;

    assert_eq!(
        attempt.outcome,
        ProbeOutcome::Healthy as i32,
        "{}",
        attempt.error_message
    );
    assert_eq!(attempt.metadata["answers"], "10 mail.example.test.");
    assert_eq!(attempt.metadata["resolver"], resolver.to_string());
}

#[tokio::test]
async fn dns_can_expect_a_name_not_to_exist() {
    let resolver = naming_server().await;

    let mut gone = dns(resolver, "gone.example.test", "A");
    let missing = attempt(probe_task::Spec::Dns(gone.clone()), AMPLE).await;

    assert_eq!(missing.outcome, ProbeOutcome::Failing as i32);
    assert_eq!(missing.error_category, "dns_rcode_mismatch");

    gone.expected_rcode = Some("NXDOMAIN".into());
    let expected = attempt(probe_task::Spec::Dns(gone), AMPLE).await;

    assert_eq!(
        expected.outcome,
        ProbeOutcome::Healthy as i32,
        "{}",
        expected.error_message
    );
    assert_eq!(expected.metadata["rcode"], "NXDOMAIN");
}

/// Pings go out only where the process may send them; elsewhere the test has nothing to show.
#[tokio::test]
async fn icmp_counts_the_pings_that_come_back() {
    if !super::icmp::available() {
        eprintln!("skipped: this process may not send pings");
        return;
    }

    let spec = probe_task::Spec::Icmp(wire::IcmpSpec {
        host: "127.0.0.1".into(),
        count: 3,
        interval_millis: 20,
        max_packet_loss_ratio: 0.5,
        max_mean_rtt_millis: None,
    });

    let attempt = attempt(spec, AMPLE).await;

    assert_eq!(
        attempt.outcome,
        ProbeOutcome::Healthy as i32,
        "{}",
        attempt.error_message
    );
    assert_eq!(attempt.metadata["sent"], "3");
    assert!(attempt.metadata.contains_key("rtt_mean_ms"));
}
