// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信智能机器人：Mole Agent 用它在企业微信里回答，也用它发告警通知。
//!
//! 智能机器人用 Bot ID 与长连接专用的 Secret 订阅一条长连接，消息、回复与通知都走这条连接（协议在
//! `infra::im::wecom`）。成员绑定的通知身份是机器人看到的 userid，用来认出提问人，也是单聊通知发给
//! 谁；群通知发到群的 chatid。机器人只能发到有人给它发过消息的会话：私聊过它的人，有人 @ 过它的群。
//!
//! 告警通知带按钮时是按钮交互型的模板卡片，点击经长连接回来；没有按钮的是 Markdown。本节点没连着
//! 机器人时，通知记进待发的回复，由连着它的节点发出。

use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    common::{delivered, require},
    wecom::markdown_content,
    wecom_template_card,
};
use crate::{
    agent::im::ImRepository,
    domain::notify::{
        buttons::AlertButtons,
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget, NotifyTargetType,
        },
    },
    infra::im::wecom,
    shared::{Error, Result, ids::Id},
};

pub const WECOM_BOT_CONNECTOR_TYPE: &str = "wecom_bot";

const WECOM_BOT_WS_URL: &str = "wss://openws.work.weixin.qq.com";

fn default_ws_url() -> String {
    WECOM_BOT_WS_URL.into()
}

/// 一个智能机器人的长连接凭据。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WecomBot {
    pub(crate) bot_id: String,
    /// 长连接专用的 Secret（不是 URL 回调的 Token 与 EncodingAESKey）。
    bot_secret: String,
    #[serde(default = "default_ws_url")]
    ws_url: String,
}

impl WecomBot {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let bot: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid wecom_bot config: {error}")))?;

        require(&bot.bot_id, "wecom_bot botId")?;
        require(&bot.bot_secret, "wecom_bot botSecret")?;

        let url = url::Url::parse(bot.ws_url.trim())
            .map_err(|_| Error::invalid("wecom_bot ws_url must be a wss:// address"))?;

        if !matches!(url.scheme(), "wss" | "ws") || url.host_str().is_none() {
            return Err(Error::invalid("wecom_bot ws_url must be a wss:// address"));
        }

        Ok(bot)
    }

    pub(crate) fn secret(&self) -> &str {
        self.bot_secret.trim()
    }

    pub(crate) fn ws_url(&self) -> &str {
        self.ws_url.trim()
    }
}

#[derive(Default)]
pub struct WecomBotConnectorAdapter {
    /// 本节点没连着机器人时，通知记进这里，由连着它的节点发出。
    outbox: Option<Arc<dyn ImRepository>>,
}

impl WecomBotConnectorAdapter {
    pub const fn new() -> Self {
        Self { outbox: None }
    }

    /// 本节点没连着机器人时把通知记进 `outbox`；没有它时只有连着机器人的节点发得出去。
    pub fn with_outbox(outbox: Arc<dyn ImRepository>) -> Self {
        Self {
            outbox: Some(outbox),
        }
    }
}

/// 通知的消息体：告警通知带按钮时是按钮交互型的模板卡片，其余是 Markdown。
fn notification(message: &NotifyMessage) -> Value {
    match AlertButtons::read(message) {
        Some(buttons) => {
            let task = format!("molesignal_{}", Id::new().0);

            json!({
                "msgtype": "template_card",
                "template_card": wecom_template_card(message, &buttons, None, &task),
            })
        }
        None => json!({
            "msgtype": "markdown",
            "markdown": { "content": markdown_content(message) },
        }),
    }
}

#[async_trait]
impl ConnectorAdapter for WecomBotConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        WECOM_BOT_CONNECTOR_TYPE
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
        vec![ConnectorAddress::fixed("wsUrl", WECOM_BOT_WS_URL)]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WecomBot::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        let known = matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedGroup
        );

        if !known || target.value.trim().is_empty() {
            return Err(Error::invalid(
                "wecom_bot target must be the userid the smart bot sees or a group chatid",
            ));
        }

        Ok(())
    }

    async fn send(
        &self,
        _config: &Value,
        _target: &NotifyTarget,
        _message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        Err(Error::internal(
            "wecom_bot sends as its connector: another server may hold the smart bot's connection",
        ))
    }

    async fn send_as(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        WecomBot::parse(&connector.config)?;
        self.validate_target(target)?;

        let kind = if target.target_type == NotifyTargetType::FixedGroup {
            2
        } else {
            1
        };
        let started = Instant::now();

        wecom::deliver(
            connector,
            target.value.trim(),
            kind,
            notification(message),
            self.outbox.as_deref(),
        )
        .await?;

        Ok(delivered(None, started))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{domain::notify::buttons::AlertAction, shared::time::TimestampMicros};

    #[test]
    fn a_smart_bot_needs_its_id_and_secret_and_a_websocket_address() {
        let bot = WecomBot::parse(&json!({ "botId": " aib1 ", "botSecret": " s " })).unwrap();

        assert_eq!(bot.bot_id.trim(), "aib1");
        assert_eq!(bot.secret(), "s");
        assert_eq!(bot.ws_url(), "wss://openws.work.weixin.qq.com");

        assert!(WecomBot::parse(&json!({ "botId": "aib1", "botSecret": "" })).is_err());
        assert!(
            WecomBot::parse(&json!({
                "botId": "aib1",
                "botSecret": "s",
                "wsUrl": "https://openws.work.weixin.qq.com",
            }))
            .is_err()
        );
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type,
            value: value.into(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn it_sends_to_people_and_groups() {
        let adapter = WecomBotConnectorAdapter::new();

        assert!(adapter.capabilities().group);
        adapter
            .validate_target(&target(NotifyTargetType::DirectUser, "zhangsan"))
            .unwrap();
        adapter
            .validate_target(&target(NotifyTargetType::FixedGroup, "wrkSFfCgAA"))
            .unwrap();
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::Webhook, "https://example.com"))
                .is_err()
        );
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::DirectUser, " "))
                .is_err()
        );
    }

    #[test]
    fn an_alert_with_buttons_goes_out_as_a_template_card() {
        let mut alert = message();
        AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            actions: vec![(AlertAction::Acknowledge, "确认".into())],
            ..Default::default()
        }
        .write(&mut alert);

        let card = notification(&alert);
        assert_eq!(card["msgtype"], "template_card");
        assert_eq!(card["template_card"]["card_type"], "button_interaction");
        assert!(
            card["template_card"]["task_id"]
                .as_str()
                .unwrap()
                .starts_with("molesignal_")
        );

        let plain = notification(&message());
        assert_eq!(plain["msgtype"], "markdown");
        assert!(
            plain["markdown"]["content"]
                .as_str()
                .unwrap()
                .contains("Disk is at 97%.")
        );
    }

    #[tokio::test]
    async fn without_the_connection_nothing_is_sent() {
        let connector = NotifyConnector {
            id: Id::from_string("wecom-bot"),
            organization_id: Id::from_string("org-1"),
            name: "WeCom bot".into(),
            connector_type: WECOM_BOT_CONNECTOR_TYPE.into(),
            config: json!({ "botId": "aib-unheld", "botSecret": "s" }),
            capabilities: Default::default(),
            enabled: true,
            status: Default::default(),
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };

        let error = WecomBotConnectorAdapter::new()
            .send_as(
                &connector,
                &target(NotifyTargetType::DirectUser, "zhangsan"),
                &message(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not connected"));
    }
}
