// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One-time action links on an alert notification sent to one person: each opens a confirm page
//! where a single press acknowledges or resolves the alert as that person, without signing in.
//!
//! The notify engine issues them when the notification goes to one person and the connector
//! draws no buttons of its own, and puts them into the message itself ([`ActionLinks::apply`]):
//! into the card's links when there is a card, otherwise at the end of the body. Every connector
//! then shows them without knowing about them.
//!
//! Only a digest of each token is stored ([`NotifyActionLinkRepository`]). A link acts once and
//! lasts a day; opening it never acts, so mail scanners that fetch links do no harm.

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};

use super::{connector::NotifyMessage, reply::ReplyAction, template::NotifyCardLink};

/// How long a link works: a day.
pub const ACTION_LINK_TTL_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// One link: the words on it and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLink {
    pub label: String,
    pub url: String,
}

/// The action links of one notification. An alert that is already acknowledged has no
/// acknowledge link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionLinks {
    pub acknowledge: Option<ActionLink>,
    pub resolve: Option<ActionLink>,
}

impl ActionLinks {
    /// The links in the order they are shown: acknowledge, then resolve.
    pub fn iter(&self) -> impl Iterator<Item = &ActionLink> {
        self.acknowledge.iter().chain(self.resolve.iter())
    }

    /// Puts the links into the message: after the card's own links when there is a card,
    /// otherwise at the end of each body the message carries (Markdown, HTML and plain text).
    pub fn apply(&self, message: &mut NotifyMessage) {
        if self.iter().next().is_none() {
            return;
        }

        if let Some(card) = &mut message.card {
            card.links.extend(self.iter().map(|link| NotifyCardLink {
                label: link.label.clone(),
                url: link.url.clone(),
            }));

            return;
        }

        if let Some(markdown) = message
            .markdown
            .as_mut()
            .filter(|body| !body.trim().is_empty())
        {
            let links: Vec<String> = self
                .iter()
                .map(|link| format!("[{}]({})", link.label, link.url))
                .collect();

            markdown.push_str(&format!("\n\n{}", links.join(" · ")));
        }

        if let Some(html) = message.html.as_mut().filter(|body| !body.trim().is_empty()) {
            let links: Vec<String> = self
                .iter()
                .map(|link| {
                    format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(&link.url),
                        escape_html(&link.label)
                    )
                })
                .collect();

            html.push_str(&format!("<p>{}</p>", links.join(" · ")));
        }

        let lines: Vec<String> = self
            .iter()
            .map(|link| format!("{}: {}", link.label, link.url))
            .collect();

        message.text.push_str(&format!("\n\n{}", lines.join("\n")));
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// What a link was issued for: whom it acts as, on which alert, doing what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionGrant {
    pub organization_id: Id,
    pub recipient_user_id: Id,
    pub alert_id: Id,
    pub action: ReplyAction,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
    pub used_at: Option<TimestampMicros>,
}

/// Where issued links are kept, under the digest of their token.
#[async_trait]
pub trait NotifyActionLinkRepository: Send + Sync {
    async fn record(&self, token_hash: &str, grant: &ActionGrant) -> Result<()>;
    /// The link behind a token digest, used or not; `None` when it is unknown or expired.
    async fn get(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ActionGrant>>;
    /// Marks the link used; `false` when it was used already or has expired.
    async fn mark_used(&self, token_hash: &str, at: TimestampMicros) -> Result<bool>;
    /// Removes links that expired before `before`; returns how many.
    async fn purge_expired(&self, before: TimestampMicros) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{super::template::NotifyCard, *};

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · API unavailable".into(),
            text: "Alert API unavailable is open.".into(),
            markdown: Some("**API unavailable** is open.".into()),
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            card: None,
            event: None,
        }
    }

    fn links() -> ActionLinks {
        ActionLinks {
            acknowledge: Some(ActionLink {
                label: "Acknowledge".into(),
                url: "https://ms.example/notify/actions/a".into(),
            }),
            resolve: Some(ActionLink {
                label: "Resolve".into(),
                url: "https://ms.example/notify/actions/r".into(),
            }),
        }
    }

    #[test]
    fn a_card_gets_the_links_after_its_own() {
        let mut message = message();
        message.card = Some(NotifyCard {
            links: vec![NotifyCardLink {
                label: "Runbook".into(),
                url: "https://wiki.example/runbook".into(),
            }],
            ..NotifyCard::default()
        });

        links().apply(&mut message);

        let labels: Vec<&str> = message
            .card
            .as_ref()
            .unwrap()
            .links
            .iter()
            .map(|link| link.label.as_str())
            .collect();
        assert_eq!(labels, ["Runbook", "Acknowledge", "Resolve"]);
        assert_eq!(message.text, "Alert API unavailable is open.");
    }

    #[test]
    fn a_body_without_a_card_ends_with_the_links() {
        let mut message = message();
        message.html = Some("<p>API unavailable</p>".into());

        links().apply(&mut message);

        assert!(message.markdown.as_deref().unwrap().ends_with(
            "[Acknowledge](https://ms.example/notify/actions/a) · [Resolve](https://ms.example/notify/actions/r)"
        ));
        assert!(message.html.as_deref().unwrap().ends_with(
            "<p><a href=\"https://ms.example/notify/actions/a\">Acknowledge</a> · <a href=\"https://ms.example/notify/actions/r\">Resolve</a></p>"
        ));
        assert!(
            message
                .text
                .ends_with("Resolve: https://ms.example/notify/actions/r")
        );
    }

    #[test]
    fn no_links_leave_the_message_alone() {
        let mut message = message();
        ActionLinks::default().apply(&mut message);

        assert_eq!(message.text, "Alert API unavailable is open.");
        assert_eq!(
            message.markdown.as_deref(),
            Some("**API unavailable** is open.")
        );
    }
}
