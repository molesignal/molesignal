// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyMessage,
        NotifyTarget, NotifyTargetType,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub const SLACK_APP_CONNECTOR_TYPE: &str = "slack_app";
pub const SLACK_WEBHOOK_CONNECTOR_TYPE: &str = "slack_webhook";

#[derive(Debug, Clone, Deserialize)]
struct SlackAppConfig {
    bot_token: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

fn default_api_base_url() -> String {
    "https://slack.com/api".into()
}

fn default_timeout_secs() -> u64 {
    10
}

impl SlackAppConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid slack_app config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        if self.bot_token.trim().is_empty() {
            return Err(Error::invalid("slack_app bot_token cannot be empty"));
        }
        validate_http_base(&self.api_base_url, "slack_app api_base_url")?;
        if !(1..=60).contains(&self.timeout_secs) {
            return Err(Error::invalid(
                "slack_app timeout_secs must be between 1 and 60",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SlackWebhookConfig {
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl SlackWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid slack_webhook config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        validate_http_base(&self.webhook_url, "slack_webhook webhook_url")?;
        if !(1..=60).contains(&self.timeout_secs) {
            return Err(Error::invalid(
                "slack_webhook timeout_secs must be between 1 and 60",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SlackAppConnectorAdapter;

impl SlackAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for SlackAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        SLACK_APP_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            rich_text: true,
            interactive: true,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        SlackAppConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        ) {
            return Err(Error::invalid(
                "slack_app target_type must be direct_user or fixed_group",
            ));
        }
        let value = target.value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(Error::invalid(
                "slack_app target must be a Slack user or channel ID",
            ));
        }
        Ok(())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = SlackAppConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("slack_app HTTP client could not be initialized"))?;
        let body = message
            .markdown
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&message.text);
        let text = format!("{}\n{}", message.title, body).trim().to_string();
        let started = Instant::now();
        let response = http_trace::send(
            &client,
            client
                .post(format!(
                    "{}/chat.postMessage",
                    config.api_base_url.trim_end_matches('/')
                ))
                .bearer_auth(config.bot_token.trim())
                .json(&json!({
                    "channel": target.value.trim(),
                    "text": text,
                    "unfurl_links": false,
                    "unfurl_media": false,
                })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("slack_app request failed"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "slack_app returned HTTP {}",
                status.as_u16()
            )));
        }
        let response: SlackResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("slack_app returned an invalid response"))?;
        if !response.ok {
            return Err(Error::unavailable(format!(
                "slack_app rejected the message: {}",
                response.error.as_deref().unwrap_or("unknown_error")
            )));
        }
        Ok(ConnectorDeliveryResult {
            provider_message_id: response.ts,
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

#[derive(Default)]
pub struct SlackWebhookConnectorAdapter;

impl SlackWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for SlackWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        SLACK_WEBHOOK_CONNECTOR_TYPE
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

    fn validate_config(&self, config: &Value) -> Result<()> {
        SlackWebhookConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::FixedGroup | NotifyTargetType::Webhook
        ) {
            return Err(Error::invalid(
                "slack_webhook target_type must be fixed_group or webhook",
            ));
        }
        if target.target_type == NotifyTargetType::Webhook {
            validate_http_base(&target.value, "slack_webhook target")
        } else if target.value.trim().is_empty() {
            Err(Error::invalid("slack_webhook target cannot be empty"))
        } else {
            Ok(())
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = SlackWebhookConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;
        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };
        let body = message
            .markdown
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&message.text);
        let text = format!("{}\n{}", message.title, body).trim().to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("slack_webhook HTTP client could not be initialized"))?;
        let started = Instant::now();
        let response = http_trace::send(
            &client,
            client.post(url).json(&json!({"text": text})),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("slack_webhook request failed"))?;
        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "slack_webhook returned HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(ConnectorDeliveryResult {
            provider_message_id: None,
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

fn validate_http_base(value: &str, field: &str) -> Result<()> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| Error::invalid(format!("{field} must be an absolute URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::invalid(format!("{field} must use http or https")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_app_config_and_recipient_ids() {
        let adapter = SlackAppConnectorAdapter::new();
        adapter
            .validate_config(&json!({"bot_token": "xoxb-secret"}))
            .unwrap();
        adapter
            .validate_target(&NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: "U01234567".into(),
                metadata: BTreeMap::new(),
            })
            .unwrap();
        assert!(
            adapter
                .validate_target(&NotifyTarget {
                    target_type: NotifyTargetType::FixedAddress,
                    value: "ops@example.com".into(),
                    metadata: BTreeMap::new(),
                })
                .is_err()
        );
        SlackWebhookConnectorAdapter::new()
            .validate_config(&json!({"webhook_url": "https://hooks.slack.com/services/id"}))
            .unwrap();
    }
}
