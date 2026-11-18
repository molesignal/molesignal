// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bot's side of Mattermost's REST API (v4): its settings, where a target's posts go (a person's
//! direct channel with the bot, or a channel by ID or by team and name), and posting.

use std::time::Duration;

use domain::notify::connector::{NotifyTarget, NotifyTargetType};
use kernel::{Error, Result};
use reqwest::{RequestBuilder, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};
use url::Url;

use super::{MattermostPost, bot::MATTERMOST_BOT_CONNECTOR_TYPE as CONNECTOR, check};
use crate::infra::{
    connectors::dispatcher::http::pinned_client,
    notify::adapters::common::{default_timeout_secs, validate_timeout},
};

/// A bot account's settings: the server, the bot's access token and the request timeout.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BotConfig {
    #[serde(default)]
    server_url: String,
    #[serde(default)]
    bot_token: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl BotConfig {
    /// Reads and checks a connector's config.
    pub(super) fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid {CONNECTOR} config: {error}")))?;

        config.server()?;
        let token = config.bot_token.trim();

        if token.is_empty() || token.chars().any(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "{CONNECTOR} botToken must be the bot account's access token"
            )));
        }

        validate_timeout(config.timeout_secs, CONNECTOR)?;
        Ok(config)
    }

    /// The server's address, without a trailing `/` or `/api/v4`.
    fn server(&self) -> Result<Url> {
        let value = self.server_url.trim().trim_end_matches('/');
        let value = value.strip_suffix("/api/v4").unwrap_or(value);

        let refused = || {
            Error::invalid(format!(
                "{CONNECTOR} serverUrl must be the http(s) address of the Mattermost server, like \
                 https://chat.example.com"
            ))
        };

        let url = Url::parse(value).map_err(|_| refused())?;

        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(refused());
        }

        Ok(url)
    }
}

/// Where a bot post goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// A person: a username, or maybe a user ID (26 letters and digits written without `@`).
    Person { name: String, maybe_id: bool },
    /// A channel by its ID.
    ChannelId(String),
    /// A channel by its team's name and its own.
    Channel { team: String, name: String },
}

impl Target {
    pub(super) fn parse(target: &NotifyTarget) -> Result<Self> {
        let value = target.value.trim();

        match target.target_type {
            NotifyTargetType::DirectUser => {
                let (name, tagged) = match value.strip_prefix('@') {
                    Some(name) => (name, true),
                    None => (value, false),
                };

                // Mattermost usernames are lowercase. Written otherwise, the target would still
                // reach the person, but their presses would match no notification method.
                if !is_name(name) || name.chars().any(|c| c.is_ascii_uppercase()) {
                    return Err(Error::invalid(format!(
                        "{CONNECTOR} direct target must be a lowercase username, with or without \
                         the @, or a 26-character user ID"
                    )));
                }

                Ok(Self::Person {
                    maybe_id: !tagged && is_id(name),
                    name: name.to_string(),
                })
            }
            NotifyTargetType::FixedGroup => {
                if let Some((team, channel)) = value.split_once('/') {
                    // People write channels the way Mattermost mentions them, `~name`.
                    let channel = channel.strip_prefix('~').unwrap_or(channel);

                    if is_name(team) && is_name(channel) {
                        return Ok(Self::Channel {
                            team: team.to_ascii_lowercase(),
                            name: channel.to_ascii_lowercase(),
                        });
                    }
                } else if is_id(value) {
                    return Ok(Self::ChannelId(value.to_string()));
                }

                Err(Error::invalid(format!(
                    "{CONNECTOR} group target must be a channel ID or team-name/channel-name"
                )))
            }
            NotifyTargetType::FixedAddress | NotifyTargetType::Webhook => Err(Error::invalid(
                format!("{CONNECTOR} target_type must be direct_user or fixed_group"),
            )),
        }
    }
}

/// A Mattermost ID: 26 lowercase letters and digits.
fn is_id(value: &str) -> bool {
    value.len() == 26
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// A username, team or channel name: letters, digits, `.`, `_` and `-`.
fn is_name(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Requests as the bot, all through one client pinned to the server's addresses.
pub(super) struct Bot {
    http: reqwest::Client,
    api: String,
    token: String,
}

impl Bot {
    pub(super) async fn connect(config: &BotConfig) -> Result<Self> {
        let server = config.server()?;
        let http = pinned_client(&server, Duration::from_secs(config.timeout_secs)).await?;

        Ok(Self {
            http,
            api: format!("{}/api/v4", server.as_str().trim_end_matches('/')),
            token: config.bot_token.trim().to_string(),
        })
    }

    /// The channel a target's posts go to.
    pub(super) async fn channel(&self, target: &Target) -> Result<String> {
        match target {
            Target::Person { name, maybe_id } => {
                let user_id = self.user(name, *maybe_id).await?;
                self.direct_channel(&user_id).await
            }
            Target::ChannelId(id) => Ok(id.clone()),
            Target::Channel { team, name } => {
                let path = format!("/teams/name/{team}/channels/name/{name}");

                match self.find(&path, "finding the channel").await? {
                    Some(channel) => id_of(&channel, "the channel"),
                    None => Err(Error::invalid(format!(
                        "{CONNECTOR} found no channel {team}/{name}: check both names, and add \
                         the bot to the team, and to the channel when it is private"
                    ))),
                }
            }
        }
    }

    /// Posts in a channel; the post's ID.
    pub(super) async fn create_post(
        &self,
        channel_id: &str,
        post: &MattermostPost,
    ) -> Result<Option<String>> {
        let mut body = json!({ "channel_id": channel_id, "message": post.message });

        if !post.attachments.is_empty() {
            body["props"] = post.props();
        }

        let created = self.post("/posts", &body, "sending the message").await?;
        Ok(created["id"].as_str().map(str::to_string))
    }

    /// A person's user ID: by ID first when the name may be one, then by username.
    async fn user(&self, name: &str, maybe_id: bool) -> Result<String> {
        if maybe_id
            && let Some(user) = self
                .find(&format!("/users/{name}"), "finding the user")
                .await?
        {
            return id_of(&user, "the user");
        }

        match self
            .find(&format!("/users/username/{name}"), "finding the user")
            .await?
        {
            Some(user) => id_of(&user, "the user"),
            None if maybe_id => Err(Error::invalid(format!(
                "{CONNECTOR} found no Mattermost user with the ID or username {name}"
            ))),
            None => Err(Error::invalid(format!(
                "{CONNECTOR} found no Mattermost user @{name}"
            ))),
        }
    }

    /// The direct channel between the bot and a person, opened if it is new.
    async fn direct_channel(&self, user_id: &str) -> Result<String> {
        let me = self
            .find("/users/me", "reading the bot's account")
            .await?
            .ok_or_else(|| Error::unavailable(format!("{CONNECTOR} found no bot account")))?;

        let bot_id = id_of(&me, "the bot's account")?;

        let channel = self
            .post(
                "/channels/direct",
                &json!([bot_id, user_id]),
                "opening the direct channel",
            )
            .await?;

        id_of(&channel, "the direct channel")
    }

    /// A resource, or `None` when the server has no such thing.
    async fn find(&self, path: &str, action: &str) -> Result<Option<Value>> {
        let response = self
            .send(self.http.get(format!("{}{path}", self.api)), action)
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        check(response, CONNECTOR, action).await.map(Some)
    }

    async fn post(&self, path: &str, body: &Value, action: &str) -> Result<Value> {
        let response = self
            .send(
                self.http.post(format!("{}{path}", self.api)).json(body),
                action,
            )
            .await?;

        check(response, CONNECTOR, action).await
    }

    async fn send(&self, request: RequestBuilder, action: &str) -> Result<reqwest::Response> {
        http_trace::send(
            &self.http,
            request.bearer_auth(&self.token),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable(format!("{CONNECTOR} request failed while {action}")))
    }
}

fn id_of(value: &Value, what: &str) -> Result<String> {
    value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable(format!("{CONNECTOR} got no ID for {what}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const USER_ID: &str = "q8c3sq9mnfgd5q5ujdh3pywxyh";

    fn target(target_type: NotifyTargetType, value: &str) -> Result<Target> {
        Target::parse(&NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        })
    }

    #[test]
    fn the_server_is_an_http_address_with_a_token() {
        let parsed = BotConfig::parse(&json!({
            "serverUrl": " https://chat.example.com/api/v4/ ",
            "botToken": "token-1",
        }))
        .unwrap();
        assert_eq!(
            parsed.server().unwrap().as_str(),
            "https://chat.example.com/"
        );

        let subpath = BotConfig::parse(&json!({
            "serverUrl": "http://intranet.example.com/mattermost",
            "botToken": "token-1",
            "timeoutSecs": 30,
        }))
        .unwrap();
        assert_eq!(
            subpath.server().unwrap().as_str(),
            "http://intranet.example.com/mattermost"
        );

        for config in [
            json!({ "botToken": "token-1" }),
            json!({ "serverUrl": "chat.example.com", "botToken": "token-1" }),
            json!({ "serverUrl": "ftp://chat.example.com", "botToken": "token-1" }),
            json!({ "serverUrl": "https://chat.example.com?x=1", "botToken": "token-1" }),
            json!({ "serverUrl": "https://chat.example.com" }),
            json!({ "serverUrl": "https://chat.example.com", "botToken": "two words" }),
            json!({ "serverUrl": "https://chat.example.com", "botToken": "t", "timeoutSecs": 0 }),
            json!({ "serverUrl": 7, "botToken": "t" }),
        ] {
            assert!(BotConfig::parse(&config).is_err(), "{config}");
        }
    }

    #[test]
    fn a_person_is_a_username_or_a_user_id() {
        assert_eq!(
            target(NotifyTargetType::DirectUser, " @alice.ops ").unwrap(),
            Target::Person {
                name: "alice.ops".into(),
                maybe_id: false,
            }
        );
        assert_eq!(
            target(NotifyTargetType::DirectUser, USER_ID).unwrap(),
            Target::Person {
                name: USER_ID.into(),
                maybe_id: true,
            }
        );
        assert_eq!(
            target(NotifyTargetType::DirectUser, &format!("@{USER_ID}")).unwrap(),
            Target::Person {
                name: USER_ID.into(),
                maybe_id: false,
            }
        );

        for value in ["", "@", "Alice", "alice smith", "alice/ops", "@@alice"] {
            assert!(
                target(NotifyTargetType::DirectUser, value).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn a_channel_is_an_id_or_a_team_and_a_name() {
        assert_eq!(
            target(NotifyTargetType::FixedGroup, USER_ID).unwrap(),
            Target::ChannelId(USER_ID.into())
        );
        assert_eq!(
            target(NotifyTargetType::FixedGroup, " Ops/~alerts-prod ").unwrap(),
            Target::Channel {
                team: "ops".into(),
                name: "alerts-prod".into(),
            }
        );

        for value in ["alerts", "ops/", "/alerts", "ops/Town Square", "a/b/c"] {
            assert!(
                target(NotifyTargetType::FixedGroup, value).is_err(),
                "{value}"
            );
        }

        assert!(
            target(
                NotifyTargetType::Webhook,
                "https://chat.example.com/hooks/x"
            )
            .is_err()
        );
        assert!(target(NotifyTargetType::FixedAddress, "alice").is_err());
    }
}
