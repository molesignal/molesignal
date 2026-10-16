// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The message a status page posts to a Slack incoming webhook: the subject of
//! the matching email as its header, the body as text, and links as buttons.

use serde_json::{Value, json};

use super::super::notification_copy::EmailCopy;

/// Slack shows at most this much of a header.
const HEADER_CHARS: usize = 150;
/// Slack takes at most this much text in one section.
const SECTION_CHARS: usize = 3_000;
/// Slack takes at most fifty blocks; a long postmortem keeps this many sections.
const MAX_SECTIONS: usize = 45;
const BUTTON_CHARS: usize = 75;

/// A link shown as a button under the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SlackButton {
    pub(super) text: String,
    pub(super) url: String,
}

pub(super) fn slack_body(copy: &EmailCopy, buttons: &[SlackButton]) -> Value {
    let mut blocks = vec![json!({
        "type": "header",
        "text": {"type": "plain_text", "text": clip(&copy.title, HEADER_CHARS), "emoji": false},
    })];

    blocks.extend(
        sections(&escape(&copy.text))
            .into_iter()
            .map(|text| json!({"type": "section", "text": {"type": "mrkdwn", "text": text}})),
    );

    if !buttons.is_empty() {
        let elements: Vec<Value> = buttons
            .iter()
            .map(|button| {
                json!({
                    "type": "button",
                    "text": {"type": "plain_text", "text": clip(&button.text, BUTTON_CHARS)},
                    "url": button.url,
                })
            })
            .collect();

        blocks.push(json!({"type": "actions", "elements": elements}));
    }

    json!({
        "text": clip(&copy.title, SECTION_CHARS),
        "blocks": blocks,
        "unfurl_links": false,
        "unfurl_media": false,
    })
}

/// Slack reads `&`, `<` and `>` as markup in message text.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Splits text into sections Slack accepts, at line breaks where it can. Text
/// beyond the last section is cut; the buttons lead to all of it.
fn sections(text: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in text.split_inclusive('\n') {
        if current.chars().count() + line.chars().count() > SECTION_CHARS && !current.is_empty() {
            sections.push(std::mem::take(&mut current));
        }

        let mut rest = line;

        while rest.chars().count() > SECTION_CHARS {
            let cut = rest
                .char_indices()
                .nth(SECTION_CHARS)
                .map_or(rest.len(), |(index, _)| index);

            sections.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }

        current.push_str(rest);
    }

    if !current.trim().is_empty() {
        sections.push(current);
    }

    if sections.len() > MAX_SECTIONS {
        sections.truncate(MAX_SECTIONS);

        if let Some(last) = sections.last_mut() {
            let kept: String = last.chars().take(SECTION_CHARS - 1).collect();
            *last = format!("{kept}…");
        }
    }

    sections
        .into_iter()
        .map(|section| section.trim_end().to_string())
        .filter(|section| !section.is_empty())
        .collect()
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let kept: String = text.chars().take(limit - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(title: &str, text: &str) -> EmailCopy {
        EmailCopy {
            title: title.into(),
            text: text.into(),
        }
    }

    #[test]
    fn the_subject_heads_the_message_and_links_become_buttons() {
        let body = slack_body(
            &copy(
                "Acme: API · Investigating",
                "API is slow <for some> & more.\n\nhttps://status.acme.example/",
            ),
            &[SlackButton {
                text: "View status page".into(),
                url: "https://status.acme.example/".into(),
            }],
        );

        assert_eq!(body["text"], "Acme: API · Investigating");
        assert_eq!(
            body["blocks"][0]["text"]["text"],
            "Acme: API · Investigating"
        );
        assert_eq!(
            body["blocks"][1]["text"]["text"],
            "API is slow &lt;for some&gt; &amp; more.\n\nhttps://status.acme.example/"
        );
        assert_eq!(
            body["blocks"][2]["elements"][0]["url"],
            "https://status.acme.example/"
        );
        assert_eq!(body["unfurl_links"], false);
    }

    #[test]
    fn long_text_is_split_into_sections_slack_accepts() {
        let line = format!("{}\n", "a".repeat(1_000));
        let long = line.repeat(10);
        let parts = sections(&long);

        assert!(parts.len() >= 4);
        assert!(
            parts
                .iter()
                .all(|part| part.chars().count() <= SECTION_CHARS)
        );

        let huge = "b".repeat(SECTION_CHARS * 60);
        let parts = sections(&huge);
        assert_eq!(parts.len(), MAX_SECTIONS);
        assert!(parts.last().unwrap().ends_with('…'));
    }

    #[test]
    fn a_long_header_is_clipped() {
        let body = slack_body(&copy(&"t".repeat(400), "body"), &[]);
        let header = body["blocks"][0]["text"]["text"].as_str().unwrap();
        assert_eq!(header.chars().count(), HEADER_CHARS);
        assert_eq!(body["blocks"].as_array().unwrap().len(), 2);
    }
}
