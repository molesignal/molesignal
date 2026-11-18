// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a Webex message says: Markdown, which everything shows, and for the bot an Adaptive Card,
//! which clients able to draw cards show instead.
//!
//! The card is the Teams card ([`teams_card`]) where Webex reads it the same way: Webex draws
//! Adaptive Cards up to version 1.3 and at most five buttons on a card, and nothing on it can be
//! pressed here, so the card carries links only (the alert's Open link first, then the card's),
//! the ones beyond the fifth as links under the text.

use domain::notify::{buttons::AlertButtons, connector::NotifyMessage};
use serde_json::{Value, json};

use super::super::{
    card::card_title,
    common::{body, truncate_bytes},
    teams_card,
};

/// The Markdown of one message is at most this many bytes.
pub(super) const MAX_MARKDOWN_BYTES: usize = 7_439;

/// Everything one message carries, its Markdown and its card, is at most this many bytes: a margin
/// under Webex's 22 740.
const MAX_MESSAGE_BYTES: usize = 22_000;

/// The text on a card starts at this many bytes and is halved while the message is too large.
const MAX_CARD_TEXT_BYTES: usize = 12_000;
const MIN_CARD_TEXT_BYTES: usize = 500;

/// Webex draws at most this many buttons on a card.
const MAX_CARD_ACTIONS: usize = 5;

const CARD_VERSION: &str = "1.3";
const ADAPTIVE_CARD: &str = "application/vnd.microsoft.card.adaptive";

/// Breaks Webex's mention syntax (`<@all>`, `<@personEmail:…>`) without changing how it reads.
const ZERO_WIDTH_SPACE: char = '\u{200B}';

/// A bot message without its recipient: the Markdown and the card, or the Markdown alone when the
/// card's fields and links leave no room under Webex's limit.
pub(super) fn bot_message(message: &NotifyMessage) -> Value {
    let markdown = markdown(message, MAX_MARKDOWN_BYTES);
    let mut text_bytes = MAX_CARD_TEXT_BYTES;

    loop {
        let card = card(message, text_bytes);
        let card_bytes = serde_json::to_vec(&card).map_or(usize::MAX, |bytes| bytes.len());

        if markdown.len().saturating_add(card_bytes) <= MAX_MESSAGE_BYTES {
            return json!({
                "markdown": markdown,
                "attachments": [{ "contentType": ADAPTIVE_CARD, "content": card }],
            });
        }

        if text_bytes <= MIN_CARD_TEXT_BYTES {
            return json!({ "markdown": markdown });
        }

        text_bytes /= 2;
    }
}

/// The card, with its text cut to `text_bytes`.
fn card(message: &NotifyMessage, text_bytes: usize) -> Value {
    let text = truncate_bytes(body(message).trim(), text_bytes);

    let shown = NotifyMessage {
        text: text.clone(),
        markdown: Some(text),
        ..message.clone()
    };

    let links = AlertButtons {
        open: open_link(message),
        ..AlertButtons::default()
    };

    let mut card = teams_card(&shown, &links, None);
    card["version"] = json!(CARD_VERSION);

    let overflow = match card.get_mut("actions").and_then(Value::as_array_mut) {
        Some(actions) if actions.len() > MAX_CARD_ACTIONS => actions.split_off(MAX_CARD_ACTIONS),
        _ => Vec::new(),
    };

    if overflow.is_empty() {
        return card;
    }

    let line = overflow
        .iter()
        .filter_map(|action| {
            Some(markdown_link(
                action["title"].as_str()?,
                action["url"].as_str()?,
            ))
        })
        .collect::<Vec<_>>()
        .join(" · ");

    // Under the text and the fields, above the footer.
    if let Some(blocks) = card.get_mut("body").and_then(Value::as_array_mut) {
        let at = blocks
            .iter()
            .position(|block| block["separator"] == true)
            .unwrap_or(blocks.len());

        blocks.insert(
            at,
            json!({ "type": "TextBlock", "text": line, "wrap": true }),
        );
    }

    card
}

/// The Markdown of a message, at most `max` bytes: the title in bold, the text, the card's fields
/// one to a line, the links (the alert's Open link first, then the card's) and the card's note. The
/// text is cut short to keep the rest; mentions in any of it ping no one.
pub(super) fn markdown(message: &NotifyMessage, max: usize) -> String {
    let card = message.card.as_ref();

    let title = match card {
        Some(card) => card_title(card, message),
        None => message.title.trim(),
    };

    let head = match title {
        "" => String::new(),
        title => quiet(&format!("**{title}**")),
    };

    let mut tail = card
        .into_iter()
        .flat_map(|card| &card.fields)
        .map(|field| format!("**{}**: {}", field.label.trim(), field.value.trim()))
        .collect::<Vec<_>>();

    let links = open_link(message)
        .into_iter()
        .chain(
            card.into_iter()
                .flat_map(|card| &card.links)
                .map(|link| (link.label.clone(), link.url.clone())),
        )
        .map(|(label, url)| markdown_link(&label, &url))
        .collect::<Vec<_>>();

    if !links.is_empty() {
        tail.push(links.join(" · "));
    }

    if let Some(note) = card
        .map(|card| card.note.trim())
        .filter(|note| !note.is_empty())
    {
        tail.push(format!("_{note}_"));
    }

    let tail = quiet(&tail.join("\n"));
    let room = max.saturating_sub(head.len() + tail.len() + 4);
    let text = truncate_bytes(&quiet(body(message).trim()), room);

    let whole = [head, text, tail]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    truncate_bytes(&whole, max)
}

/// The alert's link into MoleSignal, when the notification carries one.
fn open_link(message: &NotifyMessage) -> Option<(String, String)> {
    AlertButtons::read(message).and_then(|buttons| buttons.open)
}

/// `[label](url)`, with brackets in the label and parentheses and spaces in the URL escaped so
/// that it stays one link.
fn markdown_link(label: &str, url: &str) -> String {
    let label = label.trim().replace('[', "\\[").replace(']', "\\]");

    let url = url
        .trim()
        .replace('(', "%28")
        .replace(')', "%29")
        .replace(' ', "%20");

    format!("[{label}]({url})")
}

/// `text` with Webex's mention syntax broken, so that text from an alert cannot ping a space.
fn quiet(text: &str) -> String {
    text.replace("<@", &format!("<{ZERO_WIDTH_SPACE}@"))
}
