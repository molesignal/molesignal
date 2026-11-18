// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Webex: a bot that posts into spaces and to people, and a space's incoming webhook.
//!
//! The bot (`webex_bot`) posts each notification with its access token (`POST /messages`): the
//! text as Markdown, and an Adaptive Card that clients able to draw cards show instead (see
//! [`message`]). A person is addressed by email or person ID, a space by its room ID. Nothing on the
//! card can be pressed; its links open in the browser.
//!
//! The incoming webhook (`webex_webhook`, made with the Incoming Webhooks app of the Webex App Hub)
//! posts the Markdown into the space it was made for. Its URL holds the webhook's token: requests
//! go out like any webhook's (only to where the host resolves, following no redirect) and no error
//! names the URL.

mod message;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use domain::notify::connector::{
    ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
    NotifyChannel, NotifyMessage, NotifyTarget, NotifyTargetType,
};
use kernel::{Error, Result};
use reqwest::{RequestBuilder, header::RETRY_AFTER};
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};
use url::Url;

use self::message::{MAX_MARKDOWN_BYTES, bot_message, markdown};
use super::common::{
    client, default_timeout_secs, delivered, provider_message, require, validate_http_url,
    validate_timeout,
};
use crate::infra::connectors::dispatcher::http::pinned_client;

pub const WEBEX_BOT_CONNECTOR_TYPE: &str = "webex_bot";
pub const WEBEX_WEBHOOK_CONNECTOR_TYPE: &str = "webex_webhook";

/// Where the Webex REST API answers.
const WEBEX_API_BASE_URL: &str = "https://webexapis.com/v1";

/// The hosts incoming webhooks are served from: the current one, and the one older webhook URLs
/// name.
const WEBHOOK_HOSTS: [&str; 2] = ["webexapis.com", "api.ciscospark.com"];

/// The path of an incoming webhook, before its token.
const WEBHOOK_PATH: &str = "/v1/webhooks/incoming/";

fn default_api_base_url() -> String {
    WEBEX_API_BASE_URL.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BotConfig {
    #[serde(default)]
    bot_token: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl BotConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {WEBEX_BOT_CONNECTOR_TYPE} config: {error}"
            ))
        })?;

        require(&config.bot_token, "webex_bot botToken")?;

        if !config.api_base_url.trim().is_empty() {
            validate_http_url(&config.api_base_url, "webex_bot apiBaseUrl")?;
        }

        validate_timeout(config.timeout_secs, WEBEX_BOT_CONNECTOR_TYPE)?;
        Ok(config)
    }

    /// The API address: the configured one, or Webex's when it was left empty.
    fn base(&self) -> &str {
        match self.api_base_url.trim().trim_end_matches('/') {
            "" => WEBEX_API_BASE_URL,
            base => base,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookConfig {
    #[serde(default)]
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl WebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {WEBEX_WEBHOOK_CONNECTOR_TYPE} config: {error}"
            ))
        })?;

        webhook_url(&config.webhook_url, "webhookUrl")?;
        validate_timeout(config.timeout_secs, WEBEX_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

/// An incoming webhook URL, `https://webexapis.com/v1/webhooks/incoming/<token>`. `field` names
/// where it was written.
fn webhook_url(value: &str, field: &str) -> Result<Url> {
    let invalid = || {
        Error::invalid(format!(
            "{WEBEX_WEBHOOK_CONNECTOR_TYPE} {field} must be a Webex incoming webhook URL (https://webexapis.com/v1/webhooks/incoming/…)"
        ))
    };

    let url = Url::parse(value.trim()).map_err(|_| invalid())?;

    let webex = url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| WEBHOOK_HOSTS.contains(&host));

    let token = url.path().strip_prefix(WEBHOOK_PATH).unwrap_or_default();

    if (webex || stand_in(&url)) && !token.is_empty() && !token.contains('/') {
        Ok(url)
    } else {
        Err(invalid())
    }
}

/// Tests post to a local mock server in Webex's place.
fn stand_in(url: &Url) -> bool {
    cfg!(test) && url.host_str() == Some("127.0.0.1")
}

/// Who a bot message goes to: the field of `POST /messages` that names them, and its value.
fn recipient(target: &NotifyTarget) -> Result<(&'static str, &str)> {
    let value = target.value.trim();
    let plain = !value.is_empty() && !value.chars().any(char::is_whitespace);

    let email = value.split_once('@').is_some_and(|(name, domain)| {
        !name.is_empty() && domain.contains('.') && !domain.contains('@')
    });

    match target.target_type {
        NotifyTargetType::DirectUser if plain && email => Ok(("toPersonEmail", value)),
        NotifyTargetType::DirectUser if plain && !value.contains('@') => Ok(("toPersonId", value)),
        NotifyTargetType::FixedGroup if plain && !value.contains('@') => Ok(("roomId", value)),
        _ => Err(Error::invalid(
            "webex_bot target must be a person's email or person ID (direct_user), or a space's room ID (fixed_group)",
        )),
    }
}

#[derive(Default)]
pub struct WebexBotConnectorAdapter;

impl WebexBotConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for WebexBotConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WEBEX_BOT_CONNECTOR_TYPE
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
        NotifyChannel::Im
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![ConnectorAddress::fixed("apiBaseUrl", WEBEX_API_BASE_URL)]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        BotConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        recipient(target).map(drop)
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = BotConfig::parse(config)?;
        let (field, to) = recipient(target)?;
        let http = client(config.timeout_secs, WEBEX_BOT_CONNECTOR_TYPE)?;

        let mut payload = bot_message(message);
        payload[field] = json!(to);

        let request = http
            .post(format!("{}/messages", config.base()))
            .bearer_auth(config.bot_token.trim())
            .json(&payload);

        let started = Instant::now();
        let sent = post(WEBEX_BOT_CONNECTOR_TYPE, &http, request).await?;
        let id = sent["id"].as_str().map(str::to_string);

        Ok(delivered(id, started))
    }
}

#[derive(Default)]
pub struct WebexWebhookConnectorAdapter;

impl WebexWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for WebexWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WEBEX_WEBHOOK_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: false,
            group: true,
            rich_text: true,
            interactive: false,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WebhookConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => webhook_url(&target.value, "target").map(drop),
            NotifyTargetType::FixedGroup if target.value.trim().is_empty() => {
                Err(Error::invalid("webex_webhook target cannot be empty"))
            }
            NotifyTargetType::FixedGroup => Ok(()),
            NotifyTargetType::DirectUser | NotifyTargetType::FixedAddress => Err(Error::invalid(
                "webex_webhook target_type must be fixed_group or webhook",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = WebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            webhook_url(&target.value, "target")?
        } else {
            webhook_url(&config.webhook_url, "webhookUrl")?
        };

        let http = pinned_client(&url, Duration::from_secs(config.timeout_secs)).await?;
        let payload = json!({ "markdown": markdown(message, MAX_MARKDOWN_BYTES) });
        let started = Instant::now();

        post(
            WEBEX_WEBHOOK_CONNECTOR_TYPE,
            &http,
            http.post(url).json(&payload),
        )
        .await?;
        Ok(delivered(None, started))
    }
}

/// What Webex says when it refuses a request.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Refusal {
    #[serde(default)]
    message: String,
    /// Names the request when asking Webex's support about it.
    #[serde(default)]
    tracking_id: String,
}

/// Sends one request and reads the answer: its JSON (`null` when it has none), or why Webex
/// refused it. A rate limit says when to try again.
async fn post(kind: &str, http: &reqwest::Client, request: RequestBuilder) -> Result<Value> {
    let response = http_trace::send(http, request, HttpTarget::ThirdParty)
        .await
        .map_err(|error| {
            Error::unavailable(format!("{kind} request failed: {}", error.without_url()))
        })?;

    let status = response.status();

    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let text = response.text().await.unwrap_or_default();

    if status.as_u16() == 429 {
        return Err(Error::unavailable(match retry_after {
            Some(seconds) => format!("{kind} was rate limited by Webex: retry after {seconds} s"),
            None => format!("{kind} was rate limited by Webex"),
        }));
    }

    if !status.is_success() {
        let said: Refusal = serde_json::from_str(&text).unwrap_or_default();

        let reason = match said.message.trim() {
            "" => text.trim(),
            message => message,
        };

        let tracking = match said.tracking_id.trim() {
            "" => String::new(),
            id => format!(" (trackingId {id})"),
        };

        return Err(Error::unavailable(format!(
            "{kind} was refused: HTTP {} {}{tracking}",
            status.as_u16(),
            provider_message(reason)
        )));
    }

    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}
