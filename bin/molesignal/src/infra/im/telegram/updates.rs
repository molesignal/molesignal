// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the bot is told in an update: a message for it, or a press of an alert card's button.
//!
//! In a private chat every message is for the bot. In a group only those that mention it
//! (`@bot`), are its commands (`/new@bot`, or `/new` alone), or reply to one of its answers; a
//! reply to a notification (a message of the bot with buttons) needs the mention like any other
//! message, so talk about an alert does not reach the agent. Each person has their own thread in
//! a group, one per forum topic. `/start` (the first message of every private chat, and what a
//! `t.me/<bot>?start=…` link sends) reads as `/help`.

use agent::im::{CardAction, ConversationKind, Inbound, Sender};
use serde_json::{Value, json};

use super::callback;

/// The bot itself, as `getMe` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Me {
    pub(crate) id: i64,
    pub(crate) username: String,
}

/// The person behind a message or a press: their user ID, then their @username if they have one.
fn sender(from: &Value) -> Option<Sender> {
    if from["is_bot"].as_bool() == Some(true) {
        return None;
    }

    let id = from["id"].as_i64()?;
    let mut ids = vec![id.to_string()];

    if let Some(username) = from["username"].as_str().filter(|name| !name.is_empty()) {
        ids.push(format!("@{username}"));
    }

    let name = [from["first_name"].as_str(), from["last_name"].as_str()]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    Some(Sender {
        ids,
        name: (!name.is_empty()).then_some(name),
    })
}

/// Where to answer: the chat, the message to reply to and its forum topic.
fn place(chat_id: i64, message_id: i64, topic: Option<i64>) -> Value {
    let mut reply = json!({ "chat_id": chat_id, "message_id": message_id });

    if let Some(topic) = topic {
        reply["thread_id"] = json!(topic);
    }

    reply
}

fn topic(message: &Value) -> Option<i64> {
    (message["is_topic_message"].as_bool() == Some(true))
        .then(|| message["message_thread_id"].as_i64())
        .flatten()
}

/// A message for the bot, with the mention of it taken out; `None` for anything else (chatter in
/// a group, other bots, channels, messages without text).
pub(crate) fn inbound(message: &Value, me: &Me) -> Option<Inbound> {
    let sender = sender(&message["from"])?;
    let user_id = message["from"]["id"].as_i64()?;
    let chat = &message["chat"];
    let chat_id = chat["id"].as_i64()?;
    let message_id = message["message_id"].as_i64().filter(|id| *id != 0)?;

    // Ephemeral messages (seen by one person) can only be answered within seconds.
    if message
        .get("ephemeral_message_id")
        .is_some_and(|id| !id.is_null())
    {
        return None;
    }

    let direct = match chat["type"].as_str()? {
        "private" => true,
        "group" | "supergroup" => false,
        _ => return None,
    };

    let raw = message["text"]
        .as_str()
        .or_else(|| message["caption"].as_str())?;

    let (text, mentioned) = without_mention(raw, &me.username);
    let (text, for_bot) = command(&text, &me.username)?;

    let replied = &message["reply_to_message"];

    let answers_bot = replied["from"]["id"].as_i64() == Some(me.id)
        && replied.get("reply_markup").is_none_or(Value::is_null)
        && replied
            .get("forum_topic_created")
            .is_none_or(Value::is_null);

    let addressed = direct || mentioned || for_bot || answers_bot;

    if !addressed {
        return None;
    }

    let topic = topic(message);

    let thread_key = match (direct, topic) {
        (true, None) => String::new(),
        (true, Some(topic)) => format!("topic:{topic}"),
        (false, None) => format!("user:{user_id}"),
        (false, Some(topic)) => format!("topic:{topic}:user:{user_id}"),
    };

    Some(Inbound {
        message_id: format!("{chat_id}:{message_id}"),
        conversation_id: chat_id.to_string(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key,
        sender,
        text,
        addressed,
        reply: place(chat_id, message_id, topic),
    })
}

/// `text` without `@username` (the bot's name in any case, as a word of its own, so neither
/// `ops@bot.example` nor `@bot_two`), and whether it was there.
pub(crate) fn without_mention(text: &str, username: &str) -> (String, bool) {
    if username.is_empty() {
        return (text.trim().to_string(), false);
    }

    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mention = format!("@{}", username.to_ascii_lowercase());

    // Lowercasing ASCII keeps every byte where it was, so positions hold in `text` too.
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut mentioned = false;
    let mut from = 0;

    while let Some(found) = lower[from..].find(&mention).map(|at| from + at) {
        let end = found + mention.len();
        let before = lower[..found].chars().next_back();
        let after = lower[end..].chars().next();

        out.push_str(&text[from..found]);
        from = end;

        // Part of a longer word or address, such as `ops@bot.example` or `@bot_two`.
        if before.is_some_and(word) || after.is_some_and(word) {
            out.push_str(&text[found..end]);
            continue;
        }

        // The mention goes with one of the spaces around it.
        mentioned = true;

        if text[end..].starts_with(' ') && (out.is_empty() || out.ends_with(char::is_whitespace)) {
            from += 1;
        }
    }

    out.push_str(&text[from..]);

    (out.trim().to_string(), mentioned)
}

/// Reads a leading command: `/cmd@bot` is the bot's (written `/cmd`), `/cmd@other_bot` another
/// bot's (`None`: not for this one), and `/start` reads as `/help`. Returns the text and whether
/// it is a command.
fn command(text: &str, username: &str) -> Option<(String, bool)> {
    let Some(rest) = text.strip_prefix('/') else {
        return Some((text.to_string(), false));
    };

    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (word, argument) = rest.split_at(end);

    let name = match word.split_once('@') {
        Some((name, bot)) if bot.eq_ignore_ascii_case(username) => name,
        Some(_) => return None,
        None => word,
    };

    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');

    if !valid {
        return Some((text.to_string(), false));
    }

    if name.eq_ignore_ascii_case("start") {
        return Some(("/help".to_string(), true));
    }

    Some((format!("/{name}{argument}"), true))
}

/// A press of an alert card's button; `None` for other callback data, or a press without its
/// message (inline mode).
pub(crate) fn card_action(query: &Value, me: &Me) -> Option<CardAction> {
    let id = query["id"].as_str().filter(|id| !id.is_empty())?;
    let value = callback::decode(query["data"].as_str()?)?;
    let sender = sender(&query["from"])?;

    // The card's message; an old one may be inaccessible, but still says where it is.
    let message = &query["message"];
    let chat = &message["chat"];
    let chat_id = chat["id"].as_i64()?;
    let message_id = message["message_id"].as_i64()?;
    let topic = topic(message);

    Some(CardAction {
        value,
        sender,
        conversation_id: chat_id.to_string(),
        kind: if chat["type"].as_str() == Some("private") {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        reply: place(chat_id, message_id, topic),
        card: json!({
            "callback_query_id": id,
            "chat_id": chat_id,
            "message_id": message_id,
            "bot": me.username,
        }),
    })
}
