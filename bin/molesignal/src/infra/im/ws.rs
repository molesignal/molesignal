// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 连上一个 WebSocket 地址：`wss://` 自己建 TLS（rustls，信任操作系统信任的证书），`ws://`
//! 只给测试用的本机服务器。

use std::{sync::Arc, time::Duration};

use rustls::{
    ClientConfig,
    crypto::{CryptoProvider, aws_lc_rs},
    pki_types::ServerName,
};
use rustls_platform_verifier::Verifier;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::client::IntoClientRequest};
use url::Url;

use crate::shared::{Error, Result};

/// 连接与握手最多等这么久。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub(crate) type Socket = WebSocketStream<Box<dyn Io>>;

fn tls() -> Result<TlsConnector> {
    let provider = CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(aws_lc_rs::default_provider()));

    let verifier = Verifier::new(provider.clone())
        .map_err(|error| Error::internal(format!("cannot load the system trust store: {error}")))?;

    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::internal(format!("TLS client: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();

    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsConnector::from(Arc::new(config)))
}

/// 连上 `url`，完成 WebSocket 握手。
pub(crate) async fn connect(url: &str) -> Result<Socket> {
    tokio::time::timeout(CONNECT_TIMEOUT, open(url))
        .await
        .map_err(|_| Error::unavailable("the WebSocket connection timed out"))?
}

async fn open(url: &str) -> Result<Socket> {
    let parsed = Url::parse(url).map_err(|_| Error::invalid("invalid WebSocket address"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::invalid("the WebSocket address has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    let secure = match parsed.scheme() {
        "wss" => true,
        "ws" => false,
        _ => {
            return Err(Error::invalid(
                "a WebSocket address starts with wss:// or ws://",
            ));
        }
    };

    let port = parsed
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });

    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|error| Error::unavailable(format!("cannot reach {host}:{port}: {error}")))?;
    let _ = tcp.set_nodelay(true);

    let stream: Box<dyn Io> = if secure {
        let name = ServerName::try_from(host.clone())
            .map_err(|_| Error::invalid("invalid TLS server name"))?;

        Box::new(
            tls()?
                .connect(name, tcp)
                .await
                .map_err(|error| Error::unavailable(format!("TLS with {host} failed: {error}")))?,
        )
    } else {
        Box::new(tcp)
    };

    let request = url
        .into_client_request()
        .map_err(|error| Error::invalid(format!("invalid WebSocket request: {error}")))?;

    let (socket, _) = client_async(request, stream)
        .await
        .map_err(|error| Error::unavailable(format!("the WebSocket handshake failed: {error}")))?;

    Ok(socket)
}
