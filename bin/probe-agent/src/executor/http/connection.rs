// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The connection one request of an HTTP check travels on.

use std::{future::Future, net::SocketAddr, pin::Pin};

use anyhow::{Result, bail};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    Method, Request, Response, Uri, Version,
    body::Incoming,
    client::conn::{http1, http2},
    header::{HOST, HeaderValue},
    http::uri::Authority,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

use super::{
    proxy::Proxy,
    route::{HttpVersion, Transport},
    tls,
};
use crate::executor::{
    failure,
    timing::{Phase, PhaseClock},
};

/// Bytes to and from the other end, over whatever carries them.
pub(super) type Stream = Box<dyn Io>;

pub(super) trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

/// The wait for the head of the response to a request sent on a connection.
type Waiting = Pin<Box<dyn Future<Output = hyper::Result<Response<Incoming>>> + Send>>;

/// A connection ready to carry a request.
pub(super) struct Connection {
    requests: Requests,
    /// Whether the other end is a proxy that passes the request on.
    forwarded: bool,
}

/// How a request goes out: on its own over HTTP/1.1, or as a stream of HTTP/2.
enum Requests {
    Http1(http1::SendRequest<Full<Bytes>>),
    Http2(http2::SendRequest<Full<Bytes>>),
}

impl Connection {
    /// Opens a connection for a request to `uri`: to `address`, or through `proxy` when there
    /// is one.
    ///
    /// The probe opens the socket and runs the TLS handshake itself, so that each is a phase it
    /// can time on its own.
    pub async fn open(
        uri: &Uri,
        address: SocketAddr,
        proxy: Option<&Proxy>,
        transport: &Transport,
        phases: &PhaseClock,
    ) -> Result<Self> {
        // What TLS needs is put together first, so that no local work falls between the phases.
        let secure = if uri.scheme_str() == Some("https") {
            Some((tls::target_connector(transport)?, tls::server_name(uri)?))
        } else {
            None
        };

        // A proxy is told all of where a request goes, unless the request is out of its sight
        // in a tunnel. A proxy that passes plain requests on does so in HTTP/1.1.
        let forwarded = proxy.is_some() && secure.is_none();

        if forwarded && transport.version == HttpVersion::Http2 {
            bail!("HTTP/2 cannot go through a proxy that passes plain HTTP requests on");
        }

        let stream = match proxy {
            Some(proxy) => proxy.open(uri, transport.verify, phases).await?,
            None => reach(address, phases).await?,
        };

        // Over TLS the server picks the protocol among those offered. Over plain HTTP, HTTP/2 is
        // spoken only when the check asks for it.
        let (stream, http2): (Stream, bool) = match secure {
            Some((connector, server_name)) => {
                // The connector starts the handshake as soon as it is called, so the call is
                // timed with it.
                let handshake = async { connector.connect(server_name, stream).await };

                let tls = phases
                    .during(Phase::Tls, handshake)
                    .await
                    .map_err(|error| handshake_failed(&error, transport.version))?;

                let http2 = tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice());

                if transport.version == HttpVersion::Http2 && !http2 {
                    return Err(http2_refused());
                }

                (Box::new(tls), http2)
            }
            None => (stream, transport.version == HttpVersion::Http2),
        };

        // From here the connection runs on its own, until the request lets go of it.
        let requests = if http2 {
            let (requests, connection) =
                http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
                    .await
                    .map_err(|error| {
                        failure::target("http_failed", format!("HTTP/2 handshake failed: {error}"))
                    })?;

            tokio::spawn(connection);
            Requests::Http2(requests)
        } else {
            let (requests, connection) = http1::handshake(TokioIo::new(stream)).await?;
            tokio::spawn(connection.with_upgrades());
            Requests::Http1(requests)
        };

        Ok(Self {
            requests,
            forwarded,
        })
    }

    /// Sends `request`, addressed the way the other end expects.
    pub fn send(&mut self, request: Request<Full<Bytes>>) -> Waiting {
        match &mut self.requests {
            Requests::Http1(requests) => {
                Box::pin(requests.send_request(addressed(request, self.forwarded)))
            }
            Requests::Http2(requests) => Box::pin(requests.send_request(for_http2(request))),
        }
    }
}

/// What a TLS handshake that went wrong says about the server. Asked for HTTP/2 alone, a server
/// that speaks only HTTP/1.1 turns the handshake down, the way it does any protocol it lacks.
fn handshake_failed(error: &std::io::Error, version: HttpVersion) -> anyhow::Error {
    let no_protocol = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<rustls::Error>())
        .is_some_and(|cause| {
            matches!(
                cause,
                rustls::Error::AlertReceived(rustls::AlertDescription::NoApplicationProtocol)
            )
        });

    if no_protocol && version == HttpVersion::Http2 {
        return http2_refused();
    }

    failure::target("tls_failed", format!("TLS handshake failed: {error}"))
}

fn http2_refused() -> anyhow::Error {
    failure::target("http2_refused", "the server did not agree to speak HTTP/2")
}

/// Connects straight to `address`.
async fn reach(address: SocketAddr, phases: &PhaseClock) -> Result<Stream> {
    let tcp = phases
        .during(Phase::Connect, TcpStream::connect(address))
        .await
        .map_err(|error| {
            failure::target(
                "connect_failed",
                format!("cannot connect to {address}: {error}"),
            )
        })?;

    // A request leaves when it is written, not when the kernel has gathered enough to send.
    tcp.set_nodelay(true)?;
    Ok(Box::new(tcp))
}

/// Puts `request` in the form the other end expects: the host it is for in a header, and on the
/// request line as much of the URL as the other end needs.
fn addressed(mut request: Request<Full<Bytes>>, forwarded: bool) -> Request<Full<Bytes>> {
    if !request.headers().contains_key(HOST)
        && let Some(host) = host_of(request.uri())
    {
        request.headers_mut().insert(HOST, host);
    }

    let uri = request.uri();

    // A request for a tunnel names where to. A proxy needs the whole URL to pass a request on.
    // The server the request is for needs only the path.
    let target = if request.method() == Method::CONNECT {
        uri.authority().cloned().map(Uri::from)
    } else if forwarded {
        Some(uri.clone())
    } else {
        uri.path_and_query().cloned().map(Uri::from)
    };

    *request.uri_mut() = target.unwrap_or_default();
    request
}

/// Puts `request` in the form HTTP/2 expects: the whole URL, whose authority goes out as the
/// request's `:authority`. A Host header the check set names that authority instead, and does
/// not go out itself.
fn for_http2(mut request: Request<Full<Bytes>>) -> Request<Full<Bytes>> {
    if let Some(host) = request.headers_mut().remove(HOST)
        && let Some(authority) = host
            .to_str()
            .ok()
            .and_then(|host| host.parse::<Authority>().ok())
    {
        let mut parts = request.uri().clone().into_parts();
        parts.authority = Some(authority);

        if let Ok(uri) = Uri::from_parts(parts) {
            *request.uri_mut() = uri;
        }
    }

    *request.version_mut() = Version::HTTP_2;
    request
}

/// The host a request to `uri` is for. A URL never spells out the default port of its scheme, so
/// a port in the URI is one the server has to be told.
fn host_of(uri: &Uri) -> Option<HeaderValue> {
    let host = uri.host()?;

    let host = match uri.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };

    HeaderValue::try_from(host).ok()
}
