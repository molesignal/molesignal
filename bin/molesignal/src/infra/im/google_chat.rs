// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Chat：Chat 应用把消息发到一个 Pub/Sub 主题，这里从它的订阅里拉（不需要公网回调
//! 地址），回复走 Chat 接口。
//!
//! 服务账号既是 Chat 应用（`chat.bot`），也要能从订阅里拉消息（`pubsub`，订阅者角色）。拉到的
//! 消息先确认再交出去，平台重发的同一条按消息 ID 去重。空间里只收 @ 应用的消息，回复发在原话题
//! 里；群聊里一个人的提问是一个会话；私聊收到每一条。「处理中」是话题里的一条消息，回答好了原地
//! 改成回答。
//!
//! 事件有两种格式：Chat 应用的（顶层 `type` 与 `message`），与做成 Workspace 插件的
//! （`chat.messagePayload`）。
//!
//! 告警卡片上按钮的点击（`CARD_CLICKED`）也从这个订阅里来（[`card_action`]）：没有可以带回结果的
//! 回应，改掉卡片所在的消息，那句话私下发给点的人（[`answer`]）。

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::Listener;
use crate::{
    agent::im::{CardAction, CardAnswer, ConversationKind, Inbound, NoticeKind, Sender},
    domain::notify::buttons::AlertButtonValue,
    infra::notify::adapters::{
        GOOGLE_CHAT_ALERT_FUNCTION, GOOGLE_CHAT_MAX_TEXT_CHARS, GOOGLE_CHAT_SCOPE,
        GOOGLE_PUBSUB_SCOPE, GoogleChatApp, google_chat_card, google_chat_check as check,
        google_chat_text, truncate_chars,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// 一次最多拉这么多条。
const PULL_BATCH: u64 = 10;
/// 拉一次最多等这么久：订阅里没有消息时 Pub/Sub 会等一会儿再回。
const PULL_TIMEOUT: Duration = Duration::from_secs(90);
/// 拉回来是空的且回得很快时，等这么久再拉，免得空转。
const IDLE_WAIT: Duration = Duration::from_secs(1);

fn unreachable(error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("Google Pub/Sub request failed: {error}"))
}

/// 订阅的短名：`projects/p/subscriptions/mole` 是 `mole`。
fn short_name(subscription: &str) -> &str {
    subscription.rsplit('/').next().unwrap_or(subscription)
}

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let app = GoogleChatApp::parse(config)?;
    let subscription = app
        .subscription()
        .ok_or_else(|| Error::invalid("the Google Chat connector has no Pub/Sub subscription"))?
        .to_string();

    let http = reqwest::Client::builder()
        .timeout(PULL_TIMEOUT)
        .build()
        .map_err(|error| Error::internal(format!("Pub/Sub HTTP client: {error}")))?;

    // 订阅在、服务账号读得到：算连上了。
    let token = app.token(&http, GOOGLE_PUBSUB_SCOPE).await?;
    check(
        http_trace::send(
            &http,
            http.get(format!("{}/{subscription}", app.pubsub_base()))
                .bearer_auth(&token),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(unreachable)?,
        "reading the Pub/Sub subscription",
    )
    .await?;

    listener.connected(short_name(&subscription)).await;

    loop {
        let started = Instant::now();

        let pulled = tokio::select! {
            () = cancel.cancelled() => return Ok(()),
            pulled = pull(&app, &http, &subscription) => pulled?,
        };

        if pulled.is_empty() {
            if started.elapsed() < IDLE_WAIT {
                tokio::select! {
                    () = cancel.cancelled() => return Ok(()),
                    () = tokio::time::sleep(IDLE_WAIT) => {}
                }
            }

            continue;
        }

        let ack_ids = pulled
            .iter()
            .filter_map(|received| received["ackId"].as_str())
            .collect::<Vec<_>>();
        acknowledge(&app, &http, &subscription, &ack_ids).await?;

        for received in &pulled {
            let Some(event) = event(&received["message"]) else {
                continue;
            };

            if let Some(action) = card_action(&event) {
                listener.action(action).await;
            } else if let Some(inbound) = inbound(&event) {
                listener.message(inbound).await;
            }
        }
    }
}

/// 拉一批消息；令牌被拒绝时丢掉它，下一轮换新的。
async fn pull(
    app: &GoogleChatApp,
    http: &reqwest::Client,
    subscription: &str,
) -> Result<Vec<Value>> {
    let token = app.token(http, GOOGLE_PUBSUB_SCOPE).await?;
    let response = http_trace::send(
        http,
        http.post(format!("{}/{subscription}:pull", app.pubsub_base()))
            .bearer_auth(&token)
            .json(&json!({ "maxMessages": PULL_BATCH })),
        HttpTarget::ThirdParty,
    )
    .await
    .map_err(unreachable)?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        app.forget_token(GOOGLE_PUBSUB_SCOPE);
    }

    let pulled = check(response, "pulling from the Pub/Sub subscription").await?;

    Ok(pulled["receivedMessages"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

async fn acknowledge(
    app: &GoogleChatApp,
    http: &reqwest::Client,
    subscription: &str,
    ack_ids: &[&str],
) -> Result<()> {
    let token = app.token(http, GOOGLE_PUBSUB_SCOPE).await?;

    check(
        http_trace::send(
            http,
            http.post(format!("{}/{subscription}:acknowledge", app.pubsub_base()))
                .bearer_auth(&token)
                .json(&json!({ "ackIds": ack_ids })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(unreachable)?,
        "acknowledging Pub/Sub messages",
    )
    .await
    .map(|_| ())
}

/// Pub/Sub 消息里的 Chat 事件（`data` 是 base64 的 JSON）。
fn event(message: &Value) -> Option<Value> {
    let data = STANDARD.decode(message["data"].as_str()?).ok()?;
    serde_json::from_slice(&data).ok()
}

/// 发给应用的一条消息。只认人发的文字。
pub(crate) fn inbound(event: &Value) -> Option<Inbound> {
    let (message, space) = match event["chat"].get("messagePayload") {
        Some(payload) => (&payload["message"], &payload["space"]),
        None if event["type"] == "MESSAGE" => (&event["message"], &event["space"]),
        None => return None,
    };
    let space = if space.is_object() {
        space
    } else {
        &message["space"]
    };

    let sender = &message["sender"];

    if sender["type"] == "BOT" {
        return None;
    }

    let message_name = message["name"].as_str()?.to_string();
    let space_name = space["name"].as_str()?.to_string();
    let user = sender["name"].as_str()?.to_string();

    // 去掉了对应用的 @ 的正文；没有时用原文。
    let text = message["argumentText"]
        .as_str()
        .or_else(|| message["text"].as_str())?
        .trim()
        .to_string();

    let kind = space["spaceType"]
        .as_str()
        .or_else(|| space["type"].as_str());
    let direct = matches!(kind, Some("DIRECT_MESSAGE" | "DM")) || space["singleUserBotDm"] == true;
    let thread = message["thread"]["name"].as_str();

    let thread_key = match (direct, kind) {
        (true, _) => String::new(),
        (false, Some("GROUP_CHAT")) => format!("user:{user}"),
        (false, _) => thread.unwrap_or(&message_name).to_string(),
    };

    let mut ids = vec![user.clone()];
    ids.extend(user.strip_prefix("users/").map(str::to_string));
    ids.extend(sender["email"].as_str().map(str::to_string));

    Some(Inbound {
        message_id: message_name,
        conversation_id: space_name.clone(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key,
        sender: Sender {
            ids,
            name: sender["displayName"].as_str().map(str::to_string),
        },
        text,
        addressed: true,
        reply: json!({ "space": space_name, "thread": thread, "direct": direct }),
    })
}

/// 点了告警卡片上的按钮：Chat 应用的 `CARD_CLICKED`，或做成 Workspace 插件的
/// `chat.buttonClickedPayload`。调的不是告警按钮的函数时不算。
pub(crate) fn card_action(event: &Value) -> Option<CardAction> {
    let (user, message, space, raw) = match event["chat"].get("buttonClickedPayload") {
        Some(payload) => (
            &event["chat"]["user"],
            &payload["message"],
            &payload["space"],
            event["commonEventObject"]["parameters"][GOOGLE_CHAT_ALERT_FUNCTION].as_str(),
        ),
        None if event["type"] == "CARD_CLICKED" => {
            let function = event["common"]["invokedFunction"]
                .as_str()
                .or_else(|| event["action"]["actionMethodName"].as_str());

            if function != Some(GOOGLE_CHAT_ALERT_FUNCTION) {
                return None;
            }

            let raw = event["common"]["parameters"][GOOGLE_CHAT_ALERT_FUNCTION]
                .as_str()
                .or_else(|| {
                    event["action"]["parameters"]
                        .as_array()?
                        .iter()
                        .find(|parameter| parameter["key"] == GOOGLE_CHAT_ALERT_FUNCTION)?["value"]
                        .as_str()
                });

            (&event["user"], &event["message"], &event["space"], raw)
        }
        None => return None,
    };

    let value: AlertButtonValue = serde_json::from_str(raw?).ok()?;
    let space = if space.is_object() {
        space
    } else {
        &message["space"]
    };

    let message_name = message["name"].as_str()?;
    let space_name = space["name"].as_str()?;
    let user_name = user["name"].as_str()?;

    let kind = space["spaceType"]
        .as_str()
        .or_else(|| space["type"].as_str());
    let direct = matches!(kind, Some("DIRECT_MESSAGE" | "DM")) || space["singleUserBotDm"] == true;
    let thread = message["thread"]["name"].as_str();

    let mut ids = vec![user_name.to_string()];
    ids.extend(user_name.strip_prefix("users/").map(str::to_string));
    ids.extend(user["email"].as_str().map(str::to_string));

    Some(CardAction {
        value,
        sender: Sender {
            ids,
            name: user["displayName"].as_str().map(str::to_string),
        },
        conversation_id: space_name.to_string(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        reply: json!({ "space": space_name, "thread": thread, "direct": direct }),
        card: json!({ "message": message_name, "user": user_name }),
    })
}

/// 点击的结果：改掉卡片所在的消息，那句话私下发给点的人（做成了的不用说，卡片上写着）。
pub(super) async fn answer(config: &Value, action: &CardAction, answer: &CardAnswer) -> Result<()> {
    if let Some(card) = &answer.card {
        let name = action.card["message"]
            .as_str()
            .filter(|name| name.starts_with("spaces/"))
            .ok_or_else(|| Error::invalid("the Google Chat card to update is unknown"))?;

        let app = GoogleChatApp::parse(config)?;
        let http = app.http()?;
        let token = app.token(&http, GOOGLE_CHAT_SCOPE).await?;
        let replacement = google_chat_card(&card.message, &card.buttons, card.note.as_deref());

        check(
            http_trace::send(
                &http,
                http.patch(format!("{}/{name}?updateMask=cardsV2", app.chat_base()))
                    .bearer_auth(&token)
                    .json(&json!({ "cardsV2": replacement["cardsV2"] })),
                HttpTarget::ThirdParty,
            )
            .await
            .map_err(|_| Error::unavailable("google_chat_app request failed"))?,
            "updating the alert card",
        )
        .await?;
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
    {
        let (space, thread) = place(&action.reply)?;
        let viewer = action.card["user"].as_str();

        post(config, space, thread, text, viewer).await?;
    }

    Ok(())
}

fn chat_text(markdown: &str) -> String {
    truncate_chars(
        &google_chat_text(markdown.trim()),
        GOOGLE_CHAT_MAX_TEXT_CHARS,
    )
}

/// 发一条消息到 `space`，在 `thread` 里（有的话）；有 `viewer` 时只有这个人看得到。返回消息的资源名。
async fn post(
    config: &Value,
    space: &str,
    thread: Option<&str>,
    markdown: &str,
    viewer: Option<&str>,
) -> Result<String> {
    let app = GoogleChatApp::parse(config)?;
    let http = app.http()?;
    let token = app.token(&http, GOOGLE_CHAT_SCOPE).await?;

    let mut body = json!({ "text": chat_text(markdown) });
    let mut url = format!("{}/{space}/messages", app.chat_base());

    if let Some(viewer) = viewer {
        body["privateMessageViewer"] = json!({ "name": viewer });
    }

    if let Some(thread) = thread {
        body["thread"] = json!({ "name": thread });
        url.push_str("?messageReplyOption=REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD");
    }

    let sent = check(
        http_trace::send(
            &http,
            http.post(url).bearer_auth(&token).json(&body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("google_chat_app request failed"))?,
        "sending the message",
    )
    .await?;

    Ok(sent["name"].as_str().unwrap_or_default().to_string())
}

fn place(reply: &Value) -> Result<(&str, Option<&str>)> {
    let space = reply["space"]
        .as_str()
        .ok_or_else(|| Error::invalid("the Google Chat space to answer in is unknown"))?;

    Ok((space, reply["thread"].as_str()))
}

/// 回复到原处：话题里或私聊里。
pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let (space, thread) = place(reply)?;
    post(config, space, thread, markdown, None)
        .await
        .map(|_| ())
}

/// 「处理中」：话题里的一条消息，返回它的资源名。
pub(super) async fn react(config: &Value, reply: &Value, working: &str) -> Result<String> {
    let (space, thread) = place(reply)?;
    post(config, space, thread, working, None).await
}

/// 把「处理中」的消息改成回答；改不了就另发一条。
pub(super) async fn finish(
    config: &Value,
    reply: &Value,
    working: Option<&str>,
    markdown: &str,
) -> Result<()> {
    if let Some(name) = working.filter(|name| name.starts_with("spaces/")) {
        let app = GoogleChatApp::parse(config)?;
        let http = app.http()?;
        let token = app.token(&http, GOOGLE_CHAT_SCOPE).await?;

        let updated = http_trace::send(
            &http,
            http.patch(format!("{}/{name}?updateMask=text", app.chat_base()))
                .bearer_auth(&token)
                .json(&json!({ "text": chat_text(markdown) })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("google_chat_app request failed"));

        match updated {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => {
                tracing::debug!(status = %response.status(), "the working message was not updated");
            }
            Err(error) => tracing::debug!(%error, "the working message was not updated"),
        }
    }

    self::reply(config, reply, markdown).await
}

/// 私聊发给一个人：他要先给应用发过消息或装了应用。
pub(super) async fn tell(config: &Value, user: &str, markdown: &str) -> Result<()> {
    let app = GoogleChatApp::parse(config)?;
    let http = app.http()?;
    let token = app.token(&http, GOOGLE_CHAT_SCOPE).await?;
    let space = app.direct_message(&http, &token, user).await?;

    post(config, &space, None, markdown, None).await.map(|_| ())
}

#[cfg(test)]
mod tests;
