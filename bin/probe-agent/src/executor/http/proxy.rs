// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The proxy an HTTP check goes through when the probe's environment names one.

use std::net::SocketAddr;

use anyhow::{Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    Request, Uri,
    header::{HeaderValue, PROXY_AUTHORIZATION},
};
use hyper_util::client::proxy::matcher::Matcher;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
};

use super::{connection::Stream, tls};
use crate::executor::timing::{Phase, PhaseClock};

/// The longest answer to a request for a tunnel that is read through.
const TUNNEL_REPLY_MAX_BYTES: usize = 8192;

/// How much of an answer shows whether the tunnel was granted: `HTTP/1.1 200`.
const STATUS_BYTES: usize = 12;

/// How much of the status line of a refusal is quoted.
const STATUS_LINE_MAX_BYTES: usize = 80;

/// Which proxy the requests of a check go through, target by target.
pub(super) struct Rules(pub(super) Matcher);

impl Rules {
    /// The rules in the probe's environment: the proxies named in `HTTP_PROXY`, `HTTPS_PROXY`
    /// and `ALL_PROXY`, and the targets `NO_PROXY` exempts from them.
    pub fn from_environment() -> Self {
        Self(Matcher::from_env())
    }

    /// The proxy a request for `target` goes through, if any.
    pub fn proxy_for(&self, target: &Uri) -> Option<Proxy> {
        let chosen = self.0.intercept(target)?;

        Some(Proxy {
            uri: chosen.uri().clone(),
            credentials: chosen.basic_auth().cloned(),
        })
    }
}

/// A proxy between the probe and the target of a request.
pub(super) struct Proxy {
    /// Where the proxy listens.
    uri: Uri,
    /// What the proxy is given to know the probe by.
    credentials: Option<HeaderValue>,
}

impl Proxy {
    /// Adds the credentials for the proxy to a request the proxy is to pass on, unless the
    /// request brings its own.
    ///
    /// A request that travels through a tunnel is not the proxy's to read: there the credentials
    /// go with the request for the tunnel.
    pub fn sign(&self, request: &mut Request<Full<Bytes>>) {
        if request.uri().scheme_str() != Some("http") {
            return;
        }

        if let Some(credentials) = &self.credentials {
            request
                .headers_mut()
                .entry(PROXY_AUTHORIZATION)
                .or_insert_with(|| credentials.clone());
        }
    }

    /// Opens the way to `target`: a connection to the proxy and, for a target reached under TLS,
    /// a tunnel through the proxy to it.
    ///
    /// Each step is timed as the phase it is a kind of: finding the proxy as DNS, the socket as
    /// connecting, a handshake with the proxy itself as TLS. Having the tunnel opened counts as
    /// connecting too: only then does the connection reach the target.
    pub async fn open(
        &self,
        target: &Uri,
        verify_tls: bool,
        phases: &PhaseClock,
    ) -> Result<Stream> {
        let tunnel = target.scheme_str() == Some("https");

        // A proxy may itself be reached under TLS. What that takes is put together before any
        // phase starts, as it is for the target.
        let secure = match self.uri.scheme_str() {
            Some("http") => None,
            Some("https") => {
                let connector = if tunnel {
                    tls::connector_for_tunnel(verify_tls)?
                } else {
                    tls::connector(verify_tls)?
                };

                Some((connector, tls::server_name(&self.uri)?))
            }
            scheme => bail!("unsupported proxy scheme `{}`", scheme.unwrap_or_default()),
        };

        let host = self.uri.host().unwrap_or_default();
        let default_port = if secure.is_some() { 443 } else { 80 };
        let address = format!("{host}:{}", self.uri.port_u16().unwrap_or(default_port));

        let addresses: Vec<SocketAddr> = match address.parse() {
            // An address literal is parsed, not looked up, so it has no DNS phase to time.
            Ok(literal) => vec![literal],
            Err(_) => phases
                .during(Phase::Dns, tokio::net::lookup_host(&address))
                .await
                .map_err(|error| anyhow!("cannot look up proxy {address}: {error}"))?
                .collect(),
        };

        // The addresses are tried in turn, and connecting takes as long as all the tries.
        let tcp = phases
            .during(Phase::Connect, TcpStream::connect(addresses.as_slice()))
            .await
            .map_err(|error| anyhow!("cannot connect to proxy {address}: {error}"))?;

        tcp.set_nodelay(true)?;

        let mut stream: Stream = match secure {
            Some((connector, server_name)) => {
                // The connector starts the handshake as soon as it is called, so the call is
                // timed with it.
                let handshake = async { connector.connect(server_name, tcp).await };

                let tls = phases
                    .during(Phase::Tls, handshake)
                    .await
                    .map_err(|error| anyhow!("TLS handshake with proxy failed: {error}"))?;

                Box::new(tls)
            }
            None => Box::new(tcp),
        };

        if tunnel {
            let opening = self.tunnel(&mut stream, target);
            phases.during(Phase::Connect, opening).await?;
        }

        Ok(stream)
    }

    /// Asks the proxy to connect `stream` through to `target`.
    async fn tunnel(&self, stream: &mut Stream, target: &Uri) -> Result<()> {
        let host = target.host().unwrap_or_default();
        let port = target.port_u16().unwrap_or(443);

        let mut request =
            format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n").into_bytes();

        if let Some(credentials) = &self.credentials {
            request.extend_from_slice(b"Proxy-Authorization: ");
            request.extend_from_slice(credentials.as_bytes());
            request.extend_from_slice(b"\r\n");
        }

        request.extend_from_slice(b"\r\n");
        stream.write_all(&request).await?;
        stream.flush().await?;
        let mut reply = Vec::new();
        let mut chunk = [0_u8; 1024];

        // The tunnel is open once the head of a 200 has ended. Anything else is a refusal, and is
        // not read to its end.
        loop {
            match stream.read(&mut chunk).await? {
                0 => bail!("proxy closed the connection before opening a tunnel"),
                read => reply.extend_from_slice(&chunk[..read]),
            }

            let opened = reply.starts_with(b"HTTP/1.1 200") || reply.starts_with(b"HTTP/1.0 200");

            if opened && reply.ends_with(b"\r\n\r\n") {
                return Ok(());
            }

            if !opened && reply.len() >= STATUS_BYTES {
                let line = reply
                    .split(|byte| *byte == b'\r')
                    .next()
                    .unwrap_or_default();

                let line = &line[..line.len().min(STATUS_LINE_MAX_BYTES)];
                bail!("proxy refused a tunnel: {}", String::from_utf8_lossy(line));
            }

            if reply.len() >= TUNNEL_REPLY_MAX_BYTES {
                bail!("proxy sent too long an answer to the request for a tunnel");
            }
        }
    }
}
