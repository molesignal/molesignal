// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{net::SocketAddr, sync::Arc, time::Duration};

use hyper::Method;
use hyper_util::client::proxy::matcher::Matcher;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::oneshot,
};
use tokio_rustls::TlsAcceptor;
use url::Url;

use super::{Reply, request, send};
use crate::executor::{http::proxy::Rules, timing::PhaseClock};

/// How long a test proxy holds its answer back, so the wait for it can be told from noise.
const PAUSE: Duration = Duration::from_millis(60);
const ALLOWED: Duration = Duration::from_secs(5);

const OK: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";
const TUNNEL_OPEN: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\n";
const TUNNEL_REFUSED: &[u8] =
    b"HTTP/1.1 407 Proxy Authentication Required\r\ncontent-length: 0\r\n\r\n";

/// Reads from `stream` to the end of the head of a request, and returns the head.
async fn head(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut seen = Vec::new();
    let mut chunk = [0_u8; 1024];

    while !seen.ends_with(b"\r\n\r\n") {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => seen.extend_from_slice(&chunk[..read]),
        }
    }

    String::from_utf8_lossy(&seen).into_owned()
}

/// A proxy that answers the request it is handed itself, and reports what it was handed.
async fn forwarding_proxy() -> (SocketAddr, oneshot::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let (report, handed) = oneshot::channel();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let _ = report.send(head(&mut stream).await);
        let _ = stream.write_all(OK).await;
        let _ = stream.shutdown().await;
    });

    (address, handed)
}

/// A proxy that takes its time over a request for a tunnel, gives `answer`, and then plays the
/// target at the far end of the tunnel itself. It reports the request for the tunnel.
async fn tunnelling_proxy(answer: &'static [u8]) -> (SocketAddr, oneshot::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let (report, asked) = oneshot::channel();
    let target = self_signed_tls();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let _ = report.send(head(&mut stream).await);
        tokio::time::sleep(PAUSE).await;
        let _ = stream.write_all(answer).await;

        // A client that was refused its tunnel has nothing more to say.
        if let Ok(mut stream) = target.accept(stream).await {
            head(&mut stream).await;
            let _ = stream.write_all(OK).await;
            let _ = stream.shutdown().await;
        }
    });

    (address, asked)
}

fn self_signed_tls() -> TlsAcceptor {
    // Every test that needs TLS asks for the same provider, so losing the race to install is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("issue");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der()));

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![issued.cert.der().clone()], key)
        .expect("server config");

    TlsAcceptor::from(Arc::new(config))
}

/// An address nothing listens at: a request that went there would fail at once.
async fn nowhere() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    listener.local_addr().expect("local address")
}

/// Sends a GET for `url` by way of the proxy at `proxy`, which knows the probe as `probe:secret`.
async fn get_through(proxy: String, url: &str, phases: &PhaseClock) -> anyhow::Result<Reply> {
    let request = request(Method::GET, &Url::parse(url)?)?;
    let everything = Matcher::builder().all(format!("http://probe:secret@{proxy}"));
    let proxies = Rules(everything.build());
    let address = nowhere().await;

    send(request, address, &proxies, false, ALLOWED, phases).await
}

fn at_least(measured: Option<u64>, floor: Duration) -> bool {
    measured.is_some_and(|micros| u128::from(micros) >= floor.as_micros())
}

#[tokio::test]
async fn a_proxy_is_handed_a_plain_request_whole() {
    let (proxy, handed) = forwarding_proxy().await;
    let phases = PhaseClock::default();

    let mut reply = get_through(proxy.to_string(), "http://target.example/path?x=1", &phases)
        .await
        .expect("reply");

    let body = reply.read_body(64).await.expect("body");
    let handed = handed.await.expect("request");
    let timing = phases.breakdown(Duration::ZERO);

    assert_eq!(reply.status, 200);
    assert_eq!(body, b"ok");
    assert!(handed.starts_with("GET http://target.example/path?x=1 HTTP/1.1\r\n"));
    assert!(handed.contains("\r\nhost: target.example\r\n"));
    assert!(handed.contains("\r\nproxy-authorization: Basic cHJvYmU6c2VjcmV0\r\n"));

    // A proxy given by address is not looked up, and one passing a request on is not a TLS peer.
    assert_eq!(timing.dns_micros, None);
    assert!(timing.connect_micros.is_some());
    assert_eq!(timing.tls_micros, None);
    assert!(timing.first_byte_micros.is_some());
}

#[tokio::test]
async fn a_secure_request_goes_through_a_tunnel() {
    let (proxy, asked) = tunnelling_proxy(TUNNEL_OPEN).await;
    let by_name = format!("localhost:{}", proxy.port());
    let phases = PhaseClock::default();

    let mut reply = get_through(by_name, "https://target.example/secure", &phases)
        .await
        .expect("reply");

    let body = reply.read_body(64).await.expect("body");
    let timing = phases.breakdown(Duration::ZERO);

    assert_eq!(
        asked.await.expect("request"),
        "CONNECT target.example:443 HTTP/1.1\r\nHost: target.example:443\r\n\
         Proxy-Authorization: Basic cHJvYmU6c2VjcmV0\r\n\r\n"
    );

    assert_eq!(reply.status, 200);
    assert_eq!(body, b"ok");

    // Finding a proxy given by name is a lookup, getting its answer to the request for a tunnel
    // is part of connecting, and the handshake is the one with the target inside the tunnel.
    assert!(timing.dns_micros.is_some());
    assert!(at_least(timing.connect_micros, PAUSE));
    assert!(timing.tls_micros.is_some());
    assert!(timing.first_byte_micros.is_some());
}

#[tokio::test]
async fn a_refused_tunnel_fails_the_request() {
    let (proxy, _asked) = tunnelling_proxy(TUNNEL_REFUSED).await;
    let phases = PhaseClock::default();
    let refused = get_through(proxy.to_string(), "https://target.example/secure", &phases).await;
    let failure = refused.err().expect("failure").to_string();
    let timing = phases.breakdown(Duration::ZERO);

    assert!(
        failure.ends_with("proxy refused a tunnel: HTTP/1.1 407 Proxy Authentication Required")
    );
    assert!(at_least(timing.connect_micros, PAUSE));
    assert_eq!(timing.tls_micros, None);
    assert_eq!(timing.first_byte_micros, None);
}
