// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信：群机器人（Webhook）与自建应用。
//!
//! 自建应用用企业 ID 与应用 Secret 换访问令牌，按成员 userid 发应用消息，或发到应用建的群聊
//! （chatid）。消息都是 Markdown，企业微信限制 4096 字节。应用配了「接收消息」（回调的 Token 与
//! EncodingAESKey）时，发给成员的告警通知是带按钮的模板卡片（[`card`]），点击回到 MoleSignal 的回调
//! （[`callback`]）。

mod callback;
mod card;

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) use self::{
    callback::{WecomCallback, WecomCardEvent, card_event as wecom_card_event, encrypted_of},
    card::{tell_wecom_member, update_wecom_card, wecom_template_card},
};
use super::{
    card::{CardColor, card_color, card_title},
    common::{
        TOKENS, TokenCache, body, client, default_timeout_secs, delivered, provider_message,
        require, truncate_bytes, truncate_chars, validate_http_url, validate_timeout, with_query,
    },
};
use crate::{
    domain::notify::{
        buttons::AlertButtons,
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            NotifyChannel, NotifyMessage, NotifyTarget, NotifyTargetType,
        },
        template::NotifyCard,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
        ids::Id,
    },
};

pub const WECOM_APP_CONNECTOR_TYPE: &str = "wecom_app";
pub const WECOM_WEBHOOK_CONNECTOR_TYPE: &str = "wecom_webhook";

/// 企业微信 Markdown 消息最多这么多字节。
const MAX_CONTENT_BYTES: usize = 4_096;
/// 令牌无效或过期的错误码。
const TOKEN_ERRORS: [i64; 3] = [40_014, 42_001, 41_001];

const WECOM_API_BASE_URL: &str = "https://qyapi.weixin.qq.com/cgi-bin";

fn default_api_base_url() -> String {
    WECOM_API_BASE_URL.into()
}

fn content(title: &str, text: &str) -> String {
    let title = title.trim();
    let text = text.trim();

    let content = if title.is_empty() {
        text.to_string()
    } else {
        format!("**{title}**\n{text}")
    };

    truncate_bytes(&content, MAX_CONTENT_BYTES)
}

/// 一张卡片排成的 Markdown：标题按颜色上色（企业微信只认 info、comment、warning 三种字色，蓝色就不上色），
/// 再是正文，字段与链接一行一个，页脚是灰字。
fn card_content(message: &NotifyMessage, card: &NotifyCard) -> String {
    let title = card_title(card, message);

    let title = match card_color(card, message) {
        CardColor::Red | CardColor::Orange | CardColor::Yellow => {
            format!("<font color=\"warning\">{title}</font>")
        }
        CardColor::Green => format!("<font color=\"info\">{title}</font>"),
        CardColor::Grey => format!("<font color=\"comment\">{title}</font>"),
        CardColor::Blue => title.to_string(),
    };

    let mut content = format!("### {title}\n{}", body(message).trim());

    for field in &card.fields {
        content.push_str(&format!("\n**{}**：{}", field.label, field.value));
    }

    for link in &card.links {
        content.push_str(&format!("\n[{}]({})", link.label, link.url));
    }

    if !card.note.is_empty() {
        content.push_str(&format!("\n<font color=\"comment\">{}</font>", card.note));
    }

    truncate_bytes(&content, MAX_CONTENT_BYTES)
}

/// 应用、群机器人与智能机器人发的 Markdown：即时通讯模板有卡片时排成卡片的样子，其余是标题与正文。
pub(super) fn markdown_content(message: &NotifyMessage) -> String {
    match &message.card {
        Some(card) => card_content(message, card),
        None => content(&message.title, body(message)),
    }
}

#[derive(Debug, Default, Deserialize)]
struct WecomResponse {
    #[serde(default)]
    errcode: i64,
    #[serde(default)]
    errmsg: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    msgid: Option<String>,
    #[serde(default)]
    invaliduser: Option<String>,
    #[serde(default)]
    chatid: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WecomAppConfig {
    corp_id: String,
    corp_secret: String,
    /// 应用的 AgentId；表单里可能是数字也可能是数字字符串。
    agent_id: Value,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
    /// 应用「接收消息」的 Token 与 EncodingAESKey：填了，发给成员的告警通知带按钮。
    #[serde(default)]
    callback_token: String,
    #[serde(default)]
    callback_aes_key: String,
}

impl WecomAppConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid wecom_app config: {error}")))?;

        require(&config.corp_id, "wecom_app corpId")?;
        require(&config.corp_secret, "wecom_app corpSecret")?;
        config.agent_id()?;
        validate_http_url(&config.api_base_url, "wecom_app apiBaseUrl")?;
        validate_timeout(config.timeout_secs, WECOM_APP_CONNECTOR_TYPE)?;
        WecomCallback::from_app(&config)?;
        Ok(config)
    }

    fn agent_id(&self) -> Result<u64> {
        match &self.agent_id {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
        .filter(|id| *id > 0)
        .ok_or_else(|| Error::invalid("wecom_app agent_id must be the app's numeric AgentId"))
    }

    fn base(&self) -> &str {
        self.api_base_url.trim().trim_end_matches('/')
    }

    fn token_key(&self) -> String {
        TokenCache::key(WECOM_APP_CONNECTOR_TYPE, &self.corp_id, &self.corp_secret)
    }

    async fn token(&self, http: &reqwest::Client) -> Result<String> {
        TOKENS
            .get(&self.token_key(), async {
                let url = with_query(
                    &format!("{}/gettoken", self.base()),
                    &[
                        ("corpid", self.corp_id.trim()),
                        ("corpsecret", self.corp_secret.trim()),
                    ],
                )?;

                let response = http_trace::send(http, http.get(url), HttpTarget::ThirdParty)
                    .await
                    .map_err(|_| Error::unavailable("wecom_app token request failed"))?;

                let token: WecomResponse = response
                    .json()
                    .await
                    .map_err(|_| Error::unavailable("wecom_app token response was invalid"))?;

                if token.errcode != 0 || token.access_token.is_empty() {
                    return Err(Error::unavailable(format!(
                        "wecom_app token request was rejected: {}",
                        provider_message(&token.errmsg)
                    )));
                }

                Ok((token.access_token, token.expires_in.max(60)))
            })
            .await
    }
}

/// 用自建应用建一个应用群聊：`members`（userid，至少两人）拉进来，`owner` 当群主，群名最多 50 个字；
/// 返回 chatid。应用的可见范围要是根部门。
pub(crate) async fn create_wecom_chat(
    config: &Value,
    name: &str,
    owner: Option<&str>,
    members: &[String],
) -> Result<String> {
    let config = WecomAppConfig::parse(config)?;

    if members.len() < 2 {
        return Err(Error::invalid(
            "a WeCom group chat has at least two members",
        ));
    }

    let http = client(config.timeout_secs, WECOM_APP_CONNECTOR_TYPE)?;
    let token = config.token(&http).await?;
    let mut body = json!({ "name": truncate_chars(name, 50), "userlist": members });

    if let Some(owner) = owner {
        body["owner"] = json!(owner);
    }

    let url = with_query(
        &format!("{}/appchat/create", config.base()),
        &[("access_token", token.as_str())],
    )?;

    let response = http_trace::send(&http, http.post(url).json(&body), HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable("wecom_app group chat request failed"))?;
    let created: WecomResponse = response
        .json()
        .await
        .map_err(|_| Error::unavailable("wecom_app group chat response was invalid"))?;

    if created.errcode != 0 || created.chatid.is_empty() {
        return Err(Error::unavailable(format!(
            "wecom_app could not create the group chat: {}",
            provider_message(&created.errmsg)
        )));
    }

    Ok(created.chatid)
}

/// 往应用建的群聊里发一段文字。
pub(crate) async fn post_wecom_chat(config: &Value, chat_id: &str, text: &str) -> Result<()> {
    let config = WecomAppConfig::parse(config)?;
    let http = client(config.timeout_secs, WECOM_APP_CONNECTOR_TYPE)?;
    let token = config.token(&http).await?;

    let url = with_query(
        &format!("{}/appchat/send", config.base()),
        &[("access_token", token.as_str())],
    )?;
    let body = json!({ "chatid": chat_id, "msgtype": "text", "text": { "content": text } });

    let response = http_trace::send(&http, http.post(url).json(&body), HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable("wecom_app group message request failed"))?;
    let sent: WecomResponse = response
        .json()
        .await
        .map_err(|_| Error::unavailable("wecom_app group message response was invalid"))?;

    if sent.errcode != 0 {
        return Err(Error::unavailable(format!(
            "wecom_app could not post to the group chat: {}",
            provider_message(&sent.errmsg)
        )));
    }

    Ok(())
}

#[derive(Default)]
pub struct WecomAppConnectorAdapter;

impl WecomAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for WecomAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WECOM_APP_CONNECTOR_TYPE
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
        vec![ConnectorAddress::fixed("apiBaseUrl", WECOM_API_BASE_URL)]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WecomAppConfig::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        ) || target.value.trim().is_empty()
        {
            return Err(Error::invalid(
                "wecom_app target must be a member's userid or an app chat ID",
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
        let config = WecomAppConfig::parse(config)?;
        self.validate_target(target)?;

        let http = client(config.timeout_secs, WECOM_APP_CONNECTOR_TYPE)?;
        let token = config.token(&http).await?;

        let markdown = json!({ "content": markdown_content(message) });
        let buttons = AlertButtons::read(message);

        // 发给成员的告警通知带按钮：应用配了接收消息，点击才回得来。应用群聊不收模板卡片。
        let (path, body) = if let Some(buttons) = buttons.filter(|_| {
            target.target_type == NotifyTargetType::DirectUser
                && matches!(WecomCallback::from_app(&config), Ok(Some(_)))
        }) {
            let task = format!("molesignal_{}", Id::new().0);

            (
                "message/send",
                json!({
                    "touser": target.value.trim(),
                    "msgtype": "template_card",
                    "agentid": config.agent_id()?,
                    "template_card": wecom_template_card(message, &buttons, None, &task),
                }),
            )
        } else if target.target_type == NotifyTargetType::FixedGroup {
            (
                "appchat/send",
                json!({
                    "chatid": target.value.trim(),
                    "msgtype": "markdown",
                    "markdown": markdown,
                }),
            )
        } else {
            (
                "message/send",
                json!({
                    "touser": target.value.trim(),
                    "msgtype": "markdown",
                    "agentid": config.agent_id()?,
                    "markdown": markdown,
                }),
            )
        };

        let started = Instant::now();

        let url = with_query(
            &format!("{}/{path}", config.base()),
            &[("access_token", token.as_str())],
        )?;

        let response = http_trace::send(&http, http.post(url).json(&body), HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("wecom_app message request failed"))?;

        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "wecom_app returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let sent: WecomResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("wecom_app message response was invalid"))?;

        if TOKEN_ERRORS.contains(&sent.errcode) {
            TOKENS.forget(&config.token_key());
        }

        if sent.errcode != 0 {
            return Err(Error::unavailable(format!(
                "wecom_app rejected the message: {}",
                provider_message(&sent.errmsg)
            )));
        }

        if sent
            .invaliduser
            .as_deref()
            .is_some_and(|invalid| !invalid.trim().is_empty())
        {
            return Err(Error::unavailable(
                "wecom_app rejected the message: the member is not in the app's visible range",
            ));
        }

        Ok(delivered(sent.msgid, started))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WecomWebhookConfig {
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl WecomWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid wecom_webhook config: {error}")))?;

        validate_http_url(&config.webhook_url, "wecom_webhook webhookUrl")?;
        validate_timeout(config.timeout_secs, WECOM_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

#[derive(Default)]
pub struct WecomWebhookConnectorAdapter;

impl WecomWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for WecomWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WECOM_WEBHOOK_CONNECTOR_TYPE
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
        WecomWebhookConfig::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => validate_http_url(&target.value, "wecom_webhook target"),
            NotifyTargetType::FixedGroup if !target.value.trim().is_empty() => Ok(()),
            _ => Err(Error::invalid(
                "wecom_webhook target_type must be webhook or fixed_group",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = WecomWebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };

        let http = client(config.timeout_secs, WECOM_WEBHOOK_CONNECTOR_TYPE)?;
        let started = Instant::now();

        let response = http_trace::send(
            &http,
            http.post(url).json(&json!({
                "msgtype": "markdown",
                "markdown": { "content": markdown_content(message) },
            })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("wecom_webhook request failed"))?;

        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "wecom_webhook returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let sent: WecomResponse = response.json().await.unwrap_or_default();

        if sent.errcode != 0 {
            return Err(Error::unavailable(format!(
                "wecom_webhook rejected the message: {}",
                provider_message(&sent.errmsg)
            )));
        }

        Ok(delivered(None, started))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_id_is_a_number_or_a_numeric_string() {
        let base = |agent_id: Value| {
            WecomAppConfig::parse(&json!({
                "corpId": "ww1",
                "corpSecret": "s",
                "agentId": agent_id,
            }))
        };

        assert_eq!(
            base(json!(1_000_002)).unwrap().agent_id().unwrap(),
            1_000_002
        );
        assert_eq!(
            base(json!(" 1000002 ")).unwrap().agent_id().unwrap(),
            1_000_002
        );
        assert!(base(json!("abc")).is_err());
        assert!(base(json!(0)).is_err());
    }

    #[test]
    fn webhook_configs_and_targets_are_checked() {
        let adapter = WecomWebhookConnectorAdapter::new();
        adapter
            .validate_config(&json!({
                "webhookUrl": "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=k"
            }))
            .unwrap();
        assert!(
            adapter
                .validate_config(&json!({"webhookUrl": "nope"}))
                .is_err()
        );
        assert!(
            adapter
                .validate_target(&NotifyTarget {
                    target_type: NotifyTargetType::DirectUser,
                    value: "zhangsan".into(),
                    metadata: Default::default(),
                })
                .is_err()
        );
    }

    #[test]
    fn the_content_stays_within_the_byte_limit() {
        let long = "告".repeat(3_000);
        let content = content("标题", &long);

        assert!(content.len() <= MAX_CONTENT_BYTES);
        assert!(content.starts_with("**标题**\n"));
    }

    #[test]
    fn a_card_is_coloured_markdown_with_its_fields_links_and_footer() {
        let card = NotifyCard {
            title: String::new(),
            color: crate::domain::notify::template::NotifyCardColor::Severity,
            fields: vec![crate::domain::notify::template::NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            }],
            links: vec![crate::domain::notify::template::NotifyCardLink {
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
            metadata: std::collections::BTreeMap::from([(
                "severity".to_string(),
                "critical".to_string(),
            )]),
            provider_templates: Default::default(),
            event: None,
            card: Some(card.clone()),
        };

        assert_eq!(
            card_content(&message, &card),
            "### <font color=\"warning\">CRITICAL · Disk full</font>\nDisk is at **97%**.\n**Host**：db-1\n[Runbook](https://runbooks.example.com/disk)\n<font color=\"comment\">MoleSignal</font>"
        );
    }
}
