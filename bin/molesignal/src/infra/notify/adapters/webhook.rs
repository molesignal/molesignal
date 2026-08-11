// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{
    Method,
    header::{HeaderName, HeaderValue},
};
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

pub const WEBHOOK_CONNECTOR_TYPE: &str = "webhook";

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

#[derive(Debug, Clone, Deserialize)]
struct WebhookConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    method: WebhookMethod,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
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
        for (name, value) in &self.headers {
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::invalid("webhook contains an invalid header name"))?;
            HeaderValue::from_str(value)
                .map_err(|_| Error::invalid("webhook contains an invalid header value"))?;
        }
        Ok(())
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

    fn validate_config(&self, config: &Value) -> Result<()> {
        WebhookConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => validate_http_url(&target.value),
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
        validate_http_url(url)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("webhook HTTP client could not be initialized"))?;
        let mut request = client.request(config.method.reqwest(), url);
        for (name, value) in &config.headers {
            request = request.header(name, value);
        }
        let started = Instant::now();
        let response = http_trace::send(
            &client,
            request.json(&json!({
                "target": {
                    "type": target.target_type.as_str(),
                    "value": target.value,
                    "metadata": target.metadata,
                },
                "message": message,
            })),
            HttpTarget::ThirdParty,
        )
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

fn validate_http_url(value: &str) -> Result<()> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| Error::invalid("webhook URL must be a valid absolute URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::invalid("webhook URL must use http or https"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_configured_and_per_target_urls() {
        let adapter = WebhookConnectorAdapter::new();
        adapter
            .validate_config(&json!({
                "url": "https://hooks.example.com/notify",
                "headers": {"Authorization": "Bearer secret"},
                "timeout_secs": 5
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
}
