// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One request of an HTTP check, and the response to it.

use std::{future::Future, net::SocketAddr, time::Duration};

use anyhow::{Context as _, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::{
    HeaderMap, Method, Request, StatusCode, Uri,
    body::Incoming,
    header::{ACCEPT, AUTHORIZATION, HeaderValue},
};
use percent_encoding::percent_decode_str;
use tokio::time::Instant;
use tower::{ServiceExt as _, service_fn};
use tower_http::decompression::{Decompression, DecompressionBody};
use url::Url;

use super::{connection::Connection, proxy};
use crate::executor::timing::{Phase, PhaseClock};

/// Starts a request for `url`.
///
/// A user name and password in the URL do not go out in it: they become Basic authorization.
pub(super) fn request(method: Method, url: &Url) -> Result<Request<Full<Bytes>>> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("HTTP URL must use http or https");
    }

    let mut url = url.clone();
    let authorization = take_credentials(&mut url)?;

    let uri: Uri = url
        .as_str()
        .parse()
        .context("HTTP URL is not a valid URI")?;

    let mut request = Request::new(Full::default());
    *request.method_mut() = method;
    *request.uri_mut() = uri;

    if let Some(authorization) = authorization {
        request.headers_mut().append(AUTHORIZATION, authorization);
    }

    Ok(request)
}

/// Sends `request` over a new connection, and returns once the head of the response is in.
///
/// The connection goes to `address`, unless `proxies` send the request through a proxy.
/// `allowed` is all the time there is, from opening the connection to the last of the body.
pub(super) async fn send(
    mut request: Request<Full<Bytes>>,
    address: SocketAddr,
    proxies: &proxy::Rules,
    verify_tls: bool,
    allowed: Duration,
    phases: &PhaseClock,
) -> Result<Reply> {
    let limit = Limit::starting(allowed);
    let uri = request.uri().clone();
    let proxy = proxies.proxy_for(&uri);

    request
        .headers_mut()
        .entry(ACCEPT)
        .or_insert(HeaderValue::from_static("*/*"));

    if let Some(proxy) = &proxy {
        proxy.sign(&mut request);
    }

    let head = async {
        let proxy = proxy.as_ref();
        let mut connection = Connection::open(&uri, address, proxy, verify_tls, phases).await?;

        // A request that names no encodings of its own asks for gzip, and a gzip response is
        // unpacked whoever asked for it.
        let exchange = Decompression::new(service_fn(|request| connection.send(request)))
            .no_deflate()
            .no_br()
            .no_zstd()
            .gzip(true);

        // The wait for the reply starts with the connection ready, and ends with the head of the
        // response or with the request failing.
        let waiting = phases.during(Phase::FirstByte, exchange.oneshot(request));
        let response = waiting.await?;
        anyhow::Ok((connection, response))
    };

    let (connection, response) = limit
        .run(head)
        .await
        .map_err(|error| anyhow!("error sending request for url ({uri}): {error:#}"))?;

    let (head, body) = response.into_parts();

    Ok(Reply {
        status: head.status,
        headers: head.headers,
        body,
        limit,
        _connection: connection,
    })
}

/// A response whose head has arrived and whose body is still to be read.
pub(super) struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    body: DecompressionBody<Incoming>,
    limit: Limit,
    // The connection stays open for as long as there is a body to read from it.
    _connection: Connection,
}

impl Reply {
    /// Reads the body, unpacked if it came as gzip, and stops after `most` bytes.
    pub async fn read_body(&mut self, most: usize) -> Result<Vec<u8>> {
        let limit = self.limit;
        let mut body = Vec::new();

        let reading = async {
            while body.len() < most {
                let Some(frame) = self.body.frame().await else {
                    break;
                };

                let frame = frame.map_err(anyhow::Error::from_boxed)?;

                if let Some(chunk) = frame.data_ref() {
                    let room = most - body.len();
                    body.extend_from_slice(&chunk[..chunk.len().min(room)]);
                }
            }

            anyhow::Ok(())
        };

        limit
            .run(reading)
            .await
            .map_err(|error| anyhow!("error decoding response body: {error:#}"))?;

        Ok(body)
    }
}

/// The time a request has, from opening its connection to the last of its response body.
#[derive(Clone, Copy)]
struct Limit {
    allowed: Duration,
    ends: Instant,
}

impl Limit {
    fn starting(allowed: Duration) -> Self {
        Self {
            allowed,
            ends: Instant::now() + allowed,
        }
    }

    /// Runs `work` for as long as there is time left.
    async fn run<T>(self, work: impl Future<Output = Result<T>>) -> Result<T> {
        match tokio::time::timeout_at(self.ends, work).await {
            Ok(done) => done,
            Err(_) => bail!("timed out after {} ms", self.allowed.as_millis()),
        }
    }
}

/// Takes the user name and password out of `url`, as the Basic authorization they stand for.
fn take_credentials(url: &mut Url) -> Result<Option<HeaderValue>> {
    let decoded = |part: &str| {
        percent_decode_str(part)
            .decode_utf8()
            .ok()
            .map(String::from)
    };

    let Some(username) = decoded(url.username()) else {
        return Ok(None);
    };

    let password = url.password().and_then(decoded);

    if username.is_empty() && password.is_none() {
        return Ok(None);
    }

    // Neither can fail for a URL that has a host, as every http and https URL does.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let credentials = format!("{username}:{}", password.unwrap_or_default());
    let basic = format!("Basic {}", STANDARD.encode(credentials));
    let mut authorization = HeaderValue::try_from(basic)?;
    authorization.set_sensitive(true);
    Ok(Some(authorization))
}

#[cfg(test)]
mod tests;
