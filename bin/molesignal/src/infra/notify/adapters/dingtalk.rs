// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉：自定义机器人（群 Webhook，可加签）与企业内部应用的机器人。
//!
//! 应用机器人用 Client ID 与 Client Secret 换访问令牌，单聊按 userId 批量发，群聊按
//! openConversationId 发，消息是 `sampleMarkdown`。即时通讯模板有卡片时应用机器人与群机器人都发成
//! ActionCard，链接是卡片下面的按钮。带确认、解决按钮的告警通知是连接器里那个模板的互动卡片（见
//! [`interactive`]）。Mole Agent 在钉钉里回复也走这里。

mod card;
mod group;
mod interactive;

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use self::card::{card_message, webhook_card};
pub(crate) use self::interactive::{
    DINGTALK_ALERT_PARAM, DINGTALK_CARD_CALLBACK_TOPIC, card_params,
};
use super::common::{
    TOKENS, TokenCache, body, client, default_timeout_secs, delivered, provider_message, require,
    truncate_chars, validate_http_url, validate_timeout,
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
        ids::Id,
    },
};

pub const DINGTALK_APP_CONNECTOR_TYPE: &str = "dingtalk_app";
pub const DINGTALK_WEBHOOK_CONNECTOR_TYPE: &str = "dingtalk_webhook";

/// 钉钉 Markdown 消息正文最多这么多字符。
const MAX_TEXT_CHARS: usize = 15_000;

pub const DINGTALK_API_BASE_URL: &str = "https://api.dingtalk.com";

fn default_api_base_url() -> String {
    DINGTALK_API_BASE_URL.into()
}

/// 消息的标题与正文：标题进通知摘要，正文以四级标题开头。
fn markdown(title: &str, text: &str) -> (String, String) {
    let title = truncate_chars(title.trim(), 60);
    let text = if title.is_empty() {
        text.trim().to_string()
    } else {
        format!("#### {title}\n\n{}", text.trim())
    };

    (title, truncate_chars(&text, MAX_TEXT_CHARS))
}

/// 企业内部应用的机器人。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DingTalkApp {
    pub(crate) client_id: String,
    client_secret: String,
    /// 机器人的 robotCode；企业内部应用的机器人与 Client ID 相同，可以不填。
    #[serde(default)]
    robot_code: String,
    /// 卡片平台里发布的互动卡片模板；填了，告警通知带确认、解决按钮。
    #[serde(default)]
    card_template_id: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expire_in: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    #[serde(default)]
    process_query_key: Option<String>,
    #[serde(default)]
    invalid_staff_id_list: Vec<String>,
    #[serde(default)]
    message: Option<String>,
}

impl DingTalkApp {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let app: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid dingtalk_app config: {error}")))?;
        app.validate()?;
        Ok(app)
    }

    fn validate(&self) -> Result<()> {
        require(&self.client_id, "dingtalk_app clientId")?;
        require(&self.client_secret, "dingtalk_app clientSecret")?;
        validate_http_url(&self.api_base_url, "dingtalk_app apiBaseUrl")?;
        validate_timeout(self.timeout_secs, DINGTALK_APP_CONNECTOR_TYPE)
    }

    pub(crate) fn client_secret(&self) -> &str {
        &self.client_secret
    }

    fn base(&self) -> &str {
        self.api_base_url.trim().trim_end_matches('/')
    }

    fn robot_code(&self) -> &str {
        let code = self.robot_code.trim();

        if code.is_empty() {
            self.client_id.trim()
        } else {
            code
        }
    }

    fn token_key(&self) -> String {
        TokenCache::key(
            DINGTALK_APP_CONNECTOR_TYPE,
            &self.client_id,
            &self.client_secret,
        )
    }

    pub(crate) fn http(&self) -> Result<reqwest::Client> {
        client(self.timeout_secs, DINGTALK_APP_CONNECTOR_TYPE)
    }

    pub(crate) async fn access_token(&self, http: &reqwest::Client) -> Result<String> {
        TOKENS
            .get(&self.token_key(), async {
                let response = http_trace::send(
                    http,
                    http.post(format!("{}/v1.0/oauth2/accessToken", self.base()))
                        .json(&json!({
                            "appKey": self.client_id.trim(),
                            "appSecret": self.client_secret.trim(),
                        })),
                    HttpTarget::ThirdParty,
                )
                .await
                .map_err(|_| Error::unavailable("dingtalk_app token request failed"))?;

                let status = response.status();
                let body: Value = response.json().await.unwrap_or(Value::Null);
                let token: TokenResponse =
                    serde_json::from_value(body.clone()).unwrap_or(TokenResponse {
                        access_token: String::new(),
                        expire_in: 0,
                    });

                if !status.is_success() || token.access_token.is_empty() {
                    return Err(Error::unavailable(format!(
                        "dingtalk_app token request was rejected: {}",
                        provider_message(body["message"].as_str().unwrap_or_default())
                    )));
                }

                Ok((token.access_token, token.expire_in.max(60)))
            })
            .await
    }

    /// 发一条通知：标题之后接 Markdown 正文。
    async fn send_markdown(
        &self,
        http: &reqwest::Client,
        to: Recipient<'_>,
        title: &str,
        text: &str,
    ) -> Result<Option<String>> {
        let (title, text) = markdown(title, text);
        self.send(http, to, &title, &text).await
    }

    /// 发一条 Markdown 消息，正文原样：`Users` 是单聊的 userId，`Conversation` 是群。
    pub(crate) async fn send(
        &self,
        http: &reqwest::Client,
        to: Recipient<'_>,
        title: &str,
        text: &str,
    ) -> Result<Option<String>> {
        let param = json!({
            "title": truncate_chars(title, 60),
            "text": truncate_chars(text, MAX_TEXT_CHARS),
        });

        self.send_message(http, to, "sampleMarkdown", param).await
    }

    /// 发一条机器人消息：`msg_key` 是消息模板（`sampleMarkdown`、`sampleActionCard` 等），`param` 是它的参数。
    async fn send_message(
        &self,
        http: &reqwest::Client,
        to: Recipient<'_>,
        msg_key: &str,
        param: Value,
    ) -> Result<Option<String>> {
        let token = self.access_token(http).await?;
        let param = param.to_string();

        let (path, body) = match to {
            Recipient::Users(users) => (
                "/v1.0/robot/oToMessages/batchSend",
                json!({
                    "robotCode": self.robot_code(),
                    "userIds": users,
                    "msgKey": msg_key,
                    "msgParam": param,
                }),
            ),
            Recipient::Conversation(conversation) => (
                "/v1.0/robot/groupMessages/send",
                json!({
                    "robotCode": self.robot_code(),
                    "openConversationId": conversation,
                    "msgKey": msg_key,
                    "msgParam": param,
                }),
            ),
        };

        let response = http_trace::send(
            http,
            http.post(format!("{}{path}", self.base()))
                .header("x-acs-dingtalk-access-token", token)
                .json(&body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("dingtalk_app message request failed"))?;

        let status = response.status();
        let sent: SendResponse = response.json().await.unwrap_or_default();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            TOKENS.forget(&self.token_key());
        }

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "dingtalk_app rejected the message: HTTP {} {}",
                status.as_u16(),
                provider_message(sent.message.as_deref().unwrap_or_default())
            )));
        }

        if let Recipient::Users(users) = to
            && users
                .iter()
                .all(|user| sent.invalid_staff_id_list.contains(user))
        {
            return Err(Error::unavailable(
                "dingtalk_app rejected the message: the user is not in the app's visible range",
            ));
        }

        Ok(sent.process_query_key)
    }
}

/// 应用机器人消息发给谁。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Recipient<'a> {
    Users(&'a [String]),
    Conversation(&'a str),
}

#[derive(Default)]
pub struct DingTalkAppConnectorAdapter;

impl DingTalkAppConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for DingTalkAppConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        DINGTALK_APP_CONNECTOR_TYPE
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
        vec![ConnectorAddress::fixed("apiBaseUrl", DINGTALK_API_BASE_URL)]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        DingTalkApp::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        ) || target.value.trim().is_empty()
        {
            return Err(Error::invalid(
                "dingtalk_app target must be a user ID or an openConversationId",
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
        let app = DingTalkApp::parse(config)?;
        self.validate_target(target)?;

        let http = app.http()?;
        let started = Instant::now();
        let users = [target.value.trim().to_string()];

        let to = if target.target_type == NotifyTargetType::FixedGroup {
            Recipient::Conversation(target.value.trim())
        } else {
            Recipient::Users(&users)
        };

        // 带按钮的告警通知发成模板的互动卡片，点击经 Stream 回来；卡片的 ID 就是投递的 ID。
        if let (Some(buttons), Some(template)) =
            (AlertButtons::read(message), app.card_template_id())
        {
            let id = format!("molesignal-{}", Id::new().0);
            let params = card_params(message, &buttons, None);

            app.deliver_card(&http, to, template, &id, &params).await?;
            return Ok(delivered(Some(id), started));
        }

        let id = match &message.card {
            Some(card) => {
                let (msg_key, param) = card_message(message, card);
                app.send_message(&http, to, msg_key, param).await?
            }
            None => {
                app.send_markdown(&http, to, &message.title, body(message))
                    .await?
            }
        };

        Ok(delivered(id, started))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DingTalkWebhookConfig {
    webhook_url: String,
    #[serde(default)]
    secret: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl DingTalkWebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid dingtalk_webhook config: {error}")))?;

        validate_http_url(&config.webhook_url, "dingtalk_webhook webhookUrl")?;
        validate_timeout(config.timeout_secs, DINGTALK_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

#[derive(Debug, Default, Deserialize)]
struct WebhookResponse {
    #[serde(default)]
    errcode: i64,
    #[serde(default)]
    errmsg: String,
}

/// 加签：`timestamp\nsecret` 以密钥做 HMAC-SHA256，Base64 后作为 `sign` 参数。
fn signed_url(url: &str, secret: &str, timestamp_ms: i64) -> Result<String> {
    let mut url = Url::parse(url.trim())
        .map_err(|_| Error::invalid("dingtalk_webhook webhookUrl must be an absolute URL"))?;

    if !secret.trim().is_empty() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.trim().as_bytes());
        let sign = hmac::sign(
            &key,
            format!("{timestamp_ms}\n{}", secret.trim()).as_bytes(),
        );

        url.query_pairs_mut()
            .append_pair("timestamp", &timestamp_ms.to_string())
            .append_pair("sign", &BASE64_STANDARD.encode(sign.as_ref()));
    }

    Ok(url.to_string())
}

fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::internal("system time is before unix epoch"))?
        .as_millis();

    i64::try_from(millis).map_err(|_| Error::internal("unix milliseconds overflow"))
}

/// 往一个钉钉 Webhook 发 Markdown（自定义机器人与会话里的 sessionWebhook 一样的格式），正文
/// 原样。
pub(crate) async fn post_markdown(
    http: &reqwest::Client,
    url: &str,
    title: &str,
    text: &str,
) -> Result<()> {
    let message = json!({
        "msgtype": "markdown",
        "markdown": {
            "title": truncate_chars(title, 60),
            "text": truncate_chars(text, MAX_TEXT_CHARS),
        },
    });

    post_webhook(http, url, &message).await
}

/// 往群机器人的地址发一条消息。
async fn post_webhook(http: &reqwest::Client, url: &str, message: &Value) -> Result<()> {
    let response = http_trace::send(http, http.post(url).json(message), HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable("dingtalk webhook request failed"))?;

    if !response.status().is_success() {
        return Err(Error::unavailable(format!(
            "dingtalk webhook returned HTTP {}",
            response.status().as_u16()
        )));
    }

    let response: WebhookResponse = response.json().await.unwrap_or_default();

    if response.errcode != 0 {
        return Err(Error::unavailable(format!(
            "dingtalk webhook rejected the message: {}",
            provider_message(&response.errmsg)
        )));
    }

    Ok(())
}

#[derive(Default)]
pub struct DingTalkWebhookConnectorAdapter;

impl DingTalkWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for DingTalkWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        DINGTALK_WEBHOOK_CONNECTOR_TYPE
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
        DingTalkWebhookConfig::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => {
                validate_http_url(&target.value, "dingtalk_webhook target")
            }
            NotifyTargetType::FixedGroup if !target.value.trim().is_empty() => Ok(()),
            _ => Err(Error::invalid(
                "dingtalk_webhook target_type must be webhook or fixed_group",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = DingTalkWebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            target.value.trim()
        } else {
            config.webhook_url.trim()
        };

        let url = signed_url(url, &config.secret, now_ms()?)?;
        let http = client(config.timeout_secs, DINGTALK_WEBHOOK_CONNECTOR_TYPE)?;
        let started = Instant::now();

        // 即时通讯模板有卡片时发成卡片，其余发 Markdown。
        match &message.card {
            Some(card) => post_webhook(&http, &url, &webhook_card(message, card)).await?,
            None => {
                let (title, text) = markdown(&message.title, body(message));
                post_markdown(&http, &url, &title, &text).await?;
            }
        }

        Ok(delivered(None, started))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_and_targets_are_checked() {
        DingTalkAppConnectorAdapter::new()
            .validate_config(&json!({"clientId": "dingabc", "clientSecret": "s"}))
            .unwrap();
        assert!(
            DingTalkAppConnectorAdapter::new()
                .validate_config(&json!({"clientId": "dingabc", "clientSecret": " "}))
                .is_err()
        );
        DingTalkWebhookConnectorAdapter::new()
            .validate_config(&json!({
                "webhookUrl": "https://oapi.dingtalk.com/robot/send?access_token=t",
                "secret": "SEC1"
            }))
            .unwrap();
        assert!(
            DingTalkAppConnectorAdapter::new()
                .validate_target(&NotifyTarget {
                    target_type: NotifyTargetType::FixedAddress,
                    value: "ops@example.com".into(),
                    metadata: Default::default(),
                })
                .is_err()
        );
    }

    #[test]
    fn the_robot_code_defaults_to_the_client_id() {
        let app = DingTalkApp::parse(&json!({"clientId": "dingabc", "clientSecret": "s"})).unwrap();
        assert_eq!(app.robot_code(), "dingabc");

        let app = DingTalkApp::parse(
            &json!({"clientId": "dingabc", "clientSecret": "s", "robotCode": "robot1"}),
        )
        .unwrap();
        assert_eq!(app.robot_code(), "robot1");
    }

    #[test]
    fn a_signed_webhook_carries_the_timestamp_and_signature() {
        let url = signed_url(
            "https://oapi.dingtalk.com/robot/send?access_token=t",
            "SECsecret",
            1_700_000_000_000,
        )
        .unwrap();

        let key = hmac::Key::new(hmac::HMAC_SHA256, b"SECsecret");
        let expected =
            BASE64_STANDARD.encode(hmac::sign(&key, b"1700000000000\nSECsecret").as_ref());
        let parsed = Url::parse(&url).unwrap();
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();

        assert!(pairs.contains(&("access_token".into(), "t".into())));
        assert!(pairs.contains(&("timestamp".into(), "1700000000000".into())));
        assert!(pairs.contains(&("sign".into(), expected)));

        let unsigned =
            signed_url("https://oapi.dingtalk.com/robot/send?access_token=t", "", 1).unwrap();
        assert!(!unsigned.contains("sign="));
    }

    #[test]
    fn a_markdown_message_leads_with_its_title() {
        let (title, text) = markdown("Checkout 5xx", "Rate is **8%**.");
        assert_eq!(title, "Checkout 5xx");
        assert_eq!(text, "#### Checkout 5xx\n\nRate is **8%**.");
    }
}
