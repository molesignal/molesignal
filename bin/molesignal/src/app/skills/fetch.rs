// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 下载 Skill 仓库的压缩包。
//!
//! 每一跳都按连接的出站策略重新解析、校验，并把连接钉在校验过的地址上；重定向最多跟随几跳，
//! 令牌只发给仓库所在的主机，跳到别的主机（托管站把下载转给存储域名）时不带。下载有大小
//! 上限，超过就停止读取。

use std::time::Duration;

use futures::StreamExt;
use reqwest::{Client, StatusCode, redirect::Policy};
use url::Url;

use crate::{
    agent::skills::ArchiveRequest,
    app::egress::{TargetPolicy, resolve_target},
    shared::{Error, Result},
};

/// 最多跟随几次重定向。
const MAX_REDIRECTS: usize = 4;

const TIMEOUT: Duration = Duration::from_secs(120);

pub(super) async fn download(
    request: &ArchiveRequest,
    token: Option<&str>,
    policy: &TargetPolicy,
    limit: usize,
) -> Result<Vec<u8>> {
    let first = Url::parse(&request.url)
        .map_err(|error| Error::invalid(format!("invalid archive address: {error}")))?;

    let origin_host = first.host_str().map(str::to_ascii_lowercase);
    let mut url = first;

    for _ in 0..=MAX_REDIRECTS {
        let (host, addresses) = resolve_target(&url, policy).await?;

        let client = Client::builder()
            .redirect(Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| Error::internal(format!("repository HTTP client: {error}")))?;

        let mut builder = client
            .get(url.clone())
            .header(
                "accept",
                "application/octet-stream, application/x-gzip, */*",
            )
            .header(
                "user-agent",
                concat!("MoleSignal/", env!("CARGO_PKG_VERSION")),
            );

        if let Some(token) = token.filter(|_| origin_host.as_deref() == Some(host.as_str())) {
            let (name, value) = request.auth(token);
            builder = builder.header(name, value);
        }

        let response = crate::shared::http_trace::send(
            &client,
            builder,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|error| Error::unavailable(format!("repository download failed: {error}")))?;

        let status = response.status();

        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    Error::unavailable("the repository redirected without a location")
                })?;

            url = url
                .join(location)
                .map_err(|error| Error::unavailable(format!("bad redirect: {error}")))?;

            continue;
        }

        if !status.is_success() {
            return Err(status_error(status));
        }

        return read_limited(response, limit).await;
    }

    Err(Error::unavailable(
        "the repository redirected too many times",
    ))
}

fn status_error(status: StatusCode) -> Error {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Error::forbidden(
            "the repository refused the download: the access token is missing or cannot read it",
        ),
        StatusCode::NOT_FOUND => Error::not_found(
            "the repository, branch or tag was not found (a private repository needs an access token)",
        ),
        status => Error::unavailable(format!("the repository answered HTTP {}", status.as_u16())),
    }
}

async fn read_limited(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| Error::unavailable(format!("repository download: {error}")))?;

        if bytes.len() + chunk.len() > limit {
            return Err(Error::payload_too_large(format!(
                "the repository archive is larger than {} MiB",
                limit / 1024 / 1024
            )));
        }

        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_say_what_to_fix() {
        assert!(matches!(
            status_error(StatusCode::FORBIDDEN),
            Error::Forbidden(_)
        ));
        assert!(matches!(
            status_error(StatusCode::NOT_FOUND),
            Error::NotFound(_)
        ));
        assert!(matches!(
            status_error(StatusCode::BAD_GATEWAY),
            Error::Unavailable(_)
        ));
    }
}
