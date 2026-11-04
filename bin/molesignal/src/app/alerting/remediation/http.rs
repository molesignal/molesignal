// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! HTTP 修复动作：填好模板发一个请求，请求体是 JSON。只连地址解析出的 IP（不能是云厂商元数据地址），
//! 不跟随跳转；设了签名密钥时带 `X-MoleSignal-Timestamp` 与 `X-MoleSignal-Sign`，同 Webhook。

use std::{collections::BTreeMap, time::Duration};

use reqwest::{
    Method,
    header::{CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT},
};

use crate::{
    domain::alerting::remediation::{Escape, HttpRemediation, HttpVerb, render_template},
    infra::connectors::dispatcher::http::{pinned_client, signature},
    shared::{Error, Result, time::TimestampMicros},
};

const TIMEOUT: Duration = Duration::from_secs(30);

/// 响应留多少字在执行结果里。
const EXCERPT_CHARS: usize = 500;

/// 发出请求，返回「HTTP 状态码：响应开头」；不是 2xx 时报错。
pub async fn call(http: &HttpRemediation, vars: &BTreeMap<String, String>) -> Result<String> {
    let address = render_template(&http.url, vars, Escape::Url);
    let url = url::Url::parse(&address).map_err(|error| {
        Error::invalid(format!("the address `{address}` is not valid: {error}"))
    })?;

    let body = render_template(&http.body, vars, Escape::Json);
    let client = pinned_client(&url, TIMEOUT).await?;

    let method = match http.method {
        HttpVerb::Get => Method::GET,
        HttpVerb::Post => Method::POST,
        HttpVerb::Put => Method::PUT,
        HttpVerb::Patch => Method::PATCH,
    };

    let mut request = client.request(method, url).header(USER_AGENT, "MoleSignal");

    for (name, value) in &http.headers {
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| Error::invalid(format!("`{name}` is not a header name")))?;
        let value = HeaderValue::from_str(&render_template(value, vars, Escape::None))
            .map_err(|_| Error::invalid(format!("the value of `{name}` is not a header value")))?;

        request = request.header(name, value);
    }

    if !body.trim().is_empty() {
        request = request.header(CONTENT_TYPE, "application/json");
    }

    if let Some(secret) = http
        .signing_secret
        .as_deref()
        .filter(|secret| !secret.is_empty())
    {
        let timestamp = (TimestampMicros::now().0 / 1_000_000).to_string();

        request = request.header("X-MoleSignal-Timestamp", &timestamp).header(
            "X-MoleSignal-Sign",
            signature(secret, &timestamp, body.as_bytes()),
        );
    }

    if !body.trim().is_empty() {
        request = request.body(body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("the request could not be sent: {error}")))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let excerpt = text.trim().chars().take(EXCERPT_CHARS).collect::<String>();
    let outcome = format!("HTTP {}: {excerpt}", status.as_u16());

    if status.is_success() {
        Ok(outcome)
    } else {
        Err(Error::unavailable(outcome))
    }
}
