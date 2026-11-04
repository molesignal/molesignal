// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use super::{
    card::{card_color, card_title},
    common::truncate_chars,
};
use crate::{
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
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

pub const SLACK_APP_CONNECTOR_TYPE: &str = "slack_app";
pub const SLACK_WEBHOOK_CONNECTOR_TYPE: &str = "slack_webhook";

/// 告警卡片上按钮所在的块；操作按钮的 `action_id` 以它开头，点了以后由它认出来。
pub(crate) const SLACK_ALERT_BLOCK_ID: &str = "molesignal_alert";
/// section 块的文字最多这么多字符。
const MAX_SECTION_CHARS: usize = 3_000;
/// 标题块的文字最多这么多字符。
const MAX_HEADER_CHARS: usize = 150;

/// 标准 Markdown 换成 Slack 的 mrkdwn：标题行加粗，`**` 换成 `*`。
pub(crate) fn mrkdwn(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let heading = trimmed.trim_start_matches('#');

            if trimmed.starts_with('#') && heading.starts_with(' ') {
                format!("*{}*", heading.trim())
            } else {
                line.replace("**", "*")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// mrkdwn 里要转义的三个字符。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 卡片放进消息里的部分。有模板卡片时：标题块、正文、字段，`note` 是一句谁做了什么，按钮（告警的操作与
/// 模板卡片的链接），最后是页脚，都放进带颜色条的附件。没有时直接是 `blocks`：加粗的标题与正文、`note`、
/// 告警的按钮。操作按钮的值是 [`crate::domain::notify::buttons::AlertButtonValue`] 的 JSON。
pub(crate) fn slack_card(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Map<String, Value> {
    let card = message.card.as_ref();

    let body = message
        .markdown
        .as_deref()
        .filter(|markdown| !markdown.trim().is_empty())
        .unwrap_or(&message.text);

    let mut blocks = Vec::new();

    match card {
        Some(card) => {
            blocks.push(json!({
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": truncate_chars(card_title(card, message), MAX_HEADER_CHARS),
                },
            }));

            blocks.push(json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": truncate_chars(mrkdwn(body).trim(), MAX_SECTION_CHARS) },
            }));

            if !card.fields.is_empty() {
                let fields = card
                    .fields
                    .iter()
                    .map(|field| {
                        json!({
                            "type": "mrkdwn",
                            "text": format!("*{}*\n{}", escape(&field.label), field.value),
                        })
                    })
                    .collect::<Vec<_>>();

                blocks.push(json!({ "type": "section", "fields": fields }));
            }
        }
        None => {
            let text = format!("*{}*\n{}", escape(message.title.trim()), mrkdwn(body));

            blocks.push(json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": truncate_chars(text.trim(), MAX_SECTION_CHARS) },
            }));
        }
    }

    if let Some(note) = note {
        blocks.push(json!({
            "type": "context",
            "elements": [{ "type": "plain_text", "text": note }],
        }));
    }

    let mut elements = buttons
        .actions
        .iter()
        .map(|(action, label)| {
            let mut button = json!({
                "type": "button",
                "action_id": format!("{SLACK_ALERT_BLOCK_ID}_{}", action.as_str()),
                "text": { "type": "plain_text", "text": label },
                "value": serde_json::to_string(&buttons.value(*action)).unwrap_or_default(),
            });

            if matches!(
                action,
                AlertAction::Acknowledge | AlertAction::AcknowledgeAll
            ) {
                button["style"] = json!("primary");
            }

            button
        })
        .collect::<Vec<_>>();

    if let Some((label, url)) = &buttons.open {
        elements.push(json!({
            "type": "button",
            "action_id": format!("{SLACK_ALERT_BLOCK_ID}_open"),
            "text": { "type": "plain_text", "text": label },
            "url": url,
        }));
    }

    for (index, link) in card.into_iter().flat_map(|card| &card.links).enumerate() {
        elements.push(json!({
            "type": "button",
            "action_id": format!("{SLACK_ALERT_BLOCK_ID}_link_{index}"),
            "text": { "type": "plain_text", "text": link.label },
            "url": link.url,
        }));
    }

    if !elements.is_empty() {
        blocks.push(json!({
            "type": "actions",
            "block_id": SLACK_ALERT_BLOCK_ID,
            "elements": elements,
        }));
    }

    let mut parts = Map::new();

    match card {
        Some(card) => {
            if !card.note.is_empty() {
                blocks.push(json!({
                    "type": "context",
                    "elements": [{ "type": "mrkdwn", "text": escape(&card.note) }],
                }));
            }

            parts.insert(
                "attachments".into(),
                json!([{ "color": card_color(card, message).hex(), "blocks": blocks }]),
            );
        }
        None => {
            parts.insert("blocks".into(), Value::Array(blocks));
        }
    }

    parts
}

/// A Slack app. The bot token sends messages; the client ID and secret let
/// status page visitors add the app's incoming webhook to their channels; the
/// app-level token lets Mole Agent receive messages over Socket Mode.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlackAppConfig {
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    app_token: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

/// Slack 与 GovSlack（美国政府的 Slack）：同一种 App 的两个地址，第一个是默认的。
pub const SLACK_SITES: [(&str, &str); 2] = [
    ("slack", "https://slack.com/api"),
    ("govslack", "https://slack-gov.com/api"),
];

fn default_api_base_url() -> String {
    SLACK_SITES[0].1.into()
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
        let has_client_id = !self.client_id.trim().is_empty();
        let has_client_secret = !self.client_secret.trim().is_empty();

        if has_client_id != has_client_secret {
            return Err(Error::invalid(
                "slack_app clientId and client_secret must be set together",
            ));
        }

        if self.bot_token.trim().is_empty() && !has_client_id {
            return Err(Error::invalid(
                "slack_app needs a bot_token, or a client_id and client_secret",
            ));
        }

        let app_token = self.app_token.trim();

        if !app_token.is_empty() && !app_token.starts_with("xapp-") {
            return Err(Error::invalid(
                "slack_app app_token must be an app-level token (xapp-…)",
            ));
        }

        validate_http_base(&self.api_base_url, "slack_app apiBaseUrl")?;

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
#[serde(rename_all = "camelCase")]
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
        validate_http_base(&self.webhook_url, "slack_webhook webhookUrl")?;

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

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![ConnectorAddress::picked("apiBaseUrl", "site", &SLACK_SITES)]
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

        if config.bot_token.trim().is_empty() {
            return Err(Error::invalid(
                "slack_app needs a bot_token to send messages",
            ));
        }

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

        let mut payload = json!({
            "channel": target.value.trim(),
            "text": text,
            "unfurl_links": false,
            "unfurl_media": false,
        });

        // 带着告警按钮或模板卡片的发成卡片，`text` 留给通知与读屏。
        let buttons = AlertButtons::read(message);

        if buttons.is_some() || message.card.is_some() {
            for (key, value) in slack_card(message, &buttons.unwrap_or_default(), None) {
                payload[key] = value;
            }
        }

        let started = Instant::now();

        let response = http_trace::send(
            &client,
            client
                .post(format!(
                    "{}/chat.postMessage",
                    config.api_base_url.trim_end_matches('/')
                ))
                .bearer_auth(config.bot_token.trim())
                .json(&payload),
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

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
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
        let mut payload = json!({ "text": text });

        // 即时通讯模板有卡片时发成卡片，`text` 留给通知与读屏。
        if message.card.is_some() {
            for (key, value) in slack_card(message, &AlertButtons::default(), None) {
                payload[key] = value;
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("slack_webhook HTTP client could not be initialized"))?;

        let started = Instant::now();

        let response = http_trace::send(
            &client,
            client.post(url).json(&payload),
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
    use crate::domain::notify::template::{
        NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink,
    };

    #[test]
    fn validates_app_config_and_recipient_ids() {
        let adapter = SlackAppConnectorAdapter::new();
        adapter
            .validate_config(&json!({"botToken": "xoxb-secret"}))
            .unwrap();
        adapter
            .validate_config(&json!({"clientId": "1.2", "clientSecret": "secret"}))
            .unwrap();
        assert!(adapter.validate_config(&json!({})).is_err());
        adapter
            .validate_config(&json!({"botToken": "xoxb-secret", "appToken": "xapp-1-A"}))
            .unwrap();
        assert!(
            adapter
                .validate_config(&json!({"botToken": "xoxb-secret", "appToken": "xoxb-wrong"}))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({"botToken": "xoxb-secret", "clientId": "1.2"}))
                .is_err()
        );
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
            .validate_config(&json!({"webhookUrl": "https://hooks.slack.com/services/id"}))
            .unwrap();
    }

    #[test]
    fn alert_blocks_carry_the_buttons_and_who_acted() {
        let message = NotifyMessage {
            title: "CRITICAL · p99 > 2s & rising".into(),
            text: "Alert is open.".into(),
            markdown: Some("**API unavailable**\n\nSeverity: `critical`".into()),
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };
        let buttons = AlertButtons {
            alert_id: "incident-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: None,
            alert_ids: Vec::new(),
            actions: vec![
                (AlertAction::Acknowledge, "Acknowledge".into()),
                (AlertAction::Resolve, "Resolve".into()),
            ],
            open: Some((
                "Open in MoleSignal".into(),
                "https://example/alerts/incidents/incident-1".into(),
            )),
        };

        let card = slack_card(&message, &buttons, Some("Alice acknowledged this alert."));
        let blocks = &card["blocks"];

        assert_eq!(
            blocks[0]["text"]["text"],
            "*CRITICAL · p99 &gt; 2s &amp; rising*\n*API unavailable*\n\nSeverity: `critical`"
        );
        assert_eq!(
            blocks[1]["elements"][0]["text"],
            "Alice acknowledged this alert."
        );

        let actions = &blocks[2];
        assert_eq!(actions["block_id"], SLACK_ALERT_BLOCK_ID);
        assert_eq!(
            actions["elements"][0]["action_id"],
            "molesignal_alert_acknowledge"
        );
        assert_eq!(actions["elements"][0]["style"], "primary");

        let value: Value =
            serde_json::from_str(actions["elements"][1]["value"].as_str().unwrap()).unwrap();
        assert_eq!(
            value,
            json!({
                "action": "resolve",
                "alert_id": "incident-1",
                "event_type": "alert.triggered",
            })
        );
        assert_eq!(
            actions["elements"][2]["url"],
            "https://example/alerts/incidents/incident-1"
        );

        let resolved = slack_card(&message, &AlertButtons::default(), None);
        assert_eq!(resolved["blocks"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_template_card_goes_in_an_attachment_with_its_colour() {
        let message = NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: Some("Disk is at **97%**.".into()),
            html: None,
            metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
            provider_templates: Default::default(),
            event: None,
            card: Some(NotifyCard {
                title: String::new(),
                color: NotifyCardColor::Severity,
                fields: vec![NotifyCardField {
                    label: "Host".into(),
                    value: "db-1".into(),
                }],
                links: vec![NotifyCardLink {
                    label: "Runbook".into(),
                    url: "https://runbooks.example.com/disk".into(),
                }],
                note: "MoleSignal".into(),
            }),
        };

        let card = slack_card(&message, &AlertButtons::default(), None);
        let attachment = &card["attachments"][0];
        let blocks = &attachment["blocks"];

        assert!(card.get("blocks").is_none());
        assert_eq!(attachment["color"], "#D93025");
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[0]["text"]["text"], "CRITICAL · Disk full");
        assert_eq!(blocks[1]["text"]["text"], "Disk is at *97%*.");
        assert_eq!(blocks[2]["fields"][0]["text"], "*Host*\ndb-1");
        assert_eq!(
            blocks[3]["elements"][0]["url"],
            "https://runbooks.example.com/disk"
        );
        assert_eq!(
            blocks[3]["elements"][0]["action_id"],
            "molesignal_alert_link_0"
        );
        assert_eq!(blocks[4]["elements"][0]["text"], "MoleSignal");
    }
}
