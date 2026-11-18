// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cross-cluster gRPC channel construction (shared primitive).
//!
//! Resolves `advertise_addr` (a bare `host:port` or one with a scheme) into an endpoint,
//! attaches connect/request timeouts, enables webpki TLS on demand, and returns a tonic
//! [`Channel`]; it also provides a helper for attaching a bearer to a request.
//! Federated query fan-out, cross-cluster event push, query cancellation, and gossip node
//! discovery all share this one set of connection logic.

use std::time::Duration;

use tonic::transport::{Channel, ClientTlsConfig};

/// Connect timeout (consistent with federated queries).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Request timeout (consistent with federated queries).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Resolves `advertise_addr` into an endpoint URL.
/// Returns it unchanged if it already has a scheme; a bare address gets https / http
/// depending on `tls_verify`.
pub fn endpoint_url(advertise_addr: &str, tls_verify: bool) -> String {
    if advertise_addr.starts_with("http://") || advertise_addr.starts_with("https://") {
        advertise_addr.to_string()
    } else if tls_verify {
        format!("https://{advertise_addr}")
    } else {
        format!("http://{advertise_addr}")
    }
}

/// Builds a tonic [`Channel`] to a remote cluster (timeouts + optional webpki TLS).
/// On failure returns a generic message (no secrets or internal pointers), so the caller can
/// record it under degraded or merely warn.
pub async fn connect(advertise_addr: &str, tls_verify: bool) -> Result<Channel, String> {
    let url = endpoint_url(advertise_addr, tls_verify);

    let mut endpoint = Channel::from_shared(url.clone())
        .map_err(|e| format!("invalid endpoint: {e}"))?
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT);

    if url.starts_with("https://") {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new().with_webpki_roots())
            .map_err(|e| format!("tls config: {e}"))?;
    }

    endpoint
        .connect()
        .await
        .map_err(|e| format!("connect: {e}"))
}

/// Attaches an `authorization: Bearer <token>` header to a tonic request.
/// The plaintext token never goes into logs.
pub fn with_bearer<T>(req: &mut tonic::Request<T>, token: &str) -> Result<(), String> {
    let bearer = format!("Bearer {token}");

    req.metadata_mut().insert(
        "authorization",
        bearer
            .parse()
            .map_err(|_| "invalid bearer token bytes".to_string())?,
    );

    Ok(())
}
