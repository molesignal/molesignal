// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 飞书开放接口：回复到话题里（卡片里的 Markdown，发不出去退回纯文本）、「处理中」的表情、私聊，
//! 以及来不及在确认里带回去的点击结果。

use serde_json::{Value, json};

use super::{App, MAX_MARKDOWN_CHARS, WORKING_EMOJI, http, ok, send};
use crate::{
    agent::im::{CardAction, CardAnswer},
    infra::notify::adapters::{lark_card, truncate_chars},
    shared::{Error, Result},
};

fn card(markdown: &str) -> String {
    json!({
        "schema": "2.0",
        "body": {
            "elements": [{
                "tag": "markdown",
                "content": truncate_chars(markdown, MAX_MARKDOWN_CHARS),
            }],
        },
    })
    .to_string()
}

pub(crate) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;
    let token = app.token(&http).await?;
    let message_id = reply["message_id"]
        .as_str()
        .ok_or_else(|| Error::invalid("the Feishu reply has no message"))?;
    let in_thread = reply["in_thread"].as_bool().unwrap_or(false);
    let url = format!("{}/im/v1/messages/{message_id}/reply", app.api());

    let sent = send(
        &http,
        http.post(&url).bearer_auth(&token).json(&json!({
            "msg_type": "interactive",
            "content": card(markdown),
            "reply_in_thread": in_thread,
        })),
    )
    .await?;

    if sent["code"].as_i64() == Some(0) {
        return Ok(());
    }

    // 卡片发不出去时退回纯文本。
    let text = json!({ "text": markdown }).to_string();

    ok(
        send(
            &http,
            http.post(&url).bearer_auth(&token).json(&json!({
                "msg_type": "text",
                "content": text,
                "reply_in_thread": in_thread,
            })),
        )
        .await?,
        "the reply",
    )
    .map(|_| ())
}

pub(crate) async fn react(config: &Value, reply: &Value) -> Result<String> {
    let app = App::parse(config)?;
    let http = http()?;
    let token = app.token(&http).await?;
    let message_id = reply["message_id"]
        .as_str()
        .ok_or_else(|| Error::invalid("the Feishu reply has no message"))?;

    let body = ok(
        send(
            &http,
            http.post(format!(
                "{}/im/v1/messages/{message_id}/reactions",
                app.api()
            ))
            .bearer_auth(&token)
            .json(&json!({ "reaction_type": { "emoji_type": WORKING_EMOJI } })),
        )
        .await?,
        "the reaction",
    )?;

    body["data"]["reaction_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Feishu returned no reaction"))
}

pub(crate) async fn unreact(config: &Value, reply: &Value, reaction_id: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;
    let token = app.token(&http).await?;
    let message_id = reply["message_id"].as_str().unwrap_or_default();

    ok(
        send(
            &http,
            http.delete(format!(
                "{}/im/v1/messages/{message_id}/reactions/{reaction_id}",
                app.api()
            ))
            .bearer_auth(&token),
        )
        .await?,
        "removing the reaction",
    )
    .map(|_| ())
}

/// 私聊发给一个人（按 open_id）。
pub(crate) async fn tell(config: &Value, open_id: &str, markdown: &str) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;
    let token = app.token(&http).await?;

    ok(
        send(
            &http,
            http.post(format!(
                "{}/im/v1/messages?receive_id_type=open_id",
                app.api()
            ))
            .bearer_auth(&token)
            .json(&json!({
                "receive_id": open_id,
                "msg_type": "interactive",
                "content": card(markdown),
            })),
        )
        .await?,
        "the private message",
    )
    .map(|_| ())
}

/// 来不及在确认里带回去的点击结果：换上卡片（应用发的共享卡片才换得了）；没有卡片时私下告诉点的人。
pub(crate) async fn answer(config: &Value, action: &CardAction, answer: &CardAnswer) -> Result<()> {
    let Some(card) = &answer.card else {
        return match &answer.notice {
            Some((_, text)) => tell(config, action.sender.primary(), text).await,
            None => Ok(()),
        };
    };

    let app = App::parse(config)?;
    let http = http()?;
    let token = app.token(&http).await?;
    let message_id = action.card["message_id"]
        .as_str()
        .ok_or_else(|| Error::invalid("the Feishu card action has no message"))?;

    let content = lark_card(&card.message, &card.buttons, card.note.as_deref());

    ok(
        send(
            &http,
            http.patch(format!("{}/im/v1/messages/{message_id}", app.api()))
                .bearer_auth(&token)
                .json(&json!({ "content": content.to_string() })),
        )
        .await?,
        "updating the card",
    )
    .map(|_| ())
}
