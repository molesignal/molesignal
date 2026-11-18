// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a Matrix message says: a plain `body` for notifications and clients that show no
//! formatting, and HTML (`org.matrix.custom.html`) for the rest. The title takes the colour of the
//! card or of the alert's severity (`data-mx-color`); the card's fields are a list; the links (the
//! alert's Open link first, then the card's) are `<a>` links. The message mentions no one: with
//! `m.mentions` present and empty, `@room` in an alert's text pings nobody.

use domain::notify::{buttons::AlertButtons, connector::NotifyMessage};
use serde_json::{Value, json};

use super::super::{
    card::{card_color, card_title, escape_html, markdown_html, severity_color},
    common::{body, truncate_bytes},
};

/// The text of a message starts at this many bytes and is halved while the event is too large.
const MAX_TEXT_BYTES: usize = 16_000;
const MIN_TEXT_BYTES: usize = 500;

/// The content of an event is at most this many bytes: a margin under the 65 536 that Matrix allows
/// a whole event, signatures and all.
const MAX_CONTENT_BYTES: usize = 48_000;

/// The content of the `m.room.message` event.
pub(super) fn content(message: &NotifyMessage) -> Value {
    let mut text_bytes = MAX_TEXT_BYTES;

    loop {
        let content = render(message, text_bytes);
        let bytes = serde_json::to_vec(&content).map_or(usize::MAX, |bytes| bytes.len());

        if bytes <= MAX_CONTENT_BYTES || text_bytes <= MIN_TEXT_BYTES {
            return content;
        }

        text_bytes /= 2;
    }
}

/// The content, with the text cut to `text_bytes`.
fn render(message: &NotifyMessage, text_bytes: usize) -> Value {
    let card = message.card.as_ref();

    let (title, color) = match card {
        Some(card) => (card_title(card, message), card_color(card, message)),
        None => (message.title.trim(), severity_color(message)),
    };

    let text = truncate_bytes(body(message).trim(), text_bytes);
    let fields = card.map(|card| card.fields.as_slice()).unwrap_or_default();
    let note = card.map_or("", |card| card.note.trim());

    let links = AlertButtons::read(message)
        .and_then(|buttons| buttons.open)
        .into_iter()
        .chain(
            card.into_iter()
                .flat_map(|card| &card.links)
                .map(|link| (link.label.clone(), link.url.clone())),
        )
        .collect::<Vec<_>>();

    let mut plain = Vec::new();
    let mut html = Vec::new();

    if !title.is_empty() {
        plain.push(title.to_string());
        html.push(format!(
            "<p><strong><span data-mx-color=\"{}\">{}</span></strong></p>",
            color.hex(),
            escape_html(title)
        ));
    }

    if !text.is_empty() {
        html.push(format!("<p>{}</p>", markdown_html(&text)));
        plain.push(text);
    }

    if !fields.is_empty() {
        let lines = fields
            .iter()
            .map(|field| format!("{}: {}", field.label.trim(), field.value.trim()))
            .collect::<Vec<_>>();

        let items = fields
            .iter()
            .map(|field| {
                format!(
                    "<li><strong>{}</strong>: {}</li>",
                    escape_html(field.label.trim()),
                    escape_html(field.value.trim())
                )
            })
            .collect::<String>();

        plain.push(lines.join("\n"));
        html.push(format!("<ul>{items}</ul>"));
    }

    if !links.is_empty() {
        let lines = links
            .iter()
            .map(|(label, url)| format!("{}: {}", label.trim(), url.trim()))
            .collect::<Vec<_>>();

        let anchors = links
            .iter()
            .map(|(label, url)| {
                format!(
                    "<a href=\"{}\">{}</a>",
                    attribute(url.trim()),
                    escape_html(label.trim())
                )
            })
            .collect::<Vec<_>>();

        plain.push(lines.join("\n"));
        html.push(format!("<p>{}</p>", anchors.join(" · ")));
    }

    if !note.is_empty() {
        plain.push(note.to_string());
        html.push(format!("<p><em>{}</em></p>", escape_html(note)));
    }

    json!({
        "msgtype": "m.text",
        "body": plain.join("\n\n"),
        "format": "org.matrix.custom.html",
        "formatted_body": html.concat(),
        "m.mentions": {},
    })
}

/// A value for a double-quoted HTML attribute.
fn attribute(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}
