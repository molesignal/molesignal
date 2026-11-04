// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microsoft Teams：Bot Framework 把发给 Bot 的消息 POST 到 MoleSignal 的公网回调地址
//! （`http.external_url` 下的 `/api/v1/public/im/teams/{connector_id}/messages`，路由在
//! `api::http::routes::agent::im`），请求先按 [`auth`] 验过；回复走消息带来的 Bot Connector 地址。
//!
//! 没有长连接：[`listen`] 只确认 Bot 的凭据换得到令牌，之后等消息从回调进来。频道里只收 @ Bot 的
//! 消息，回复发在原帖子下；群聊没有话题，一个人的提问是一个会话；私聊收到每一条。「处理中」是
//! 一条回复，回答好了原地改成回答。
//!
//! 告警卡片上的按钮是 `Action.Execute`：点了 Teams 把 `adaptiveCard/action` 也 POST 到这个回调
//! （[`card_action`]），回应里换上的卡片只换点的人眼前的那张（[`invoke_response`]），再按 Bot
//! Connector 换掉原消息，会话里每个人都看得到（[`answer`]）。

pub(crate) mod auth;

use reqwest::Method;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::Listener;
use crate::{
    agent::im::{CardAction, CardAnswer, ConversationKind, Inbound, Sender},
    domain::notify::buttons::AlertButtonValue,
    infra::notify::adapters::{
        TEAMS_ALERT_VALUE_KEY, TEAMS_ALERT_VERB, TEAMS_MAX_TEXT_CHARS, TeamsApp, teams_card,
        teams_card_message, teams_encode, truncate_chars,
    },
    shared::{Error, Result},
};

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let app = TeamsApp::parse(config)?;
    app.token(&app.http()?).await?;

    listener.connected(app.app_id()).await;
    cancel.cancelled().await;
    Ok(())
}

/// 频道的会话 ID 带着帖子：`19:…@thread.tacv2;messageid=1700000000000`。
fn channel_and_post(conversation: &str) -> (&str, Option<&str>) {
    match conversation.split_once(";messageid=") {
        Some((channel, post)) => (channel, Some(post)),
        None => (conversation, None),
    }
}

/// 解开 `&amp;` 之类的字符实体（Teams 的正文是带标签的文字）。
fn unescape(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 去掉标签，`<br>` 与段落换成换行。
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);

        let Some(close) = rest[open..].find('>').map(|at| open + at) else {
            out.push_str(&rest[open..]);
            return out;
        };

        let tag = rest[open + 1..close].trim().to_ascii_lowercase();

        if tag.starts_with("br") || tag == "/p" || tag == "/div" {
            out.push('\n');
        }

        rest = &rest[close + 1..];
    }

    out.push_str(rest);
    out
}

/// 消息的文字：去掉对 Bot 的 @，别人的 @ 写成 `@名字`，再去掉标签。
fn text_of(activity: &Value, bot_id: &str) -> String {
    let mut text = activity["text"].as_str().unwrap_or_default().to_string();

    for entity in activity["entities"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if entity["type"] != "mention" {
            continue;
        }

        let Some(mention) = entity["text"].as_str() else {
            continue;
        };

        if entity["mentioned"]["id"].as_str() == Some(bot_id) {
            text = text.replace(mention, "");
        } else if let Some(name) = entity["mentioned"]["name"].as_str() {
            text = text.replace(mention, &format!("@{name}"));
        }
    }

    unescape(&strip_tags(&text)).trim().to_string()
}

/// 发给 Bot 的一条消息。只认人发的文字：卡片的提交（没有 `text`）不算。
pub(crate) fn inbound(activity: &Value) -> Option<Inbound> {
    if activity["type"] != "message" || activity["text"].as_str().is_none() {
        return None;
    }

    let activity_id = activity["id"].as_str()?.to_string();
    let conversation = activity["conversation"]["id"].as_str()?.to_string();
    let service_url = activity["serviceUrl"].as_str()?.to_string();
    let bot_id = activity["recipient"]["id"].as_str().unwrap_or_default();
    let from = &activity["from"];
    let user = from["id"].as_str()?.to_string();
    let tenant = activity["conversation"]["tenantId"]
        .as_str()
        .or_else(|| activity["channelData"]["tenant"]["id"].as_str());

    let kind = activity["conversation"]["conversationType"]
        .as_str()
        .unwrap_or("personal");
    let direct = kind == "personal";
    let (channel, post) = channel_and_post(&conversation);

    let thread_key = match kind {
        "personal" => String::new(),
        "channel" => post.unwrap_or(&activity_id).to_string(),
        _ => format!("user:{user}"),
    };

    let mut ids = vec![user.clone()];
    ids.extend(from["aadObjectId"].as_str().map(str::to_string));

    Some(Inbound {
        // 消息 ID 只在会话里唯一：带上会话（频道里不带帖子，免得太长）。
        message_id: format!("{channel}:{activity_id}"),
        conversation_id: channel.to_string(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key,
        sender: Sender {
            ids,
            name: from["name"].as_str().map(str::to_string),
        },
        text: text_of(activity, bot_id),
        addressed: true,
        reply: json!({
            "service_url": service_url,
            "conversation": conversation,
            "activity_id": activity_id,
            "tenant": tenant,
        }),
    })
}

/// 点了告警卡片上的按钮：`Action.Execute` 发来的 `adaptiveCard/action`，或只认得 `Action.Submit` 的旧
/// 客户端发来的一条带 `value` 的消息。值不是告警按钮的不算。
pub(crate) fn card_action(activity: &Value) -> Option<CardAction> {
    let data = match activity["type"].as_str()? {
        "invoke"
            if activity["name"] == "adaptiveCard/action"
                && activity["value"]["action"]["verb"] == TEAMS_ALERT_VERB =>
        {
            &activity["value"]["action"]["data"]
        }
        "message" => &activity["value"],
        _ => return None,
    };

    let value: AlertButtonValue =
        serde_json::from_value(data[TEAMS_ALERT_VALUE_KEY].clone()).ok()?;
    let conversation = activity["conversation"]["id"].as_str()?;
    let service_url = activity["serviceUrl"].as_str()?;
    // 卡片所在的那条消息。
    let card = activity["replyToId"].as_str().filter(|id| !id.is_empty())?;
    let from = &activity["from"];
    let user = from["id"].as_str()?.to_string();
    let tenant = activity["conversation"]["tenantId"]
        .as_str()
        .or_else(|| activity["channelData"]["tenant"]["id"].as_str());

    let direct = activity["conversation"]["conversationType"]
        .as_str()
        .is_none_or(|kind| kind == "personal");
    let (channel, _) = channel_and_post(conversation);

    let mut ids = vec![user];
    ids.extend(from["aadObjectId"].as_str().map(str::to_string));

    Some(CardAction {
        value,
        sender: Sender {
            ids,
            name: from["name"].as_str().map(str::to_string),
        },
        conversation_id: channel.to_string(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        reply: json!({
            "service_url": service_url,
            "conversation": conversation,
            "activity_id": card,
            "tenant": tenant,
        }),
        card: json!({
            "service_url": service_url,
            "conversation": conversation,
            "activity_id": card,
        }),
    })
}

/// `adaptiveCard/action` 的回应：换上的卡片，或给点的人的一句话；还没有结果时什么都不换。
pub(crate) fn invoke_response(answer: Option<&CardAnswer>) -> Value {
    match answer {
        Some(CardAnswer {
            card: Some(card), ..
        }) => json!({
            "statusCode": 200,
            "type": "application/vnd.microsoft.card.adaptive",
            "value": teams_card(&card.message, &card.buttons, card.note.as_deref()),
        }),
        Some(CardAnswer {
            notice: Some((_, text)),
            ..
        }) => json!({
            "statusCode": 200,
            "type": "application/vnd.microsoft.activity.message",
            "value": text,
        }),
        _ => json!({ "statusCode": 200 }),
    }
}

/// 按 Bot Connector 换掉卡片所在的消息，会话里每个人都看得到。给点的人的那句话已经在回应里了。
pub(super) async fn answer(config: &Value, action: &CardAction, answer: &CardAnswer) -> Result<()> {
    let Some(card) = &answer.card else {
        return Ok(());
    };

    let (service, conversation, activity) = place(&action.card)?;
    let app = TeamsApp::parse(config)?;
    let http = app.http()?;
    let token = app.token(&http).await?;

    let mut body = teams_card_message(&card.message, &card.buttons, card.note.as_deref());
    body["id"] = json!(activity);

    app.call(
        &http,
        &token,
        Method::PUT,
        service,
        &format!(
            "/v3/conversations/{}/activities/{}",
            teams_encode(conversation),
            teams_encode(activity)
        ),
        &body,
    )
    .await
    .map(|_| ())
}

fn message(markdown: &str) -> Value {
    json!({
        "type": "message",
        "textFormat": "markdown",
        "text": truncate_chars(markdown.trim(), TEAMS_MAX_TEXT_CHARS),
    })
}

/// 回复要的：Bot Connector 地址、会话与要回复的消息。
fn place(reply: &Value) -> Result<(&str, &str, &str)> {
    match (
        reply["service_url"].as_str(),
        reply["conversation"].as_str(),
        reply["activity_id"].as_str(),
    ) {
        (Some(service), Some(conversation), Some(activity)) => {
            Ok((service, conversation, activity))
        }
        _ => Err(Error::invalid("the Teams message to answer is unknown")),
    }
}

/// 回复那条消息（频道里在原帖子下），返回回复的 ID。
async fn respond(config: &Value, reply: &Value, markdown: &str) -> Result<String> {
    let (service, conversation, activity) = place(reply)?;
    let app = TeamsApp::parse(config)?;
    let http = app.http()?;
    let token = app.token(&http).await?;

    let sent = app
        .call(
            &http,
            &token,
            Method::POST,
            service,
            &format!(
                "/v3/conversations/{}/activities/{}",
                teams_encode(conversation),
                teams_encode(activity)
            ),
            &message(markdown),
        )
        .await?;

    Ok(sent.id.unwrap_or_default())
}

pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    respond(config, reply, markdown).await.map(|_| ())
}

/// 「处理中」：一条回复，返回它的 ID。
pub(super) async fn react(config: &Value, reply: &Value, working: &str) -> Result<String> {
    let id = respond(config, reply, working).await?;

    if id.is_empty() {
        return Err(Error::unavailable(
            "Teams did not return the working message",
        ));
    }

    Ok(id)
}

/// 把「处理中」的回复改成回答；改不了就另回复一条。
pub(super) async fn finish(
    config: &Value,
    reply: &Value,
    working: Option<&str>,
    markdown: &str,
) -> Result<()> {
    if let Some(working) = working.filter(|working| !working.is_empty()) {
        let (service, conversation, _) = place(reply)?;
        let app = TeamsApp::parse(config)?;
        let http = app.http()?;
        let token = app.token(&http).await?;

        let mut body = message(markdown);
        body["id"] = json!(working);

        let updated = app
            .call(
                &http,
                &token,
                Method::PUT,
                service,
                &format!(
                    "/v3/conversations/{}/activities/{}",
                    teams_encode(conversation),
                    teams_encode(working)
                ),
                &body,
            )
            .await;

        match updated {
            Ok(_) => return Ok(()),
            Err(error) => tracing::debug!(%error, "the working message was not updated"),
        }
    }

    self::reply(config, reply, markdown).await
}

/// 私聊发给一个人：Bot 要已经装给了他（个人范围）。
pub(super) async fn tell(config: &Value, reply: &Value, user: &str, markdown: &str) -> Result<()> {
    let (service, _, _) = place(reply)?;
    let app = TeamsApp::parse(config)?;
    let http = app.http()?;
    let token = app.token(&http).await?;
    let conversation = app
        .personal_conversation(&http, &token, service, user)
        .await?;

    app.call(
        &http,
        &token,
        Method::POST,
        service,
        &format!(
            "/v3/conversations/{}/activities",
            teams_encode(&conversation)
        ),
        &message(markdown),
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests;
