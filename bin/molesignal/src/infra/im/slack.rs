// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Slack：Socket Mode 收事件，Web API 回复。
//!
//! 用应用级令牌（`xapp-`）开一条 Socket Mode 连接；每个信封先回 `envelope_id` 确认，再处理里面
//! 的事件。`app_mention` 是频道里 @ 机器人；`message` 是私聊，或频道里已有话题的回复（与
//! `app_mention` 是同一条消息时按消息去重）。回答用 Markdown 块发到话题里。
//!
//! 点了告警卡片上的按钮是 `interactive` 信封里的 `block_actions`：处理完用它的 `response_url`
//! 换掉卡片，再发一条只有点的人看得到的消息。

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Instant, interval_at};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::{Listener, ws};
use crate::{
    agent::im::{CardAction, CardAnswer, ConversationKind, Inbound, NoticeKind, Sender},
    domain::notify::buttons::AlertButtonValue,
    infra::notify::adapters::{
        SLACK_ALERT_BLOCK_ID, SLACK_SITES, mrkdwn, slack_card, truncate_chars,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// 多久发一次 WebSocket ping；这么久的三倍没听到任何东西就重连。
const PING: Duration = Duration::from_secs(30);
/// Markdown 块最多这么多字符。
const MAX_MARKDOWN_CHARS: usize = 11_000;
const WORKING_EMOJI: &str = "eyes";

fn default_api_base_url() -> String {
    SLACK_SITES[0].1.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct App {
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    app_token: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
}

impl App {
    fn parse(config: &Value) -> Result<Self> {
        let app: Self = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid slack_app config: {error}")))?;

        if app.bot_token.trim().is_empty() {
            return Err(Error::invalid("the Slack app needs a bot token to answer"));
        }

        Ok(app)
    }

    fn api(&self) -> &str {
        self.api_base_url.trim().trim_end_matches('/')
    }

    async fn call(
        &self,
        http: &reqwest::Client,
        method: &str,
        token: &str,
        body: &Value,
    ) -> Result<Value> {
        let response = http_trace::send(
            http,
            http.post(format!("{}/{method}", self.api()))
                .bearer_auth(token.trim())
                .json(body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|error| Error::unavailable(format!("Slack {method} failed: {error}")))?;

        let body: Value = response.json().await.map_err(|_| {
            Error::unavailable(format!("Slack {method} returned an invalid response"))
        })?;

        if body["ok"].as_bool() == Some(true) {
            Ok(body)
        } else {
            Err(Error::unavailable(format!(
                "Slack {method} was refused: {}",
                body["error"].as_str().unwrap_or("unknown_error")
            )))
        }
    }

    async fn bot(&self, http: &reqwest::Client, method: &str, body: &Value) -> Result<Value> {
        self.call(http, method, &self.bot_token, body).await
    }
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Error::internal("Slack HTTP client could not be initialized"))
}

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let app = App::parse(config)?;

    if !app.app_token.trim().starts_with("xapp-") {
        return Err(Error::invalid(
            "the Slack app needs an app-level token (xapp-…) with connections:write for Socket Mode",
        ));
    }

    let http = http()?;
    let identity = app.bot(&http, "auth.test", &json!({})).await?;
    let bot_user = identity["user_id"].as_str().unwrap_or_default().to_string();
    let bot_name = identity["user"].as_str().unwrap_or_default().to_string();

    let opened = app
        .call(&http, "apps.connections.open", &app.app_token, &json!({}))
        .await?;
    let url = opened["url"]
        .as_str()
        .ok_or_else(|| Error::unavailable("Slack returned no Socket Mode address"))?;

    let mut socket = ws::connect(url).await?;
    let mut ticker = interval_at(Instant::now() + PING, PING);
    let mut heard = Instant::now();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = socket.close(None).await;
                return Ok(());
            }
            _ = ticker.tick() => {
                if heard.elapsed() > PING * 3 {
                    return Err(Error::unavailable("Slack stopped answering"));
                }

                socket
                    .send(Message::Ping(Vec::new().into()))
                    .await
                    .map_err(|error| Error::unavailable(format!("Slack connection lost: {error}")))?;
            }
            message = socket.next() => {
                let Some(message) = message else {
                    return Ok(());
                };

                let message = message
                    .map_err(|error| Error::unavailable(format!("Slack connection lost: {error}")))?;
                heard = Instant::now();

                let envelope: Value = match message {
                    Message::Text(text) => match serde_json::from_str(&text) {
                        Ok(envelope) => envelope,
                        Err(_) => continue,
                    },
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };

                match envelope["type"].as_str() {
                    Some("hello") => listener.connected(&bot_name).await,
                    Some("disconnect") => return Ok(()),
                    _ => {}
                }

                if let Some(id) = envelope["envelope_id"].as_str() {
                    socket
                        .send(Message::Text(json!({ "envelope_id": id }).to_string().into()))
                        .await
                        .map_err(|error| Error::unavailable(format!("Slack connection lost: {error}")))?;
                }

                if envelope["type"] == "events_api"
                    && let Some(inbound) = inbound(&envelope["payload"]["event"], &bot_user)
                {
                    listener.message(inbound).await;
                }

                if envelope["type"] == "interactive"
                    && let Some(action) = card_action(&envelope["payload"])
                {
                    listener.action(action).await;
                }
            }
        }
    }
}

/// Slack 文本里的转义。
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// 一个事件里的用户消息。机器人自己的、编辑与删除之类带 `subtype` 的都不算。
pub(crate) fn inbound(event: &Value, bot_user: &str) -> Option<Inbound> {
    let kind = event["type"].as_str()?;

    if !matches!(kind, "app_mention" | "message")
        || event
            .get("subtype")
            .is_some_and(|subtype| !subtype.is_null())
        || event.get("bot_id").is_some_and(|bot| !bot.is_null())
    {
        return None;
    }

    let user = event["user"].as_str().filter(|user| !user.is_empty())?;

    if user == bot_user {
        return None;
    }

    let channel = event["channel"].as_str()?.to_string();
    let ts = event["ts"].as_str()?.to_string();
    let thread_ts = event["thread_ts"].as_str().filter(|ts| !ts.is_empty());
    let direct = event["channel_type"].as_str() == Some("im") || channel.starts_with('D');

    let mention = format!("<@{bot_user}>");
    let raw = event["text"].as_str().unwrap_or_default();
    let mentioned = kind == "app_mention" || (!bot_user.is_empty() && raw.contains(&mention));
    let text = unescape(&raw.replace(&mention, "")).trim().to_string();

    // 频道里没 @ 也不在话题里的消息不是给机器人的。
    if !direct && !mentioned && thread_ts.is_none() {
        return None;
    }

    let thread_key = if direct {
        String::new()
    } else {
        thread_ts.unwrap_or(&ts).to_string()
    };

    let reply_thread = if direct {
        thread_ts.map(str::to_string)
    } else {
        Some(thread_key.clone())
    };

    Some(Inbound {
        message_id: format!("{channel}:{ts}"),
        conversation_id: channel.clone(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key,
        sender: Sender {
            ids: vec![user.to_string()],
            name: None,
        },
        text,
        addressed: direct || mentioned,
        reply: json!({ "channel": channel, "ts": ts, "thread_ts": reply_thread }),
    })
}

/// 点了告警卡片上的按钮（`block_actions`）。链接按钮没有值，不算。
pub(crate) fn card_action(payload: &Value) -> Option<CardAction> {
    if payload["type"] != "block_actions" {
        return None;
    }

    let pressed = payload["actions"]
        .as_array()?
        .iter()
        .find(|action| action["block_id"] == SLACK_ALERT_BLOCK_ID && action["value"].is_string())?;
    let value: AlertButtonValue = serde_json::from_str(pressed["value"].as_str()?).ok()?;

    let user = payload["user"]["id"]
        .as_str()
        .filter(|user| !user.is_empty())?;
    let channel = payload["channel"]["id"]
        .as_str()
        .or_else(|| payload["container"]["channel_id"].as_str())
        .filter(|channel| !channel.is_empty())?;
    let ts = payload["container"]["message_ts"]
        .as_str()
        .or_else(|| payload["message"]["ts"].as_str())?;
    let thread_ts = payload["message"]["thread_ts"]
        .as_str()
        .filter(|ts| !ts.is_empty());

    Some(CardAction {
        value,
        sender: Sender {
            ids: vec![user.to_string()],
            name: None,
        },
        conversation_id: channel.to_string(),
        kind: if channel.starts_with('D') {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        reply: json!({ "channel": channel, "ts": ts, "thread_ts": thread_ts }),
        card: json!({ "response_url": payload["response_url"] }),
    })
}

/// 点击的结果：用 `response_url` 换掉卡片，那句话发成只有点的人看得到的消息（做成了的不用说，
/// 卡片上写着）。`response_url` 不用令牌。
pub(super) async fn answer(
    _config: &Value,
    action: &CardAction,
    answer: &CardAnswer,
) -> Result<()> {
    let url = action.card["response_url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| Error::invalid("the Slack action has no response URL"))?;
    let http = http()?;

    if let Some(card) = &answer.card {
        let mut replacement = json!({
            "replace_original": true,
            "text": card.message.title,
        });

        for (key, value) in slack_card(&card.message, &card.buttons, card.note.as_deref()) {
            replacement[key] = value;
        }

        respond(&http, url, &replacement).await?;
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
    {
        respond(
            &http,
            url,
            &json!({
                "response_type": "ephemeral",
                "replace_original": false,
                "text": mrkdwn(text),
            }),
        )
        .await?;
    }

    Ok(())
}

async fn respond(http: &reqwest::Client, url: &str, body: &Value) -> Result<()> {
    let response = http_trace::send(http, http.post(url).json(body), HttpTarget::ThirdParty)
        .await
        .map_err(|error| Error::unavailable(format!("Slack response_url failed: {error}")))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(Error::unavailable(format!(
            "Slack response_url returned HTTP {}",
            response.status().as_u16()
        )))
    }
}

pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;
    let markdown = truncate_chars(markdown, MAX_MARKDOWN_CHARS);
    let channel = reply["channel"].as_str().unwrap_or_default();
    let thread = reply["thread_ts"].clone();

    let with_block = app
        .bot(
            &http,
            "chat.postMessage",
            &json!({
                "channel": channel,
                "thread_ts": thread,
                "text": mrkdwn(&markdown),
                "blocks": [{ "type": "markdown", "text": markdown }],
                "unfurl_links": false,
            }),
        )
        .await;

    match with_block {
        Ok(_) => Ok(()),
        // 不认 Markdown 块的工作区：只发 mrkdwn 文本。
        Err(error) if error.to_string().contains("invalid_blocks") => app
            .bot(
                &http,
                "chat.postMessage",
                &json!({
                    "channel": channel,
                    "thread_ts": thread,
                    "text": mrkdwn(&markdown),
                    "unfurl_links": false,
                }),
            )
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    }
}

pub(super) async fn react(config: &Value, reply: &Value) -> Result<String> {
    let app = App::parse(config)?;
    let http = http()?;

    app.bot(
        &http,
        "reactions.add",
        &json!({
            "channel": reply["channel"],
            "timestamp": reply["ts"],
            "name": WORKING_EMOJI,
        }),
    )
    .await?;

    Ok(WORKING_EMOJI.to_string())
}

pub(super) async fn unreact(config: &Value, reply: &Value, reaction: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;

    app.bot(
        &http,
        "reactions.remove",
        &json!({ "channel": reply["channel"], "timestamp": reply["ts"], "name": reaction }),
    )
    .await
    .map(|_| ())
}

/// 只有发消息的人看得到：频道里发一条只给他看的消息，私聊里照常回复。
pub(super) async fn tell(config: &Value, reply: &Value, user: &str, markdown: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;
    let channel = reply["channel"].as_str().unwrap_or_default();

    if channel.starts_with('D') {
        return self::reply(config, reply, markdown).await;
    }

    app.bot(
        &http,
        "chat.postEphemeral",
        &json!({
            "channel": channel,
            "user": user,
            "thread_ts": reply["thread_ts"],
            "text": mrkdwn(markdown),
        }),
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests;
