// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mattermost: a bot account ([`bot`], `mattermost_bot`) and a channel's incoming webhook
//! ([`webhook`], `mattermost_webhook`).
//!
//! Both post Slack-compatible message attachments ([`attachment`]). The bot first finds where a
//! post goes through the REST API ([`client`]): a person, by username or user ID, in their direct
//! channel with the bot; or a channel, by ID or `team-name/channel-name`. Only bot posts carry the
//! alert buttons. Each button names the address Mattermost posts its presses to, with the
//! connector's callback token (`routes::agent::im::mattermost` takes them), and Mattermost keeps
//! that address from clients. Webhook posts could not tell who pressed, so they keep only the link
//! to MoleSignal.
//!
//! The server and the webhook are addresses people enter: requests go through `pinned_client`,
//! which refuses cloud metadata addresses and follows no redirect.

mod attachment;
mod bot;
mod client;
mod webhook;

pub(crate) use attachment::{MattermostPost, mattermost_post};
pub use bot::{MATTERMOST_BOT_CONNECTOR_TYPE, MattermostBotConnectorAdapter};
use kernel::{Error, Result};
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::Value;
pub use webhook::{MATTERMOST_WEBHOOK_CONNECTOR_TYPE, MattermostWebhookConnectorAdapter};

use crate::infra::notify::adapters::common::provider_message;

/// The server's answer: its JSON body (`null` when it is not JSON, like a webhook's `ok`), or why
/// it refused, in the server's words. `action` says what was being done.
async fn check(response: reqwest::Response, connector: &str, action: &str) -> Result<Value> {
    let status = response.status();
    let retry = retry_after(response.headers());
    let text = response.text().await.unwrap_or_default();

    if status.is_success() {
        return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
    }

    // Mattermost's errors are JSON with a `message`; a proxy in front of it may answer in text.
    let said = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|body| body["message"].as_str().map(str::to_string))
        .unwrap_or(text);

    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(Error::unavailable(format!(
            "{connector} was rate limited while {action}; retry in {} s: {}",
            retry.unwrap_or(1),
            provider_message(&said)
        )));
    }

    Err(Error::unavailable(format!(
        "{connector} {action} was refused: HTTP {} {}",
        status.as_u16(),
        provider_message(&said)
    )))
}

/// Seconds to wait before trying again: `Retry-After`, or when Mattermost's rate limit resets.
fn retry_after(headers: &HeaderMap) -> Option<u64> {
    ["retry-after", "x-ratelimit-reset"]
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok()?.trim().parse().ok())
}
