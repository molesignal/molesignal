// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::BTreeMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
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

pub const LARK_APP_CONNECTOR_TYPE: &str = "lark_app";
pub const LARK_WEBHOOK_CONNECTOR_TYPE: &str = "lark_webhook";

#[derive(Debug, Clone, Deserialize)]
struct LarkAppConfig {
    app_id: String,
    app_secret: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_receive_id_type")]
    receive_id_type: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

fn default_api_base_url() -> String {
    "https://open.feishu.cn/open-apis".into()
}

fn default_receive_id_type() -> String {
    "open_id".into()
}

fn default_timeout_secs() -> u64 {
    10
}

impl LarkAppConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid lark_app config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        if self.app_id.trim().is_empty() || self.app_secret.trim().is_empty() {
            return Err(Error::invalid(
                "lark_app app_id and app_secret cannot be empty",
            ));
        }
        validate_http_url(&self.api_base_url, "lark_app api_base_url")?;
        if !matches!(
            self.receive_id_type.as_str(),
            "open_id" | "user_id" | "union_id" | "email" | "chat_id"
        ) {
            return Err(Error::invalid("lark_app receive_id_type is not supported"));
        }
        validate_timeout(self.timeout_secs, "lark_app")
    }
}

#[derive(Debug, Deserialize)]
struct LarkTokenResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
}

#[derive(Debug, Deserialize)]
struct LarkSendResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<LarkSendData>,
}

#[derive(Debug, Deserialize)]
struct LarkSendData {
    #[serde(default)]
    message_id: Option<String>,
}

#[derive(Default)]
pub struct LarkAppConnectorAdapter;

impl LarkAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for LarkAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        LARK_APP_CONNECTOR_TYPE
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
        LarkAppConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser
                | NotifyTargetType::FixedAddress
                | NotifyTargetType::FixedGroup
        ) || target.value.trim().is_empty()
        {
            return Err(Error::invalid(
                "lark_app target must be a user identity or chat ID",
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
        let config = LarkAppConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("lark_app HTTP client could not be initialized"))?;
        let base = config.api_base_url.trim_end_matches('/');
        let token_response = http_trace::send(
            &client,
            client
                .post(format!("{base}/auth/v3/tenant_access_token/internal"))
                .json(&json!({
                    "app_id": config.app_id,
                    "app_secret": config.app_secret,
                })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("lark_app token request failed"))?;
        if !token_response.status().is_success() {
            return Err(Error::unavailable(format!(
                "lark_app token endpoint returned HTTP {}",
                token_response.status().as_u16()
            )));
        }
        let token: LarkTokenResponse = token_response
            .json()
            .await
            .map_err(|_| Error::unavailable("lark_app token response was invalid"))?;
        if token.code != 0 || token.tenant_access_token.is_empty() {
            return Err(Error::unavailable(format!(
                "lark_app token request was rejected: {}",
                safe_provider_message(&token.msg)
            )));
        }
        let receive_id_type = match target.target_type {
            NotifyTargetType::FixedGroup => "chat_id",
            NotifyTargetType::FixedAddress => "email",
            NotifyTargetType::DirectUser => config.receive_id_type.as_str(),
            NotifyTargetType::Webhook => unreachable!("target was validated above"),
        };
        let content = json!({
            "zh_cn": {
                "title": message.title,
                "content": [[{
                    "tag": "text",
                    "text": message.markdown.as_deref().unwrap_or(&message.text)
                }]]
            }
        });
        let started = Instant::now();
        let response = http_trace::send(
            &client,
            client
                .post(format!(
                    "{base}/im/v1/messages?receive_id_type={receive_id_type}"
                ))
                .bearer_auth(token.tenant_access_token)
                .json(&json!({
                    "receive_id": target.value.trim(),
                    "msg_type": "post",
                    "content": content.to_string(),
                })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("lark_app message request failed"))?;
        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "lark_app message endpoint returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let response: LarkSendResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("lark_app message response was invalid"))?;
        if response.code != 0 {
            return Err(Error::unavailable(format!(
                "lark_app rejected the message: {}",
                safe_provider_message(&response.msg)
            )));
        }
        Ok(ConnectorDeliveryResult {
            provider_message_id: response.data.and_then(|data| data.message_id),
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LarkWebhookConfig {
    webhook_url: String,
    #[serde(default)]
    secret: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl LarkWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid lark_webhook config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        validate_http_url(&self.webhook_url, "lark_webhook webhook_url")?;
        validate_timeout(self.timeout_secs, "lark_webhook")
    }
}

#[derive(Debug, Deserialize)]
struct LarkWebhookResponse {
    #[serde(default)]
    code: Option<i32>,
    #[serde(default)]
    status_code: Option<i32>,
    #[serde(default)]
    msg: String,
}

#[derive(Default)]
pub struct LarkWebhookConnectorAdapter;

impl LarkWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for LarkWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        LARK_WEBHOOK_CONNECTOR_TYPE
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
        LarkWebhookConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::Webhook | NotifyTargetType::FixedGroup
        ) {
            return Err(Error::invalid(
                "lark_webhook target_type must be webhook or fixed_group",
            ));
        }
        if target.target_type == NotifyTargetType::Webhook {
            validate_http_url(&target.value, "lark_webhook target")
        } else if target.value.trim().is_empty() {
            Err(Error::invalid("lark_webhook target cannot be empty"))
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
        let config = LarkWebhookConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;
        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };
        let text = format!("{}\n{}", message.title, message.text)
            .trim()
            .to_string();
        let mut body = json!({
            "msg_type": "text",
            "content": {
                "text": text
            }
        });
        if !config.secret.is_empty() {
            let timestamp = unix_seconds()?;
            body["timestamp"] = json!(timestamp.to_string());
            body["sign"] = json!(lark_signature(timestamp, &config.secret));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("lark_webhook HTTP client could not be initialized"))?;
        let started = Instant::now();
        let response = http_trace::send(
            &client,
            client.post(url).json(&body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("lark_webhook request failed"))?;
        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "lark_webhook returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let response: LarkWebhookResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("lark_webhook response was invalid"))?;
        if response.code.or(response.status_code).unwrap_or(0) != 0 {
            return Err(Error::unavailable(format!(
                "lark_webhook rejected the message: {}",
                safe_provider_message(&response.msg)
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

fn validate_timeout(timeout_secs: u64, connector: &str) -> Result<()> {
    if (1..=60).contains(&timeout_secs) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{connector} timeout_secs must be between 1 and 60"
        )))
    }
}

fn validate_http_url(value: &str, field: &str) -> Result<()> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| Error::invalid(format!("{field} must be an absolute URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::invalid(format!("{field} must use http or https")));
    }
    Ok(())
}

fn safe_provider_message(message: &str) -> &str {
    let message = message.trim();
    if message.is_empty() {
        "unknown_error"
    } else {
        message
    }
}

fn lark_signature(timestamp_secs: i64, secret: &str) -> String {
    let string_to_sign = format!("{timestamp_secs}\n{secret}");
    let key = hmac::Key::new(hmac::HMAC_SHA256, string_to_sign.as_bytes());
    BASE64_STANDARD.encode(hmac::sign(&key, b"").as_ref())
}

fn unix_seconds() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::internal("system time is before unix epoch"))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::internal("unix seconds overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_lark_app_and_webhook_configs() {
        LarkAppConnectorAdapter::new()
            .validate_config(&json!({"app_id": "cli_a", "app_secret": "secret"}))
            .unwrap();
        LarkWebhookConnectorAdapter::new()
            .validate_config(&json!({
                "webhook_url": "https://open.feishu.cn/open-apis/bot/v2/hook/id",
                "secret": "secret"
            }))
            .unwrap();
        assert!(
            LarkAppConnectorAdapter::new()
                .validate_config(&json!({"app_id": "cli_a", "app_secret": ""}))
                .is_err()
        );
    }
}
