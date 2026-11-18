// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Telegram: a bot (`telegram_bot`) that polls the Bot API for updates and answers through it.
//!
//! The node that holds the bot's lease long-polls `getUpdates`, so no public address is needed:
//! first `deleteWebhook` (Telegram hands updates out one way at a time), then polls of 30 seconds
//! for messages and button presses, each poll confirming the updates before it. Which messages
//! are for the bot is up to [`updates`].
//!
//! Answers are Telegram's HTML ([`html`]), in messages of at most 4096 characters that reply to
//! the question; 👀 on the question means the agent is working. A press of an alert card's button
//! is a callback query: the card is rewritten in place and the presser told in a toast
//! ([`answer`]). Telling one person goes to their private chat with the bot; someone who never
//! started it is told in a message of the group that only they see, and a press of theirs opens
//! the private chat (`t.me/<bot>?start=link`), where a first message gets the linking offer.

pub(crate) mod api;
pub(crate) mod callback;
pub(crate) mod html;
mod updates;

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use agent::im::{AlertCard, CardAction, CardAnswer, ConversationKind, NoticeKind};
use kernel::{Error, Result};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use self::{
    api::{Bot, Failure, Refusal, post_card},
    updates::Me,
};
use super::Listener;
use crate::infra::notify::adapters::{TelegramBotConnectorAdapter, truncate_chars};

/// How long a poll waits for updates.
const POLL_SECS: u64 = 30;
/// A poll gives up this much later than Telegram answers an empty one.
const POLL_SLACK: Duration = Duration::from_secs(15);
/// Polls in a row that may fail to get through before the connection counts as lost.
const POLL_RETRIES: u32 = 3;
const POLL_RETRY_WAIT: Duration = Duration::from_secs(2);
/// The updates the bot takes: messages and button presses.
const UPDATES: [&str; 2] = ["message", "callback_query"];
/// The reaction that means "working on it".
const WORKING: &str = "👀";
/// What the link into the private chat sends (`/start link`, read as `/help`).
const START_PARAMETER: &str = "link";
/// What a toast may hold.
const MAX_TOAST_CHARS: usize = 200;
/// The longest rate-limit wait sitting out before sending a message once more.
const MAX_RETRY_WAIT_SECS: u64 = 10;
/// How long the bot remembers that it could not write to someone privately.
const UNREACHED_FOR: Duration = Duration::from_secs(10 * 60);

/// People the bot could not write to privately lately, by bot and user ID: a press of theirs in a
/// group is answered with a link that opens the private chat.
static UNREACHED: LazyLock<Mutex<HashMap<(i64, i64), Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn remember_unreached(bot: i64, user: i64) {
    if let Ok(mut unreached) = UNREACHED.lock() {
        unreached.retain(|_, at| at.elapsed() < UNREACHED_FOR);
        unreached.insert((bot, user), Instant::now());
    }
}

fn unreached(bot: i64, user: i64) -> bool {
    UNREACHED.lock().is_ok_and(|unreached| {
        unreached
            .get(&(bot, user))
            .is_some_and(|at| at.elapsed() < UNREACHED_FOR)
    })
}

/// Polls for updates until `cancel`, or until Telegram refuses (a revoked token, another poller
/// of the same bot) or stays out of reach.
pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let bot = Bot::parse(config)?;
    let http = bot.client()?;

    let me = bot
        .call(&http, "getMe", &json!({}))
        .await
        .map_err(|failure| failure.error("getMe"))?;

    let me = Me {
        id: me["id"].as_i64().unwrap_or_else(|| bot.id()),
        username: me["username"].as_str().unwrap_or_default().to_string(),
    };

    // Updates go to a webhook or to getUpdates, never both; pending ones are kept.
    bot.call(
        &http,
        "deleteWebhook",
        &json!({ "drop_pending_updates": false }),
    )
    .await
    .map_err(|failure| failure.error("deleteWebhook"))?;

    listener.connected(&format!("@{}", me.username)).await;

    let poll = Duration::from_secs(POLL_SECS) + POLL_SLACK;
    let mut offset = None::<i64>;
    let mut failures = 0;

    loop {
        let mut body = json!({ "timeout": POLL_SECS, "allowed_updates": UPDATES });

        if let Some(offset) = offset {
            body["offset"] = json!(offset);
        }

        let polled = tokio::select! {
            () = cancel.cancelled() => return Ok(()),
            polled = bot.call_within(&http, "getUpdates", &body, poll) => polled,
        };

        let batch = match polled {
            Ok(batch) => {
                failures = 0;
                batch
            }
            Err(failure) => {
                let refused = failure
                    .refusal()
                    .map(|refusal| (refusal.code, refusal.retry_after));

                let wait = match refused {
                    Some((429, retry_after)) => Duration::from_secs(retry_after.unwrap_or(5)),
                    Some((code, _)) if code < 500 => return Err(failure.error("getUpdates")),
                    _ if failures < POLL_RETRIES => {
                        failures += 1;
                        POLL_RETRY_WAIT
                    }
                    _ => return Err(failure.error("getUpdates")),
                };

                tokio::select! {
                    () = cancel.cancelled() => return Ok(()),
                    () = tokio::time::sleep(wait) => {}
                }

                continue;
            }
        };

        for update in batch.as_array().into_iter().flatten() {
            if let Some(id) = update["update_id"].as_i64() {
                offset = Some(offset.map_or(id + 1, |offset| offset.max(id + 1)));
            }

            if let Some(inbound) = updates::inbound(&update["message"], &me) {
                listener.message(inbound).await;
            }

            let query = &update["callback_query"];

            if !query.is_object() {
                continue;
            }

            match updates::card_action(query, &me) {
                Some(action) => {
                    listener.action(action).await;
                }
                None => dismiss(&bot, &http, query).await,
            }
        }
    }
}

/// Stops the spinner on a pressed button that is not an alert card's.
async fn dismiss(bot: &Bot, http: &reqwest::Client, query: &Value) {
    if let Some(id) = query["id"].as_str()
        && let Err(failure) = bot
            .call(
                http,
                "answerCallbackQuery",
                &json!({ "callback_query_id": id }),
            )
            .await
    {
        let error = failure.error("answerCallbackQuery");
        tracing::debug!(%error, "a button press was not answered");
    }
}

/// The result of a press: the card rewritten in place, and a toast for the presser (an alert they
/// have to close when something went wrong). Someone the bot could not write to privately is
/// taken to its private chat instead.
pub(crate) async fn answer(config: &Value, action: &CardAction, answer: &CardAnswer) -> Result<()> {
    let bot = Bot::parse(config)?;
    let http = bot.client()?;
    let card = &action.card;

    let query = card["callback_query_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::invalid("the Telegram press has no callback query"))?;

    // The card first, so that it reads anew when the spinner stops.
    let edited = match &answer.card {
        Some(alert) => edit(&bot, &http, card, alert).await,
        None => Ok(()),
    };

    let mut toast = json!({ "callback_query_id": query });

    if let Some((kind, text)) = &answer.notice {
        toast["text"] = json!(truncate_chars(&html::plain_text(text), MAX_TOAST_CHARS));

        if matches!(kind, NoticeKind::Warning | NoticeKind::Error) {
            toast["show_alert"] = json!(true);
        }
    }

    let unreached_presser = action.kind == ConversationKind::Group
        && action
            .sender
            .primary()
            .parse::<i64>()
            .is_ok_and(|user| unreached(bot.id(), user));

    if unreached_presser
        && let Some(username) = card["bot"].as_str().filter(|name| !name.is_empty())
    {
        toast["url"] = json!(format!("https://t.me/{username}?start={START_PARAMETER}"));
        toast["show_alert"] = json!(false);
    }

    let answered = bot
        .call(&http, "answerCallbackQuery", &toast)
        .await
        .map(|_| ())
        .map_err(|failure| failure.error("answerCallbackQuery"));

    edited.and(answered)
}

/// Rewrites the card where it is. An edit that changes nothing is fine.
async fn edit(bot: &Bot, http: &reqwest::Client, card: &Value, alert: &AlertCard) -> Result<()> {
    let target = json!({ "chat_id": card["chat_id"], "message_id": card["message_id"] });

    let fields = |link_buttons| {
        TelegramBotConnectorAdapter::card(
            &alert.message,
            Some(&alert.buttons),
            alert.note.as_deref(),
            link_buttons,
        )
    };

    match post_card(bot, http, "editMessageText", &target, fields).await {
        Ok(_) => Ok(()),
        Err(failure) if failure.refusal().is_some_and(Refusal::unchanged) => Ok(()),
        Err(failure) => Err(failure.error("editMessageText")),
    }
}

/// Sends one message of HTML text with `base` saying where (and how); as plain text when
/// Telegram cannot read the HTML. A short rate limit (the parts of a long answer come one after
/// another) is waited out once.
async fn send_text(
    bot: &Bot,
    http: &reqwest::Client,
    base: &Value,
    text: &str,
) -> std::result::Result<Value, Failure> {
    let mut body = base.clone();
    body["text"] = json!(text);
    body["parse_mode"] = json!("HTML");
    body["link_preview_options"] = json!({ "is_disabled": true });

    let mut sent = bot.call(http, "sendMessage", &body).await;

    let wait = match &sent {
        Err(Failure::Refused(refusal)) if refusal.code == 429 => refusal
            .retry_after
            .filter(|secs| *secs <= MAX_RETRY_WAIT_SECS),
        _ => None,
    };

    if let Some(secs) = wait {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        sent = bot.call(http, "sendMessage", &body).await;
    }

    if !matches!(&sent, Err(Failure::Refused(refusal)) if refusal.unreadable_html()) {
        return sent;
    }

    body["text"] = json!(html::plain(text));

    if let Some(fields) = body.as_object_mut() {
        fields.remove("parse_mode");
    }

    bot.call(http, "sendMessage", &body).await
}

/// Sends Markdown where `place` says (`chat_id`; the `message_id` it replies to and the forum
/// topic `thread_id`, when there are), in as many messages as it takes.
async fn say(
    bot: &Bot,
    http: &reqwest::Client,
    place: &Value,
    markdown: &str,
) -> std::result::Result<(), Failure> {
    let mut base = json!({ "chat_id": place["chat_id"] });

    if let Some(message_id) = place["message_id"].as_i64() {
        base["reply_parameters"] =
            json!({ "message_id": message_id, "allow_sending_without_reply": true });
    }

    if let Some(topic) = place["thread_id"].as_i64() {
        base["message_thread_id"] = json!(topic);
    }

    for text in html::messages(markdown, html::MAX_TEXT) {
        send_text(bot, http, &base, &text).await?;
    }

    Ok(())
}

fn require_chat(reply: &Value) -> Result<()> {
    if reply["chat_id"].is_i64() || reply["chat_id"].is_string() {
        Ok(())
    } else {
        Err(Error::invalid("the Telegram reply has no chat"))
    }
}

pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let bot = Bot::parse(config)?;
    let http = bot.client()?;
    require_chat(reply)?;

    say(&bot, &http, reply, markdown)
        .await
        .map_err(|failure| failure.error("sendMessage"))
}

pub(super) async fn react(config: &Value, reply: &Value) -> Result<String> {
    let working = json!([{ "type": "emoji", "emoji": WORKING }]);
    set_reaction(config, reply, working).await?;
    Ok(WORKING.to_string())
}

/// Takes the bot's reaction off (a bot sets one at most).
pub(super) async fn unreact(config: &Value, reply: &Value, _reaction: &str) -> Result<()> {
    set_reaction(config, reply, json!([])).await
}

async fn set_reaction(config: &Value, reply: &Value, reaction: Value) -> Result<()> {
    let bot = Bot::parse(config)?;
    let http = bot.client()?;
    require_chat(reply)?;

    let body = json!({
        "chat_id": reply["chat_id"],
        "message_id": reply["message_id"],
        "reaction": reaction,
    });

    bot.call(&http, "setMessageReaction", &body)
        .await
        .map(|_| ())
        .map_err(|failure| failure.error("setMessageReaction"))
}

/// Tells one person privately, in their private chat with the bot. Someone who never started the
/// bot (or blocked it) is told in the group where they wrote (`reply`), in a message only they
/// see; when that fails too, a press of theirs is answered with a link into the private chat.
pub(super) async fn tell(config: &Value, reply: &Value, user: &str, markdown: &str) -> Result<()> {
    let bot = Bot::parse(config)?;
    let http = bot.client()?;

    let user_id = user
        .trim()
        .parse::<i64>()
        .map_err(|_| Error::invalid(format!("{user} is not a Telegram user ID")))?;

    match say(&bot, &http, &json!({ "chat_id": user_id }), markdown).await {
        Ok(()) => return Ok(()),
        Err(failure) if !failure.refusal().is_some_and(Refusal::unreachable_person) => {
            return Err(failure.error("sendMessage"));
        }
        Err(_) => {}
    }

    if let Some(chat_id) = reply["chat_id"].as_i64().filter(|chat| *chat != user_id) {
        let topic = reply["thread_id"].as_i64();

        match whisper(&bot, &http, chat_id, topic, user_id, markdown).await {
            Ok(()) => return Ok(()),
            Err(error) => tracing::debug!(%error, "a message for one person was not sent"),
        }
    }

    remember_unreached(bot.id(), user_id);

    Err(Error::invalid(
        "the person has not started a private chat with the Telegram bot",
    ))
}

/// Sends a message in a group that only `user_id` sees (an ephemeral message). Should Telegram
/// post it for everyone, it is deleted at once and this fails: it was not for the group.
async fn whisper(
    bot: &Bot,
    http: &reqwest::Client,
    chat_id: i64,
    topic: Option<i64>,
    user_id: i64,
    markdown: &str,
) -> Result<()> {
    let mut base = json!({
        "chat_id": chat_id,
        "ephemeral_message_parameters": { "receiver_user_id": user_id },
    });

    if let Some(topic) = topic {
        base["message_thread_id"] = json!(topic);
    }

    for text in html::messages(markdown, html::MAX_TEXT) {
        let sent = send_text(bot, http, &base, &text)
            .await
            .map_err(|failure| failure.error("sendMessage"))?;

        if sent
            .get("ephemeral_message_id")
            .is_some_and(|id| !id.is_null())
        {
            continue;
        }

        if let Some(message_id) = sent["message_id"].as_i64().filter(|id| *id != 0) {
            let delete = json!({ "chat_id": chat_id, "message_id": message_id });
            let _ = bot.call(http, "deleteMessage", &delete).await;
        }

        return Err(Error::unavailable(
            "Telegram showed a message meant for one person to the whole group",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests;
