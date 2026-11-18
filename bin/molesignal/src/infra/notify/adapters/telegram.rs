// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Telegram: a bot made with @BotFather (`telegram_bot`), the same bot Mole Agent answers through.
//! The Bot API client, Telegram's HTML and the data of alert buttons live with Mole Agent's side
//! (`infra::im::telegram`), which reads the presses.
//!
//! A notification is one message in Telegram's HTML ([`card`]): alert buttons press back to the
//! bot, links are link buttons (or lines of the text when Telegram will not take an address), and
//! link previews are off. A person is reached by their Telegram user ID once they have started the
//! bot (a private chat's ID is the person's); a group by its chat ID (`-100…`, with the bot in it),
//! a channel by its `@username` (with the bot as an admin).

mod card;

use std::time::Instant;

use async_trait::async_trait;
use domain::notify::{
    buttons::AlertButtons,
    connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        NotifyChannel, NotifyMessage, NotifyTarget, NotifyTargetType,
    },
};
use kernel::{Error, Result};
use serde_json::{Map, Value, json};

use super::common::delivered;
use crate::infra::im::telegram::api::{Bot, TELEGRAM_API_BASE_URL, post_card};

pub const TELEGRAM_BOT_CONNECTOR_TYPE: &str = "telegram_bot";

#[derive(Default)]
pub struct TelegramBotConnectorAdapter;

impl TelegramBotConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }

    /// A notification as the fields of a Telegram message (`text`, `parse_mode`,
    /// `link_preview_options`, `reply_markup`): `note` says who did what to the alert, and
    /// without `link_buttons` the links are lines of the text. Mole Agent rewrites a card with it
    /// after a press (`infra::im::telegram::answer`).
    pub(crate) fn card(
        message: &NotifyMessage,
        buttons: Option<&AlertButtons>,
        note: Option<&str>,
        link_buttons: bool,
    ) -> Map<String, Value> {
        card::render(message, buttons, note, link_buttons)
    }
}

/// A public channel's `@username`: 5 to 32 letters, digits and underscores, from a letter.
fn channel_username(value: &str) -> bool {
    value.strip_prefix('@').is_some_and(|name| {
        (5..=32).contains(&name.len())
            && name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The `chat_id` a target sends to: a user's or group's ID as a number, a channel's `@username`.
fn chat(target: &NotifyTarget) -> Result<Value> {
    let value = target.value.trim();
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());

    match target.target_type {
        NotifyTargetType::DirectUser => value
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0 && digits(value))
            .map(Value::from)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "{TELEGRAM_BOT_CONNECTOR_TYPE} direct_user target must be a Telegram user ID (digits)"
                ))
            }),
        NotifyTargetType::FixedGroup => {
            if value.strip_prefix('-').is_some_and(digits)
                && let Ok(id) = value.parse::<i64>()
            {
                return Ok(Value::from(id));
            }

            if channel_username(value) {
                return Ok(Value::from(value));
            }

            Err(Error::invalid(format!(
                "{TELEGRAM_BOT_CONNECTOR_TYPE} fixed_group target must be a chat ID (-100…) or a channel's @username"
            )))
        }
        NotifyTargetType::FixedAddress | NotifyTargetType::Webhook => Err(Error::invalid(
            format!("{TELEGRAM_BOT_CONNECTOR_TYPE} target_type must be direct_user or fixed_group"),
        )),
    }
}

#[async_trait]
impl ConnectorAdapter for TelegramBotConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TELEGRAM_BOT_CONNECTOR_TYPE
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
        vec![ConnectorAddress::fixed("apiBaseUrl", TELEGRAM_API_BASE_URL)]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        Bot::parse(config).map(|_| ())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        chat(target).map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let bot = Bot::parse(config)?;
        let chat = chat(target)?;
        let http = bot.client()?;
        let buttons = AlertButtons::read(message);
        let started = Instant::now();

        let sent = post_card(
            &bot,
            &http,
            "sendMessage",
            &json!({ "chat_id": chat }),
            |link_buttons| card::render(message, buttons.as_ref(), None, link_buttons),
        )
        .await
        .map_err(|failure| failure.error("sendMessage"))?;

        // A message ID is only unique within its chat.
        let id = sent["message_id"]
            .as_i64()
            .map(|message_id| match sent["chat"]["id"].as_i64() {
                Some(chat_id) => format!("{chat_id}:{message_id}"),
                None => message_id.to_string(),
            });

        Ok(delivered(id, started))
    }
}

#[cfg(test)]
mod tests;
