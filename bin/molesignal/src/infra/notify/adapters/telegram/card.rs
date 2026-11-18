// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A notification as one Telegram message: the title in bold (behind a dot of the card's colour),
//! the body, the card's fields one per line, then who did what and the card's note in italics.
//!
//! The alert's buttons are callback buttons in the first row, their data written by
//! `infra::im::telegram::callback`; "acknowledge all" carries too many alerts and is left out.
//! "Open in MoleSignal" and the card's links follow as link buttons, two to a row, or as lines of
//! the text when Telegram will not take their addresses.

use domain::notify::{
    buttons::{AlertAction, AlertButtons},
    connector::NotifyMessage,
};
use serde_json::{Map, Value, json};

use super::super::{
    card::{CardColor, card_color, card_title},
    common::{body, truncate_chars},
};
use crate::infra::im::telegram::{
    callback,
    html::{self, MAX_TEXT},
};

const MAX_TITLE_CHARS: usize = 256;
const MAX_LABEL_CHARS: usize = 64;
const MAX_FIELD_CHARS: usize = 1_024;
const MAX_NOTE_CHARS: usize = 512;
/// The fields, links and notes under the body take at most this much of a message.
const MAX_TAIL: usize = MAX_TEXT / 2;
/// Link buttons in a row.
const LINKS_PER_ROW: usize = 2;

fn dot(color: CardColor) -> &'static str {
    match color {
        CardColor::Blue => "🔵",
        CardColor::Green => "🟢",
        CardColor::Yellow => "🟡",
        CardColor::Orange => "🟠",
        CardColor::Red => "🔴",
        CardColor::Grey => "⚪",
    }
}

fn italic(text: &str) -> String {
    format!(
        "<i>{}</i>",
        html::escape(&truncate_chars(text, MAX_NOTE_CHARS))
    )
}

/// The message's fields for `sendMessage` or `editMessageText`: `text`, `parse_mode`,
/// `link_preview_options` and, when there are buttons, `reply_markup`. `note` says who did what
/// to the alert; without `link_buttons` the links are lines of the text.
pub(super) fn render(
    message: &NotifyMessage,
    buttons: Option<&AlertButtons>,
    note: Option<&str>,
    link_buttons: bool,
) -> Map<String, Value> {
    let card = message.card.as_ref();

    let title = match card {
        Some(card) => format!(
            "{} <b>{}</b>",
            dot(card_color(card, message)),
            html::escape(&truncate_chars(card_title(card, message), MAX_TITLE_CHARS))
        ),
        None => match message.title.trim() {
            "" => String::new(),
            title => format!(
                "<b>{}</b>",
                html::escape(&truncate_chars(title, MAX_TITLE_CHARS))
            ),
        },
    };

    let open = buttons
        .and_then(|buttons| buttons.open.as_ref())
        .map(|(label, url)| (label.as_str(), url.as_str()));

    let card_links = card
        .into_iter()
        .flat_map(|card| &card.links)
        .map(|link| (link.label.as_str(), link.url.as_str()));

    let links = open
        .into_iter()
        .chain(card_links)
        .map(|(label, url)| (truncate_chars(label.trim(), MAX_LABEL_CHARS), url.trim()))
        .filter(|(label, url)| !label.is_empty() && html::linkable(url))
        .collect::<Vec<_>>();

    let tail = tail(message, &links, note, link_buttons);
    let used = html::visible_len(&title) + html::visible_len(&tail) + 3;
    let body = html::fit(body(message), MAX_TEXT.saturating_sub(used));

    let head = [title, body]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let text = [head, tail]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    // A message cannot be empty.
    let text = if text.is_empty() {
        "…".to_string()
    } else {
        text
    };

    let mut fields = [
        ("text", Value::String(text)),
        ("parse_mode", json!("HTML")),
        ("link_preview_options", json!({ "is_disabled": true })),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect::<Map<String, Value>>();

    if let Some(keyboard) = keyboard(buttons, &links, link_buttons) {
        fields.insert("reply_markup".into(), keyboard);
    }

    fields
}

/// What goes under the body: the card's fields, the links (when they are not buttons) and the
/// notes, as far as they fit in [`MAX_TAIL`].
fn tail(
    message: &NotifyMessage,
    links: &[(String, &str)],
    note: Option<&str>,
    link_buttons: bool,
) -> String {
    let notes = note
        .into_iter()
        .chain(message.card.as_ref().map(|card| card.note.as_str()))
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(italic)
        .collect::<Vec<_>>();

    let lines = if link_buttons {
        Vec::new()
    } else {
        links
            .iter()
            .map(|(label, url)| format!("{}: {}", html::escape(label), html::escape(url)))
            .collect::<Vec<_>>()
    };

    let mut room = MAX_TAIL.saturating_sub(
        notes
            .iter()
            .chain(&lines)
            .map(|line| html::visible_len(line) + 2)
            .sum::<usize>(),
    );

    let mut fields = Vec::new();

    for field in message.card.iter().flat_map(|card| &card.fields) {
        let (label, value) = (field.label.trim(), field.value.trim());

        if label.is_empty() && value.is_empty() {
            continue;
        }

        let line = format!(
            "<b>{}:</b> {}",
            html::escape(&truncate_chars(label, MAX_LABEL_CHARS)),
            html::inline(&truncate_chars(value, MAX_FIELD_CHARS))
        );

        let len = html::visible_len(&line) + 1;

        if len > room {
            break;
        }

        room -= len;
        fields.push(line);
    }

    [fields, lines, notes]
        .into_iter()
        .filter(|block| !block.is_empty())
        .map(|block| block.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The alert's buttons in the first row (acknowledge stands out), then the links two to a row.
fn keyboard(
    buttons: Option<&AlertButtons>,
    links: &[(String, &str)],
    link_buttons: bool,
) -> Option<Value> {
    let mut rows = Vec::new();

    let actions = buttons
        .into_iter()
        .flat_map(|buttons| {
            buttons.actions.iter().filter_map(move |(action, label)| {
                let data = callback::encode(&buttons.value(*action))?;

                let mut button = json!({
                    "text": truncate_chars(label, MAX_LABEL_CHARS),
                    "callback_data": data,
                });

                if matches!(action, AlertAction::Acknowledge | AlertAction::Approve) {
                    button["style"] = json!("primary");
                }

                Some(button)
            })
        })
        .collect::<Vec<_>>();

    if !actions.is_empty() {
        rows.push(Value::Array(actions));
    }

    if link_buttons {
        for pair in links.chunks(LINKS_PER_ROW) {
            let row = pair
                .iter()
                .map(|(label, url)| json!({ "text": label, "url": url }))
                .collect();

            rows.push(Value::Array(row));
        }
    }

    (!rows.is_empty()).then(|| json!({ "inline_keyboard": rows }))
}
