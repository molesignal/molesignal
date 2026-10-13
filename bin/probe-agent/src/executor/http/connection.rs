// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The connection one request of an HTTP check travels on.

use std::{future::Future, net::SocketAddr};

use anyhow::{Result, anyhow};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    Method, Request, Response, Uri,
    body::Incoming,
    client::conn::http1::{self, SendRequest},
    header::{HOST, HeaderValue},
};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

use super::{proxy::Proxy, tls};
use crate::executor::timing::{Phase, PhaseClock};

/// Bytes to and from the other end, over whatever carries them.
pub(super) type Stream = Box<dyn Io>;

pub(super) trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

/// A connection ready to carry a request.
pub(super) struct Connection {
    requests: SendRequest<Full<Bytes>>,
    /// Whether the other end is a proxy that passes the request on.
    forwarded: bool,
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
        verify_tls: bool,
        phases: &PhaseClock,
    ) -> Result<Self> {
        // What TLS needs is put together first, so that no local work falls between the phases.
        let secure = if uri.scheme_str() == Some("https") {
            Some((tls::connector(verify_tls)?, tls::server_name(uri)?))
        } else {
            None
        };

        // A proxy is told all of where a request goes, unless the request is out of its sight
        // in a tunnel.
        let forwarded = proxy.is_some() && secure.is_none();

        let stream = match proxy {
            Some(proxy) => proxy.open(uri, verify_tls, phases).await?,
            None => reach(address, phases).await?,
        };

        let stream: Stream = match secure {
            Some((connector, server_name)) => {
                // The connector starts the handshake as soon as it is called, so the call is
                // timed with it.
                let handshake = async { connector.connect(server_name, stream).await };

                let tls = phases
                    .during(Phase::Tls, handshake)
                    .await
                    .map_err(|error| anyhow!("TLS handshake failed: {error}"))?;

                Box::new(tls)
            }
            None => stream,
        };

        // From here the connection runs on its own, until the request lets go of it.
        let (requests, connection) = http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(connection.with_upgrades());

        Ok(Self {
            requests,
            forwarded,
        })
    }

    /// Sends `request`, addressed the way the other end expects.
    pub fn send(
        &mut self,
        request: Request<Full<Bytes>>,
    ) -> impl Future<Output = hyper::Result<Response<Incoming>>> + use<> {
        self.requests
            .send_request(addressed(request, self.forwarded))
    }
}

/// Connects straight to `address`.
async fn reach(address: SocketAddr, phases: &PhaseClock) -> Result<Stream> {
    let tcp = phases
        .during(Phase::Connect, TcpStream::connect(address))
        .await
        .map_err(|error| anyhow!("cannot connect to {address}: {error}"))?;

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
