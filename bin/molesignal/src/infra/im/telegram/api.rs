// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Telegram Bot API: `POST {apiBaseUrl}/bot{token}/{method}` with a JSON body. Every answer is
//! `{"ok": true, "result": …}`, or `{"ok": false, "error_code": …, "description": …}` with
//! `parameters` saying when to retry or where a group moved.
//!
//! The token is part of the address, so no error carries the request's URL.

use std::time::Duration;

use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use signals::http_trace::{self, HttpTarget};
use url::Url;

use super::html;
use crate::infra::notify::adapters::{TELEGRAM_BOT_CONNECTOR_TYPE, truncate_chars};

/// Where the Bot API is served.
pub(crate) const TELEGRAM_API_BASE_URL: &str = "https://api.telegram.org";

fn default_api_base_url() -> String {
    TELEGRAM_API_BASE_URL.into()
}

fn default_timeout_secs() -> u64 {
    10
}

/// A bot: its token (from @BotFather), the Bot API's address and how long a call may take. Not
/// `Debug`, so the token is never printed.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Bot {
    #[serde(default)]
    bot_token: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl Bot {
    /// The bot of a `telegram_bot` connector, checked: a token shaped like the ones @BotFather
    /// gives, an http(s) address and a timeout between 1 and 60 seconds.
    pub(crate) fn parse(config: &Value) -> Result<Self> {
        let bot: Self = serde_json::from_value(config.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TELEGRAM_BOT_CONNECTOR_TYPE} config: {error}"
            ))
        })?;

        let token = bot.bot_token.trim();

        if token.is_empty() {
            return Err(Error::invalid(format!(
                "{TELEGRAM_BOT_CONNECTOR_TYPE} botToken cannot be empty"
            )));
        }

        let shaped = token.split_once(':').is_some_and(|(id, secret)| {
            !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit())
                && id.parse::<i64>().is_ok()
                && !secret.is_empty()
                && secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        });

        if !shaped {
            return Err(Error::invalid(format!(
                "{TELEGRAM_BOT_CONNECTOR_TYPE} botToken must be the token @BotFather gave, like 123456789:AAE…"
            )));
        }

        let address = Url::parse(bot.api_base_url.trim()).ok();

        if !address
            .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        {
            return Err(Error::invalid(format!(
                "{TELEGRAM_BOT_CONNECTOR_TYPE} apiBaseUrl must be an http(s) URL"
            )));
        }

        if !(1..=60).contains(&bot.timeout_secs) {
            return Err(Error::invalid(format!(
                "{TELEGRAM_BOT_CONNECTOR_TYPE} timeoutSecs must be between 1 and 60"
            )));
        }

        Ok(bot)
    }

    /// The bot's own user ID: the token's first part.
    pub(crate) fn id(&self) -> i64 {
        self.bot_token
            .trim()
            .split_once(':')
            .and_then(|(id, _)| id.parse().ok())
            .unwrap_or_default()
    }

    /// A client whose calls give up after the configured timeout.
    pub(crate) fn client(&self) -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .build()
            .map_err(|_| Error::internal("the Telegram HTTP client could not be initialized"))
    }

    fn url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{method}",
            self.api_base_url.trim().trim_end_matches('/'),
            self.bot_token.trim()
        )
    }

    /// Calls `method` and returns its `result`.
    pub(crate) async fn call(
        &self,
        http: &reqwest::Client,
        method: &str,
        body: &Value,
    ) -> std::result::Result<Value, Failure> {
        self.send(http, http.post(self.url(method)).json(body))
            .await
    }

    /// Like [`Self::call`], waiting up to `timeout` (a long poll outlasts the usual timeout).
    pub(crate) async fn call_within(
        &self,
        http: &reqwest::Client,
        method: &str,
        body: &Value,
        timeout: Duration,
    ) -> std::result::Result<Value, Failure> {
        let request = http.post(self.url(method)).timeout(timeout).json(body);
        self.send(http, request).await
    }

    async fn send(
        &self,
        http: &reqwest::Client,
        request: reqwest::RequestBuilder,
    ) -> std::result::Result<Value, Failure> {
        let response = http_trace::send(http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|error| Failure::Unreachable(error.without_url().to_string()))?;

        let status = response.status().as_u16();

        let text = response
            .text()
            .await
            .map_err(|error| Failure::Unreachable(error.without_url().to_string()))?;

        let Ok(answer) = serde_json::from_str::<Answer>(&text) else {
            return Err(if (200..300).contains(&status) {
                Failure::Unreachable("the answer was not JSON".into())
            } else {
                Failure::Refused(Refusal {
                    code: status,
                    description: format!("HTTP {status}"),
                    retry_after: None,
                    migrate_to: None,
                })
            });
        };

        if answer.ok {
            return Ok(answer.result);
        }

        let parameters = answer.parameters.unwrap_or_default();

        Err(Failure::Refused(Refusal {
            code: answer
                .error_code
                .and_then(|code| u16::try_from(code).ok())
                .unwrap_or(status),
            description: answer.description.unwrap_or_default(),
            retry_after: parameters.retry_after,
            migrate_to: parameters.migrate_to_chat_id,
        }))
    }
}

/// Sends a card (`sendMessage`) or rewrites one in place (`editMessageText`). `target` says where
/// (`chat_id`, and `message_id` for an edit); `card` gives the message's fields, with the links as
/// buttons or (`false`) written into the text. When Telegram refuses a link button (it opens only
/// public http(s) addresses) the links go into the text; when it cannot read the HTML, the text
/// goes plain.
pub(crate) async fn post_card(
    bot: &Bot,
    http: &reqwest::Client,
    method: &str,
    target: &Value,
    card: impl Fn(bool) -> Map<String, Value>,
) -> std::result::Result<Value, Failure> {
    let mut fields = card(true);
    let mut sent = bot.call(http, method, &merged(target, &fields)).await;

    if matches!(&sent, Err(Failure::Refused(refusal)) if refusal.link_button()) {
        fields = card(false);
        sent = bot.call(http, method, &merged(target, &fields)).await;
    }

    if matches!(&sent, Err(Failure::Refused(refusal)) if refusal.unreadable_html()) {
        let text = fields
            .get("text")
            .and_then(Value::as_str)
            .map(html::plain)
            .unwrap_or_default();

        fields.insert("text".into(), Value::String(text));
        fields.remove("parse_mode");
        sent = bot.call(http, method, &merged(target, &fields)).await;
    }

    sent
}

fn merged(target: &Value, fields: &Map<String, Value>) -> Value {
    let mut body = target.as_object().cloned().unwrap_or_default();
    body.extend(fields.clone());
    Value::Object(body)
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error_code: Option<i64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Parameters>,
}

#[derive(Debug, Default, Deserialize)]
struct Parameters {
    #[serde(default)]
    retry_after: Option<u64>,
    #[serde(default)]
    migrate_to_chat_id: Option<i64>,
}

/// Why a call failed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The request did not get through, or its answer could not be read.
    Unreachable(String),
    /// Telegram refused it.
    Refused(Refusal),
}

/// A refusal: Telegram's code (the HTTP status) and description, and the hints that come with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) code: u16,
    pub(crate) description: String,
    /// Seconds to wait, on a rate limit.
    pub(crate) retry_after: Option<u64>,
    /// The group became this supergroup.
    pub(crate) migrate_to: Option<i64>,
}

impl Refusal {
    /// Telegram would not take a link button: it opens only public http(s) addresses.
    pub(crate) fn link_button(&self) -> bool {
        let description = self.description.to_ascii_lowercase();
        self.code == 400 && description.contains("button") && description.contains("url")
    }

    /// Telegram could not read the HTML.
    pub(crate) fn unreadable_html(&self) -> bool {
        self.code == 400 && self.description.contains("can't parse entities")
    }

    /// The message already reads like that (an edit that changes nothing).
    pub(crate) fn unchanged(&self) -> bool {
        self.code == 400 && self.description.contains("message is not modified")
    }

    /// The bot cannot write to this person: they never started it, blocked it, or are unknown.
    pub(crate) fn unreachable_person(&self) -> bool {
        self.code == 403 || (self.code == 400 && self.description.contains("chat not found"))
    }
}

impl Failure {
    pub(crate) fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Refused(refusal) => Some(refusal),
            Self::Unreachable(_) => None,
        }
    }

    /// As an error. A rate limit says when to retry; a refusal that the same request would meet
    /// again (a bad token, a chat the bot cannot reach, a group that moved) is invalid.
    pub(crate) fn error(self, method: &str) -> Error {
        let refusal = match self {
            Self::Unreachable(reason) => {
                return Error::unavailable(format!(
                    "Telegram {method} failed: {}",
                    truncate_chars(&reason, 300)
                ));
            }
            Self::Refused(refusal) => refusal,
        };

        if refusal.code == 429 {
            return Error::unavailable(match refusal.retry_after {
                Some(seconds) => {
                    format!("Telegram {method} was rate limited: retry after {seconds} s")
                }
                None => format!("Telegram {method} was rate limited"),
            });
        }

        let description = match refusal.description.trim() {
            "" => "unknown_error".to_string(),
            description => truncate_chars(description, 300),
        };

        let hint = match (refusal.code, refusal.migrate_to) {
            (_, Some(chat)) => format!(" (the group is now the supergroup {chat})"),
            (401, None) => " (check the bot token)".to_string(),
            (403, None) if description.contains("initiate conversation") => {
                " (the person has to start the bot first)".to_string()
            }
            _ => String::new(),
        };

        let said = format!(
            "Telegram {method} was refused: {} {description}{hint}",
            refusal.code
        );

        if (400..500).contains(&refusal.code) {
            Error::invalid(said)
        } else {
            Error::unavailable(said)
        }
    }
}
