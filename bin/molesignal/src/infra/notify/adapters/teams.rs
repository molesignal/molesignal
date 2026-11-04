// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microsoft Teams：频道 Webhook（Workflows 或旧的 Incoming Webhook）与 Bot（Bot Framework）。
//!
//! Webhook 发一张 Adaptive Card。Bot 用应用 ID 与客户端密码在租户里换令牌（见 [`app`]），往频道
//! 或群聊的会话发消息；发给一个人时先和他建一个一对一会话（Bot 要已经装给了这个人）。

mod app;
mod card;

use std::time::Instant;

use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use self::card::adaptive_card;
pub(crate) use self::{
    app::{TEAMS_CLOUDS, TeamsApp, encode},
    card::{TEAMS_ALERT_VALUE_KEY, TEAMS_ALERT_VERB, card_message, teams_card},
};
use super::common::{
    body, client, default_timeout_secs, delivered, provider_message, truncate_chars,
    validate_http_url, validate_timeout,
};
use crate::{
    domain::notify::{
        buttons::AlertButtons,
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            NotifyChannel, NotifyMessage, NotifyTarget, NotifyTargetType,
        },
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub const TEAMS_APP_CONNECTOR_TYPE: &str = "teams_app";
pub const TEAMS_WEBHOOK_CONNECTOR_TYPE: &str = "teams_webhook";

/// 一条消息的正文最多这么多字符（Teams 限制一条消息约 28 KB）。
pub(crate) const MAX_TEXT_CHARS: usize = 20_000;

#[derive(Default)]
pub struct TeamsAppConnectorAdapter;

impl TeamsAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for TeamsAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TEAMS_APP_CONNECTOR_TYPE
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
        vec![ConnectorAddress::picked(
            "serviceUrl",
            "cloud",
            &TEAMS_CLOUDS,
        )]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        TeamsApp::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        ) || target.value.trim().is_empty()
            || target.value.chars().any(char::is_whitespace)
        {
            return Err(Error::invalid(
                "teams_app target must be a user ID or a conversation ID",
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
        let app = TeamsApp::parse(config)?;
        self.validate_target(target)?;

        let http = app.http()?;
        let token = app.token(&http).await?;
        let started = Instant::now();

        let conversation = if target.target_type == NotifyTargetType::FixedGroup {
            target.value.trim().to_string()
        } else {
            app.personal_conversation(&http, &token, app.service(), target.value.trim())
                .await?
        };

        // 带着告警按钮或模板卡片的发成 Adaptive Card，其余发 Markdown。
        let buttons = AlertButtons::read(message);

        let activity = match (&buttons, &message.card) {
            (None, None) => {
                let title = message.title.trim();
                let text = if title.is_empty() {
                    body(message).trim().to_string()
                } else {
                    format!("**{title}**\n\n{}", body(message).trim())
                };

                json!({
                    "type": "message",
                    "textFormat": "markdown",
                    "text": truncate_chars(&text, MAX_TEXT_CHARS),
                })
            }
            _ => card_message(message, &buttons.unwrap_or_default(), None),
        };

        let sent = app
            .call(
                &http,
                &token,
                Method::POST,
                app.service(),
                &format!("/v3/conversations/{}/activities", encode(&conversation)),
                &activity,
            )
            .await?;

        Ok(delivered(sent.id, started))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamsWebhookConfig {
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl TeamsWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid teams_webhook config: {error}")))?;

        validate_http_url(&config.webhook_url, "teams_webhook webhookUrl")?;
        validate_timeout(config.timeout_secs, TEAMS_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

#[derive(Default)]
pub struct TeamsWebhookConnectorAdapter;

impl TeamsWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for TeamsWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TEAMS_WEBHOOK_CONNECTOR_TYPE
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
        TeamsWebhookConfig::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => validate_http_url(&target.value, "teams_webhook target"),
            NotifyTargetType::FixedGroup if !target.value.trim().is_empty() => Ok(()),
            _ => Err(Error::invalid(
                "teams_webhook target_type must be webhook or fixed_group",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = TeamsWebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };

        let http = client(config.timeout_secs, TEAMS_WEBHOOK_CONNECTOR_TYPE)?;
        let started = Instant::now();

        // 即时通讯模板有卡片时发它，其余是标题与正文的卡片。群里的 Webhook 收不到点击，不带操作按钮。
        let card = match &message.card {
            Some(_) => card_message(message, &AlertButtons::default(), None),
            None => adaptive_card(&message.title, body(message)),
        };

        let response = http_trace::send(&http, http.post(url).json(&card), HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("teams_webhook request failed"))?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        // 旧的 Incoming Webhook 失败时也可能回 200，正文里写着投递失败。
        if !status.is_success()
            || text
                .trim_start()
                .starts_with("Webhook message delivery failed")
        {
            return Err(Error::unavailable(format!(
                "teams_webhook was refused: HTTP {} {}",
                status.as_u16(),
                provider_message(&text)
            )));
        }

        Ok(delivered(None, started))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_are_checked() {
        TeamsAppConnectorAdapter::new()
            .validate_config(&json!({
                "appId": "00000000-0000-0000-0000-000000000001",
                "clientSecret": "s",
                "tenantId": "contoso.onmicrosoft.com",
            }))
            .unwrap();
        assert!(
            TeamsAppConnectorAdapter::new()
                .validate_config(&json!({"appId": "a", "clientSecret": "s"}))
                .is_err()
        );
        TeamsWebhookConnectorAdapter::new()
            .validate_config(
                &json!({"webhookUrl": "https://prod.westus.logic.azure.com/workflows/x"}),
            )
            .unwrap();
    }
}
