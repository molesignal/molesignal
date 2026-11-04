// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Chat：空间的 Webhook 与 Chat 应用（服务账号）。
//!
//! 应用用服务账号的密钥签一个 JWT 换访问令牌（范围 `chat.bot`，见 [`app`]），往空间发消息；发给
//! 一个人时先找到他和应用的私聊空间（他要先给应用发过消息或装了应用）。正文换成 Google Chat
//! 的格式。

mod app;

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(test)]
pub(crate) use self::app::test_key;
pub(crate) use self::app::{
    CHAT_BOT_SCOPE, GOOGLE_CHAT_API_BASE_URL, GOOGLE_PUBSUB_BASE_URL, GoogleChatApp, PUBSUB_SCOPE,
    check, space_name,
};
use super::{
    card::{card_title, escape_html, markdown_html},
    common::{
        body, client, default_timeout_secs, delivered, google_chat_text, provider_message,
        truncate_chars, validate_http_url, validate_timeout,
    },
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

pub const GOOGLE_CHAT_APP_CONNECTOR_TYPE: &str = "google_chat_app";
pub const GOOGLE_CHAT_WEBHOOK_CONNECTOR_TYPE: &str = "google_chat_webhook";

/// 一条消息最多这么多字符（Google Chat 限制 4096）。
pub(crate) const MAX_TEXT_CHARS: usize = 4_000;

/// 告警卡片上操作按钮点了以后调的函数名；按钮的值（JSON）放在同名的参数里，由它们认出来。
pub(crate) const GOOGLE_CHAT_ALERT_FUNCTION: &str = "molesignal_alert";

fn text(title: &str, body: &str) -> String {
    let title = title.trim();
    let text = if title.is_empty() {
        google_chat_text(body.trim())
    } else {
        format!("*{title}*\n{}", google_chat_text(body.trim()))
    };

    truncate_chars(&text, MAX_TEXT_CHARS)
}

/// 一张卡片发成的消息（cardsV2）：标题在卡片头上（Google Chat 的卡片头没有颜色），正文排成卡片认的 HTML，
/// 字段是带名称的一行行文字，`note` 是一句谁做了什么，然后是按钮（告警的操作与链接），最后是灰字的页脚。
/// 没有模板卡片时标题是通知的标题。
pub(crate) fn google_chat_card(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Value {
    let card = message.card.as_ref();
    let title = match card {
        Some(card) => card_title(card, message),
        None => message.title.as_str(),
    };

    let mut sections = vec![json!({
        "widgets": [{
            "textParagraph": {
                "text": truncate_chars(&markdown_html(body(message).trim()), MAX_TEXT_CHARS),
            },
        }],
    })];

    if let Some(card) = card
        && !card.fields.is_empty()
    {
        let widgets = card
            .fields
            .iter()
            .map(|field| {
                json!({
                    "decoratedText": {
                        "topLabel": field.label,
                        "text": escape_html(&field.value),
                    },
                })
            })
            .collect::<Vec<_>>();

        sections.push(json!({ "widgets": widgets }));
    }

    if let Some(note) = note {
        sections.push(json!({
            "widgets": [{
                "textParagraph": {
                    "text": format!("<font color=\"#80868b\">{}</font>", escape_html(note)),
                },
            }],
        }));
    }

    let mut actions = buttons
        .actions
        .iter()
        .map(|(action, label)| {
            let value = serde_json::to_string(&buttons.value(*action)).unwrap_or_default();

            json!({
                "text": label,
                "onClick": {
                    "action": {
                        "function": GOOGLE_CHAT_ALERT_FUNCTION,
                        "parameters": [{ "key": GOOGLE_CHAT_ALERT_FUNCTION, "value": value }],
                    },
                },
            })
        })
        .collect::<Vec<_>>();

    let links = buttons
        .open
        .iter()
        .map(|(label, url)| (label.as_str(), url.as_str()))
        .chain(
            card.into_iter()
                .flat_map(|card| &card.links)
                .map(|link| (link.label.as_str(), link.url.as_str())),
        );

    for (label, url) in links {
        actions.push(json!({ "text": label, "onClick": { "openLink": { "url": url } } }));
    }

    if !actions.is_empty() {
        sections.push(json!({ "widgets": [{ "buttonList": { "buttons": actions } }] }));
    }

    if let Some(card) = card
        && !card.note.is_empty()
    {
        sections.push(json!({
            "widgets": [{
                "textParagraph": {
                    "text": format!("<font color=\"#80868b\">{}</font>", escape_html(&card.note)),
                },
            }],
        }));
    }

    json!({
        "fallbackText": title,
        "cardsV2": [{
            "cardId": "molesignal",
            "card": { "header": { "title": title }, "sections": sections },
        }],
    })
}

/// 应用与 Webhook 发的消息：带着告警按钮或模板卡片的是卡片，其余是文字。Webhook 收不到点击，不带操作按钮。
fn payload(message: &NotifyMessage, buttons: Option<AlertButtons>) -> Value {
    match (&buttons, &message.card) {
        (None, None) => json!({ "text": text(&message.title, body(message)) }),
        _ => google_chat_card(message, &buttons.unwrap_or_default(), None),
    }
}

#[derive(Default)]
pub struct GoogleChatAppConnectorAdapter;

impl GoogleChatAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for GoogleChatAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        GOOGLE_CHAT_APP_CONNECTOR_TYPE
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
        vec![
            ConnectorAddress::fixed("apiBaseUrl", GOOGLE_CHAT_API_BASE_URL),
            ConnectorAddress::fixed("pubsubBaseUrl", GOOGLE_PUBSUB_BASE_URL),
        ]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        GoogleChatApp::parse(config)?.validate_token_uri()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        ) || target.value.trim().is_empty()
            || target.value.chars().any(char::is_whitespace)
        {
            return Err(Error::invalid(
                "google_chat_app target must be a user (users/123) or a space (spaces/AAAA)",
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
        let app = GoogleChatApp::parse(config)?;
        self.validate_target(target)?;

        let http = app.http()?;
        let token = app.token(&http, CHAT_BOT_SCOPE).await?;
        let started = Instant::now();

        let space = if target.target_type == NotifyTargetType::FixedGroup {
            space_name(&target.value)
        } else {
            app.direct_message(&http, &token, &target.value).await?
        };

        let payload = payload(message, AlertButtons::read(message));

        let sent = check(
            http_trace::send(
                &http,
                http.post(format!("{}/{space}/messages", app.chat_base()))
                    .bearer_auth(&token)
                    .json(&payload),
                HttpTarget::ThirdParty,
            )
            .await
            .map_err(|_| Error::unavailable("google_chat_app request failed"))?,
            "sending the message",
        )
        .await?;

        Ok(delivered(
            sent["name"].as_str().map(str::to_string),
            started,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleChatWebhookConfig {
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl GoogleChatWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!("invalid google_chat_webhook config: {error}"))
        })?;

        validate_http_url(&config.webhook_url, "google_chat_webhook webhookUrl")?;
        validate_timeout(config.timeout_secs, GOOGLE_CHAT_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

#[derive(Default)]
pub struct GoogleChatWebhookConnectorAdapter;

impl GoogleChatWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for GoogleChatWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        GOOGLE_CHAT_WEBHOOK_CONNECTOR_TYPE
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
        GoogleChatWebhookConfig::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => {
                validate_http_url(&target.value, "google_chat_webhook target")
            }
            NotifyTargetType::FixedGroup if !target.value.trim().is_empty() => Ok(()),
            _ => Err(Error::invalid(
                "google_chat_webhook target_type must be webhook or fixed_group",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = GoogleChatWebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };

        let http = client(config.timeout_secs, GOOGLE_CHAT_WEBHOOK_CONNECTOR_TYPE)?;
        let started = Instant::now();

        let response = http_trace::send(
            &http,
            http.post(url).json(&payload(message, None)),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("google_chat_webhook request failed"))?;

        let status = response.status();
        let sent: Value = response.json().await.unwrap_or(Value::Null);

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "google_chat_webhook was refused: HTTP {} {}",
                status.as_u16(),
                provider_message(sent["error"]["message"].as_str().unwrap_or_default())
            )));
        }

        Ok(delivered(
            sent["name"].as_str().map(str::to_string),
            started,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_a_service_account_key() {
        let adapter = GoogleChatAppConnectorAdapter::new();
        let key = json!({
            "type": "service_account",
            "client_email": "mole@project.iam.gserviceaccount.com",
            "private_key": app::test_key(),
        });

        adapter
            .validate_config(&json!({ "credentials": key }))
            .unwrap();
        adapter
            .validate_config(&json!({ "credentials": key.to_string() }))
            .unwrap();
        assert!(
            adapter
                .validate_config(
                    &json!({ "credentials": {"client_email": "x", "private_key": "nope"} })
                )
                .is_err()
        );
    }

    #[test]
    fn the_token_uri_of_the_key_is_googles() {
        let adapter = GoogleChatAppConnectorAdapter::new();
        let key = |token_uri: &str| {
            json!({
                "credentials": {
                    "client_email": "mole@project.iam.gserviceaccount.com",
                    "private_key": app::test_key(),
                    "token_uri": token_uri,
                },
            })
        };

        for google in [
            "https://oauth2.googleapis.com/token",
            "https://accounts.google.com/o/oauth2/token",
        ] {
            adapter.validate_config(&key(google)).unwrap();
        }

        for elsewhere in [
            "https://attacker.example/token",
            "http://oauth2.googleapis.com/token",
            "https://oauth2.googleapis.com.attacker.example/token",
        ] {
            assert!(
                adapter.validate_config(&key(elsewhere)).is_err(),
                "{elsewhere}"
            );
        }
    }

    #[test]
    fn text_takes_its_google_chat_form() {
        assert_eq!(
            text("Disk full", "**db-1** at 95%"),
            "*Disk full*\n*db-1* at 95%"
        );
    }

    #[test]
    fn a_card_is_a_chat_card_with_fields_buttons_and_footer() {
        use crate::domain::notify::template::{
            NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink,
        };

        let card = NotifyCard {
            title: String::new(),
            color: NotifyCardColor::Severity,
            fields: vec![NotifyCardField {
                label: "Host".into(),
                value: "db-1 <primary>".into(),
            }],
            links: vec![NotifyCardLink {
                label: "Runbook".into(),
                url: "https://runbooks.example.com/disk".into(),
            }],
            note: "MoleSignal".into(),
        };

        let message = NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at **97%**.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: Some(card.clone()),
        };

        let payload = payload(&message, None);
        let card = &payload["cardsV2"][0]["card"];

        assert_eq!(payload["fallbackText"], "CRITICAL · Disk full");
        assert_eq!(card["header"]["title"], "CRITICAL · Disk full");
        assert_eq!(
            card["sections"][0]["widgets"][0]["textParagraph"]["text"],
            "Disk is at <b>97%</b>."
        );
        assert_eq!(
            card["sections"][1]["widgets"][0]["decoratedText"]["text"],
            "db-1 &lt;primary&gt;"
        );
        assert_eq!(
            card["sections"][2]["widgets"][0]["buttonList"]["buttons"][0]["onClick"]["openLink"]["url"],
            "https://runbooks.example.com/disk"
        );
        assert_eq!(
            card["sections"][3]["widgets"][0]["textParagraph"]["text"],
            "<font color=\"#80868b\">MoleSignal</font>"
        );
    }

    #[test]
    fn an_alert_card_calls_back_with_its_buttons_value() {
        use crate::domain::notify::buttons::{AlertAction, AlertButtonValue};

        let message = NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at **97%**.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };
        let buttons = AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: None,
            alert_ids: Vec::new(),
            actions: vec![(AlertAction::Acknowledge, "Acknowledge".into())],
            open: Some(("Open".into(), "https://molesignal.example.com".into())),
        };

        let payload = payload(&message, Some(buttons));
        let card = &payload["cardsV2"][0]["card"];

        assert_eq!(card["header"]["title"], "CRITICAL · Disk full");

        let buttons = &card["sections"][1]["widgets"][0]["buttonList"]["buttons"];
        let action = &buttons[0]["onClick"]["action"];
        assert_eq!(action["function"], GOOGLE_CHAT_ALERT_FUNCTION);
        assert_eq!(action["parameters"][0]["key"], GOOGLE_CHAT_ALERT_FUNCTION);

        let value: AlertButtonValue =
            serde_json::from_str(action["parameters"][0]["value"].as_str().unwrap()).unwrap();
        assert_eq!(value.action, AlertAction::Acknowledge);
        assert_eq!(value.alert_id, "inc-1");

        assert_eq!(
            buttons[1]["onClick"]["openLink"]["url"],
            "https://molesignal.example.com"
        );
    }
}
