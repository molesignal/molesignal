// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Matrix: a bot account on a homeserver, posting into rooms and to people.
//!
//! The bot authenticates with its access token. A room is named by its ID (`!…`) or by an alias
//! (`#name:server`, looked up in the room directory); when the bot is not in the room yet it
//! joins, which works for rooms anyone may join and for rooms it was invited to. A person
//! (`@name:server`) is written to in a direct room: the newest one the bot's `m.direct` account
//! data lists for them that they are still in or invited to, or else a new one the bot creates,
//! inviting them, and records there (see [`homeserver`]). Each notification is one
//! `m.room.message` (`m.text`) with a plain body and HTML (see [`message`]); it is not encrypted.
//!
//! The homeserver's address is entered by hand, so requests go out like a webhook's: only to where
//! its host resolves, never to a cloud metadata address, following no redirect.

mod homeserver;
mod message;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use domain::notify::connector::{
    ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel, NotifyMessage,
    NotifyTarget, NotifyTargetType,
};
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use self::{homeserver::Homeserver, message::content};
use super::common::{default_timeout_secs, delivered, require, validate_timeout};

pub const MATRIX_BOT_CONNECTOR_TYPE: &str = "matrix_bot";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatrixConfig {
    #[serde(default)]
    homeserver_url: String,
    #[serde(default)]
    access_token: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl MatrixConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {MATRIX_BOT_CONNECTOR_TYPE} config: {error}"
            ))
        })?;

        homeserver_url(&config.homeserver_url)?;
        require(&config.access_token, "matrix_bot accessToken")?;
        validate_timeout(config.timeout_secs, MATRIX_BOT_CONNECTOR_TYPE)?;
        Ok(config)
    }
}

/// The homeserver's base URL: https, with neither a query nor a fragment.
fn homeserver_url(value: &str) -> Result<Url> {
    let invalid = || {
        Error::invalid(format!(
            "{MATRIX_BOT_CONNECTOR_TYPE} homeserverUrl must be the homeserver's https URL (https://matrix.example.org)"
        ))
    };

    let url = Url::parse(value.trim()).map_err(|_| invalid())?;
    let secure = url.scheme() == "https" || stand_in(&url);

    if !secure || url.host_str().is_none() || url.query().is_some() || url.fragment().is_some() {
        return Err(invalid());
    }

    Ok(url)
}

/// Tests talk to a local mock server in the homeserver's place.
fn stand_in(url: &Url) -> bool {
    cfg!(test) && url.host_str() == Some("127.0.0.1")
}

/// Where a notification goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Destination {
    /// A room by its ID (`!…`; rooms of version 12 and later have no server part).
    Room(String),
    /// A room by an alias (`#name:server`).
    Alias(String),
    /// A person (`@name:server`), in a direct room with the bot.
    Person(String),
}

impl Destination {
    fn of(target: &NotifyTarget) -> Result<Self> {
        let value = target.value.trim();
        let plain = !value.is_empty() && !value.chars().any(char::is_whitespace);

        match target.target_type {
            NotifyTargetType::FixedGroup if plain && value.len() > 1 && value.starts_with('!') => {
                Ok(Self::Room(value.into()))
            }
            NotifyTargetType::FixedGroup if plain && qualified(value, '#') => {
                Ok(Self::Alias(value.into()))
            }
            NotifyTargetType::DirectUser if plain && qualified(value, '@') => {
                Ok(Self::Person(value.into()))
            }
            NotifyTargetType::FixedGroup => Err(Error::invalid(
                "matrix_bot fixed_group target must be a room ID (!…) or a room alias (#name:server)",
            )),
            NotifyTargetType::DirectUser => Err(Error::invalid(
                "matrix_bot direct_user target must be a Matrix user ID (@name:server)",
            )),
            NotifyTargetType::FixedAddress | NotifyTargetType::Webhook => Err(Error::invalid(
                "matrix_bot target_type must be direct_user or fixed_group",
            )),
        }
    }
}

/// `value` is `<sigil><name>:<server>`, with neither part empty.
fn qualified(value: &str, sigil: char) -> bool {
    value
        .strip_prefix(sigil)
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(name, server)| !name.is_empty() && !server.is_empty())
}

#[derive(Default)]
pub struct MatrixBotConnectorAdapter;

impl MatrixBotConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for MatrixBotConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        MATRIX_BOT_CONNECTOR_TYPE
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

    fn validate_config(&self, config: &Value) -> Result<()> {
        MatrixConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        Destination::of(target).map(drop)
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = MatrixConfig::parse(config)?;
        let destination = Destination::of(target)?;
        let base = homeserver_url(&config.homeserver_url)?;
        let timeout = Duration::from_secs(config.timeout_secs);
        let homeserver = Homeserver::connect(&base, config.access_token.trim(), timeout).await?;
        let content = content(message);

        // One transaction per notification: should a post be repeated, the homeserver keeps one.
        let txn = format!("molesignal-{}", Uuid::now_v7().simple());
        let started = Instant::now();

        let event_id = match destination {
            Destination::Room(room) => homeserver.post(&room, &room, &txn, &content).await?,
            Destination::Alias(alias) => {
                let room = homeserver.resolve(&alias).await?;
                homeserver.post(&room, &alias, &txn, &content).await?
            }
            Destination::Person(person) => {
                let room = homeserver.direct_room(&person).await?;
                homeserver.post(&room, &room, &txn, &content).await?
            }
        };

        Ok(delivered(Some(event_id), started))
    }
}
