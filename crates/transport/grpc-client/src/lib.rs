// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群 gRPC channel 构造（共用基元）。
//!
//! 把 `advertise_addr`（裸 `host:port` 或带 scheme）解析为 endpoint、挂连接/请求超时、
//! 按需开 webpki TLS，返回 tonic [`Channel`]；并提供给请求挂 bearer 的 helper。
//! 联邦查询扇出、跨集群事件推送、查询取消、gossip 节点发现共用同一套连接逻辑。

use std::time::Duration;

use tonic::transport::{Channel, ClientTlsConfig};

/// 连接超时（与联邦查询一致）。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 请求超时（与联邦查询一致）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 把 `advertise_addr` 解析为 endpoint URL。
/// 已带 scheme 则原样返回；裸地址按 `tls_verify` 选 https / http。
pub fn endpoint_url(advertise_addr: &str, tls_verify: bool) -> String {
    if advertise_addr.starts_with("http://") || advertise_addr.starts_with("https://") {
        advertise_addr.to_string()
    } else if tls_verify {
        format!("https://{advertise_addr}")
    } else {
        format!("http://{advertise_addr}")
    }
}

/// 建一个到远端集群的 tonic [`Channel`]（超时 + 可选 webpki TLS）。
/// 失败返回泛化文案（不含 secret / 内部指针），供调用方记入 degraded / 仅 warn。
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

/// 给一个 tonic 请求挂 `authorization: Bearer <token>` 头。明文 token 绝不入日志。
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
