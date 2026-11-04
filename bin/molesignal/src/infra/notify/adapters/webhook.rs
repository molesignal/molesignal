// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通用 Webhook：把通知以 HTTP 请求发到一个地址。
//!
//! 请求体有两种：
//! - `event`（默认）：写给机器读的事件（版本、事件、告警、规则、升级，见通知引擎），再加上 `target`
//!   与渲染好的 `message`。
//! - `template`：通知策略为 Webhook 渠道选的模板的正文原样作为请求体；模板是 JSON 格式时按 JSON 发。
//!
//! 每个请求带 `X-MoleSignal-Event`（通知类型）与 `X-MoleSignal-Delivery`（投递 ID，接收方可以拿来
//! 去重）。设了签名密钥时还带 `X-MoleSignal-Timestamp`（Unix 秒）与 `X-MoleSignal-Sign:
//! sha256=<hex>`，算法与连接器的 HTTP 目标相同：对 `<timestamp>.<body>` 算 HMAC-SHA256。
//!
//! 出站与连接器的 HTTP 目标一样：只连解析得到的地址，云元数据地址一律拒绝，不跟随跳转。

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{
    Method,
    header::{CONTENT_TYPE, HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
        NotifyMessage, NotifyTarget, NotifyTargetType,
    },
    infra::connectors::dispatcher::http::{pinned_client, signature},
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
        time::TimestampMicros,
    },
};

pub const WEBHOOK_CONNECTOR_TYPE: &str = "webhook";

/// 签名密钥最短多长。
const MIN_SECRET_CHARS: usize = 16;

/// 投递自己设的请求头，配置里的请求头不能用这些名字。
const RESERVED_HEADERS: [&str; 5] = [
    "content-type",
    "x-molesignal-event",
    "x-molesignal-delivery",
    "x-molesignal-timestamp",
    "x-molesignal-sign",
];

/// 引擎写进消息元数据、只给请求头用的键，不随 `message` 发出去。
const INTERNAL_METADATA: [&str; 2] = ["delivery_id", "body_format"];

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WebhookMethod {
    #[default]
    Post,
    Put,
    Patch,
}

impl WebhookMethod {
    fn reqwest(self) -> Method {
        match self {
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Patch => Method::PATCH,
        }
    }
}

/// 请求体发什么。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WebhookBody {
    /// 写给机器读的事件，加上目标与渲染好的消息。
    #[default]
    Event,
    /// 通知模板的正文原样。
    Template,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    method: WebhookMethod,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
    #[serde(default)]
    body: WebhookBody,
    /// 签名密钥：设了就给每个请求签名。
    #[serde(default)]
    signing_secret: Option<String>,
}

fn default_timeout_secs() -> u64 {
    10
}

impl WebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid webhook config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        if self.url.trim().is_empty() {
            return Err(Error::invalid("webhook config.url cannot be empty"));
        }

        validate_http_url(&self.url)?;

        if !(1..=60).contains(&self.timeout_secs) {
            return Err(Error::invalid(
                "webhook timeout_secs must be between 1 and 60",
            ));
        }

        if self
            .secret()
            .is_some_and(|secret| secret.chars().count() < MIN_SECRET_CHARS)
        {
            return Err(Error::invalid(format!(
                "webhook signing_secret must be at least {MIN_SECRET_CHARS} characters"
            )));
        }

        for (name, value) in &self.headers {
            let parsed = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::invalid("webhook contains an invalid header name"))?;
            HeaderValue::from_str(value)
                .map_err(|_| Error::invalid("webhook contains an invalid header value"))?;

            if RESERVED_HEADERS.contains(&parsed.as_str()) {
                return Err(Error::invalid(format!(
                    "webhook header `{name}` is set by the delivery itself"
                )));
            }
        }

        Ok(())
    }

    fn secret(&self) -> Option<&str> {
        self.signing_secret
            .as_deref()
            .map(str::trim)
            .filter(|secret| !secret.is_empty())
    }
}

#[derive(Default)]
pub struct WebhookConnectorAdapter;

impl WebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for WebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WEBHOOK_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            rich_text: true,
            interactive: false,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Webhook
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WebhookConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => validate_http_url(&target.value).map(drop),
            NotifyTargetType::DirectUser
            | NotifyTargetType::FixedAddress
            | NotifyTargetType::FixedGroup => {
                if target.value.trim().is_empty() {
                    Err(Error::invalid("webhook target cannot be empty"))
                } else {
                    Ok(())
                }
            }
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = WebhookConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.url.trim()
        };

        let url = validate_http_url(url)?;
        let client = pinned_client(&url, Duration::from_secs(config.timeout_secs)).await?;
        let (body, content_type) = request_body(config.body, target, message)?;
        let mut request = client.request(config.method.reqwest(), url);

        for (name, value) in &config.headers {
            request = request.header(name, value);
        }

        request = request.header(CONTENT_TYPE, content_type);

        if let Some(event_type) = event_type(message) {
            request = request.header("x-molesignal-event", event_type);
        }

        if let Some(delivery_id) = message.metadata.get("delivery_id") {
            request = request.header("x-molesignal-delivery", delivery_id.as_str());
        }

        if let Some(secret) = config.secret() {
            let timestamp = (TimestampMicros::now().0 / 1_000_000).to_string();

            request = request
                .header("x-molesignal-timestamp", &timestamp)
                .header("x-molesignal-sign", signature(secret, &timestamp, &body));
        }

        let started = Instant::now();

        // 地址里可能带着凭据，出错时不写地址。
        let response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("webhook request failed"))?;

        let status = response.status();

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "webhook returned HTTP {}",
                status.as_u16()
            )));
        }

        Ok(ConnectorDeliveryResult {
            provider_message_id: response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

/// 请求体与它的类型。
fn request_body(
    mode: WebhookBody,
    target: &NotifyTarget,
    message: &NotifyMessage,
) -> Result<(Vec<u8>, &'static str)> {
    if mode == WebhookBody::Template {
        let json = message.metadata.get("body_format").map(String::as_str) == Some("json");
        let content_type = if json {
            "application/json"
        } else {
            "text/plain; charset=utf-8"
        };

        return Ok((message.text.clone().into_bytes(), content_type));
    }

    let mut shown = message.clone();
    shown.event = None;
    shown
        .metadata
        .retain(|key, _| !INTERNAL_METADATA.contains(&key.as_str()));

    let mut body = match &message.event {
        Some(Value::Object(event)) => event.clone(),
        _ => serde_json::Map::from_iter([("version".to_string(), json!(1))]),
    };

    body.insert(
        "target".into(),
        json!({
            "type": target.target_type.as_str(),
            "value": target.value,
            "metadata": target.metadata,
        }),
    );

    body.insert(
        "message".into(),
        serde_json::to_value(&shown).map_err(|_| Error::internal("webhook body"))?,
    );

    let bytes = serde_json::to_vec(&Value::Object(body))
        .map_err(|_| Error::internal("webhook body could not be written"))?;

    Ok((bytes, "application/json"))
}

/// 通知类型：告警的写在元数据里，其余的写在事件里。
fn event_type(message: &NotifyMessage) -> Option<&str> {
    message
        .metadata
        .get("event_type")
        .map(String::as_str)
        .or_else(|| {
            message
                .event
                .as_ref()
                .and_then(|event| event.get("event"))
                .and_then(|event| event.get("type"))
                .and_then(Value::as_str)
        })
}

fn validate_http_url(value: &str) -> Result<Url> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| Error::invalid("webhook URL must be a valid absolute URL"))?;

    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::invalid("webhook URL must use http or https"));
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Checkout errors".into(),
            text: r#"{"summary":"Checkout errors"}"#.into(),
            markdown: None,
            html: None,
            metadata: [
                ("event_type".to_string(), "alert.triggered".to_string()),
                ("delivery_id".to_string(), "dlv-1".to_string()),
            ]
            .into(),
            provider_templates: Default::default(),
            card: None,
            event: Some(json!({"version": 1, "event": {"type": "alert.triggered"}})),
        }
    }

    fn target() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedGroup,
            value: "platform-oncall".into(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn validates_configured_and_per_target_urls() {
        let adapter = WebhookConnectorAdapter::new();
        adapter
            .validate_config(&json!({
                "url": "https://hooks.example.com/notify",
                "headers": {"Authorization": "Bearer secret"},
                "timeoutSecs": 5
            }))
            .unwrap();
        adapter
            .validate_target(&NotifyTarget {
                target_type: NotifyTargetType::Webhook,
                value: "https://hooks.example.com/team".into(),
                metadata: BTreeMap::new(),
            })
            .unwrap();
        assert!(
            adapter
                .validate_config(&json!({"url": "file:///tmp/a"}))
                .is_err()
        );
    }

    #[test]
    fn a_short_secret_and_a_header_the_delivery_sets_are_refused() {
        let adapter = WebhookConnectorAdapter::new();
        let base = |extra: Value| {
            let mut config = json!({"url": "https://hooks.example.com/notify"});
            config
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            config
        };

        assert!(
            adapter
                .validate_config(&base(json!({"signingSecret": "0123456789abcdef"})))
                .is_ok()
        );
        assert!(
            adapter
                .validate_config(&base(json!({"signingSecret": "short"})))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&base(json!({"headers": {"X-MoleSignal-Sign": "x"}})))
                .is_err()
        );
    }

    #[test]
    fn the_event_body_keeps_target_and_message_beside_the_event() {
        let (body, content_type) = request_body(WebhookBody::Event, &target(), &message()).unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(content_type, "application/json");
        assert_eq!(body["version"], 1);
        assert_eq!(body["event"]["type"], "alert.triggered");
        assert_eq!(body["target"]["value"], "platform-oncall");
        assert_eq!(body["message"]["title"], "CRITICAL · Checkout errors");
        assert!(body["message"].get("event").is_none());
        assert!(body["message"]["metadata"].get("delivery_id").is_none());
        assert_eq!(body["message"]["metadata"]["event_type"], "alert.triggered");
    }

    #[test]
    fn the_template_body_is_sent_as_written() {
        let mut message = message();
        message.metadata.insert("body_format".into(), "json".into());

        let (body, content_type) =
            request_body(WebhookBody::Template, &target(), &message).unwrap();
        assert_eq!(content_type, "application/json");
        assert_eq!(body, br#"{"summary":"Checkout errors"}"#);

        message.metadata.remove("body_format");
        let (_, content_type) = request_body(WebhookBody::Template, &target(), &message).unwrap();
        assert_eq!(content_type, "text/plain; charset=utf-8");
    }
}
