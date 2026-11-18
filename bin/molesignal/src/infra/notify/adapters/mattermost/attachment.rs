// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a Mattermost post shows: one Slack-compatible message attachment with the colour bar, the
//! title linking to MoleSignal, the body, the card's fields, its links as Markdown, the notes as the
//! footer, and the alert buttons. A notification with no card, no buttons and no note is plain
//! Markdown instead.
//!
//! Mattermost posts a press to the button's integration URL with the button's context, here the
//! [`domain::notify::buttons::AlertButtonValue`]; it keeps both from clients.

use domain::notify::{
    buttons::{AlertAction, AlertButtons},
    connector::NotifyMessage,
    template::NotifyCard,
};
use serde_json::{Value, json};

use crate::infra::notify::adapters::{
    card::{card_color, card_title, severity_color},
    common::{body, truncate_chars},
};

/// The longest Markdown a post carries: Mattermost's default post size is 16,383 characters.
const MAX_TEXT_CHARS: usize = 16_000;
/// The longest title or field name.
const MAX_TITLE_CHARS: usize = 250;
/// The longest field value.
const MAX_FIELD_CHARS: usize = 2_000;
/// The longest footer: Mattermost cuts longer ones short.
const MAX_FOOTER_CHARS: usize = 300;
/// A field value this short, on one line, sits beside another.
const SHORT_FIELD_CHARS: usize = 40;

/// A rendered post: its Markdown message and its attachments (none for plain Markdown).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MattermostPost {
    pub(crate) message: String,
    pub(crate) attachments: Vec<Value>,
}

impl MattermostPost {
    /// The post's `props`, where Mattermost reads the attachments of posts made through its API.
    pub(crate) fn props(&self) -> Value {
        json!({ "attachments": self.attachments })
    }
}

/// Renders a notification. Of `buttons`, the action buttons appear only when they carry the address
/// presses go to, and the link to MoleSignal becomes the title's link; `note` says who did what to
/// the alert.
pub(crate) fn mattermost_post(
    message: &NotifyMessage,
    buttons: Option<&AlertButtons>,
    note: Option<&str>,
) -> MattermostPost {
    let card = message.card.as_ref();
    let actions = buttons.map(actions).unwrap_or_default();
    let open = buttons.and_then(|buttons| buttons.open.as_ref());
    let note = note.map(str::trim).filter(|note| !note.is_empty());

    if card.is_none() && actions.is_empty() && open.is_none() && note.is_none() {
        return MattermostPost {
            message: plain(message),
            attachments: Vec::new(),
        };
    }

    let title = card.map_or_else(|| message.title.trim(), |card| card_title(card, message));
    let color = card.map_or_else(|| severity_color(message), |card| card_color(card, message));

    let summary = if title.is_empty() {
        message.text.trim()
    } else {
        title
    };

    let mut attachment = json!({
        "fallback": truncate_chars(summary, MAX_TITLE_CHARS),
        "color": color.hex(),
        "text": truncate_chars(&text(message), MAX_TEXT_CHARS),
    });

    if !title.is_empty() {
        attachment["title"] = json!(truncate_chars(title, MAX_TITLE_CHARS));
    }

    if let Some((_, url)) = open {
        attachment["title_link"] = json!(url);
    }

    if let Some(card) = card
        && !card.fields.is_empty()
    {
        attachment["fields"] = fields(card);
    }

    let footer = footer(note, card);

    if !footer.is_empty() {
        attachment["footer"] = json!(truncate_chars(&footer, MAX_FOOTER_CHARS));
    }

    if !actions.is_empty() {
        attachment["actions"] = Value::Array(actions);
    }

    MattermostPost {
        message: String::new(),
        attachments: vec![attachment],
    }
}

/// The attachment's text: the body, then the card's links in Markdown.
fn text(message: &NotifyMessage) -> String {
    let mut text = body(message).trim().to_string();

    let links = message
        .card
        .iter()
        .flat_map(|card| &card.links)
        .map(|link| markdown_link(&link.label, &link.url))
        .collect::<Vec<_>>();

    if !links.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }

        text.push_str(&links.join(" · "));
    }

    text
}

/// The card's fields; a short value on one line sits beside another.
fn fields(card: &NotifyCard) -> Value {
    card.fields
        .iter()
        .map(|field| {
            let value = field.value.trim();

            json!({
                "short": value.chars().count() <= SHORT_FIELD_CHARS && !value.contains('\n'),
                "title": truncate_chars(field.label.trim(), MAX_TITLE_CHARS),
                "value": truncate_chars(value, MAX_FIELD_CHARS),
            })
        })
        .collect()
}

/// Who did what to the alert, then the card's own small print.
fn footer(note: Option<&str>, card: Option<&NotifyCard>) -> String {
    [note, card.map(|card| card.note.trim())]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The alert's action buttons, each posting its value to the address the buttons carry. Without
/// that address nothing would take a press, so there are none.
fn actions(buttons: &AlertButtons) -> Vec<Value> {
    let Some(callback) = buttons
        .callback
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Vec::new();
    };

    buttons
        .actions
        .iter()
        .map(|(action, label)| {
            let mut button = json!({
                "id": action_id(*action),
                "type": "button",
                "name": label,
                "integration": {
                    "url": callback,
                    "context": buttons.value(*action),
                },
            });

            if matches!(
                action,
                AlertAction::Acknowledge | AlertAction::AcknowledgeAll | AlertAction::Approve
            ) {
                button["style"] = json!("primary");
            }

            button
        })
        .collect()
}

/// A button's ID. A press names it in the path of its API request, which older servers allow
/// letters and digits only.
fn action_id(action: AlertAction) -> String {
    action.as_str().replace('_', "")
}

/// Plain Markdown: the title in bold over the body.
fn plain(message: &NotifyMessage) -> String {
    let title = message.title.trim();
    let body = body(message).trim();

    let text = match (title.is_empty(), body.is_empty()) {
        (true, _) => body.to_string(),
        (false, true) => format!("**{}**", escape_markdown(title)),
        (false, false) => format!("**{}**\n{body}", escape_markdown(title)),
    };

    truncate_chars(&text, MAX_TEXT_CHARS)
}

/// A Markdown link whose label and address cannot end early.
fn markdown_link(label: &str, url: &str) -> String {
    let label = label
        .trim()
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");

    let url = url
        .trim()
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29");

    format!("[{label}]({url})")
}

/// Text that Markdown shows as written.
fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '~' | '[' | ']') {
            out.push('\\');
        }

        out.push(c);
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::template::{NotifyCardColor, NotifyCardField, NotifyCardLink};

    use super::*;

    const CALLBACK: &str = "https://ms.example.com/api/v1/public/notify/c1/mattermost?token=t0k";

    fn message(card: Option<NotifyCard>) -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: Some("Disk on **db-1** is at `97%`.".into()),
            html: None,
            metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
            provider_templates: Default::default(),
            event: None,
            card,
        }
    }

    fn card() -> NotifyCard {
        NotifyCard {
            title: String::new(),
            color: NotifyCardColor::Severity,
            fields: vec![
                NotifyCardField {
                    label: "Host".into(),
                    value: "db-1".into(),
                },
                NotifyCardField {
                    label: "Query".into(),
                    value: "SELECT count(*) FROM disks WHERE used_percent > 95 AND host = 'db-1'"
                        .into(),
                },
            ],
            links: vec![
                NotifyCardLink {
                    label: "Runbook".into(),
                    url: "https://runbooks.example.com/disk".into(),
                },
                NotifyCardLink {
                    label: "Panel [p95]".into(),
                    url: "https://grafana.example.com/d/x (disk)".into(),
                },
            ],
            note: "Sent by MoleSignal".into(),
        }
    }

    fn buttons(callback: Option<&str>) -> AlertButtons {
        AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: Some("policy-1".into()),
            alert_ids: Vec::new(),
            actions: vec![
                (AlertAction::Acknowledge, "Acknowledge".into()),
                (AlertAction::Resolve, "Resolve".into()),
                (AlertAction::Snooze, "Snooze 1h".into()),
            ],
            open: Some((
                "Open in MoleSignal".into(),
                "https://ms.example.com/alerts/incidents/inc-1".into(),
            )),
            callback: callback.map(str::to_string),
        }
    }

    #[test]
    fn a_card_becomes_one_coloured_attachment() {
        let post = mattermost_post(&message(Some(card())), None, None);
        let attachment = &post.attachments[0];

        assert_eq!(post.message, "");
        assert_eq!(post.attachments.len(), 1);
        assert_eq!(attachment["fallback"], "CRITICAL · Disk full");
        assert_eq!(attachment["color"], "#D93025");
        assert_eq!(attachment["title"], "CRITICAL · Disk full");
        assert!(attachment.get("title_link").is_none());
        assert_eq!(
            attachment["text"],
            "Disk on **db-1** is at `97%`.\n\n[Runbook](https://runbooks.example.com/disk) · \
             [Panel \\[p95\\]](https://grafana.example.com/d/x%20%28disk%29)"
        );
        assert_eq!(
            attachment["fields"],
            json!([
                { "short": true, "title": "Host", "value": "db-1" },
                {
                    "short": false,
                    "title": "Query",
                    "value": "SELECT count(*) FROM disks WHERE used_percent > 95 AND host = 'db-1'",
                },
            ])
        );
        assert_eq!(attachment["footer"], "Sent by MoleSignal");
        assert!(attachment.get("actions").is_none());
        assert_eq!(post.props(), json!({ "attachments": [attachment] }));
    }

    #[test]
    fn buttons_post_their_value_to_the_callback_address() {
        let post = mattermost_post(&message(None), Some(&buttons(Some(CALLBACK))), None);
        let attachment = &post.attachments[0];
        let actions = attachment["actions"].as_array().unwrap();

        assert_eq!(attachment["title"], "CRITICAL · Disk full");
        assert_eq!(
            attachment["title_link"],
            "https://ms.example.com/alerts/incidents/inc-1"
        );
        assert_eq!(attachment["text"], "Disk on **db-1** is at `97%`.");
        assert_eq!(actions.len(), 3);

        assert_eq!(
            actions[0],
            json!({
                "id": "acknowledge",
                "type": "button",
                "name": "Acknowledge",
                "style": "primary",
                "integration": {
                    "url": CALLBACK,
                    "context": {
                        "action": "acknowledge",
                        "alert_id": "inc-1",
                        "event_type": "alert.triggered",
                        "policy_id": "policy-1",
                    },
                },
            })
        );
        assert_eq!(actions[1]["id"], "resolve");
        assert!(actions[1].get("style").is_none());
        assert_eq!(actions[2]["integration"]["context"]["action"], "snooze");
    }

    #[test]
    fn acknowledging_a_bundle_names_every_alert_with_a_plain_id() {
        let bundle = AlertButtons {
            alert_ids: vec!["inc-1".into(), "inc-2".into()],
            actions: vec![(AlertAction::AcknowledgeAll, "Acknowledge all".into())],
            ..buttons(Some(CALLBACK))
        };

        let post = mattermost_post(&message(None), Some(&bundle), None);
        let button = &post.attachments[0]["actions"][0];

        assert_eq!(button["id"], "acknowledgeall");
        assert_eq!(button["style"], "primary");
        assert_eq!(
            button["integration"]["context"]["alert_ids"],
            json!(["inc-1", "inc-2"])
        );
    }

    #[test]
    fn without_a_callback_only_the_link_to_molesignal_stays() {
        let post = mattermost_post(&message(None), Some(&buttons(None)), None);
        let attachment = &post.attachments[0];

        assert!(attachment.get("actions").is_none());
        assert_eq!(
            attachment["title_link"],
            "https://ms.example.com/alerts/incidents/inc-1"
        );
    }

    #[test]
    fn who_acted_goes_in_the_footer_before_the_cards_note() {
        let resolved = AlertButtons {
            actions: Vec::new(),
            ..buttons(Some(CALLBACK))
        };

        let mut done = message(Some(card()));
        done.metadata.insert("status".into(), "resolved".into());

        let post = mattermost_post(&done, Some(&resolved), Some("Alice resolved this alert."));
        let attachment = &post.attachments[0];

        assert_eq!(attachment["color"], "#1E8E3E");
        assert_eq!(
            attachment["footer"],
            "Alice resolved this alert. · Sent by MoleSignal"
        );
        assert!(attachment.get("actions").is_none());

        // A note alone still makes an attachment, so the post keeps its shape.
        let noted = mattermost_post(&message(None), None, Some("Alice acknowledged this alert."));
        assert_eq!(
            noted.attachments[0]["footer"],
            "Alice acknowledged this alert."
        );
    }

    #[test]
    fn a_plain_notification_is_markdown_under_a_bold_title() {
        let mut plain = message(None);
        plain.title = "Report *weekly* [ops]".into();

        let post = mattermost_post(&plain, None, None);

        assert!(post.attachments.is_empty());
        assert_eq!(
            post.message,
            "**Report \\*weekly\\* \\[ops\\]**\nDisk on **db-1** is at `97%`."
        );

        plain.markdown = None;
        plain.title = String::new();
        assert_eq!(
            mattermost_post(&plain, None, None).message,
            "Disk is at 97%."
        );

        plain.text = "x".repeat(MAX_TEXT_CHARS + 10);
        assert_eq!(
            mattermost_post(&plain, None, None).message.chars().count(),
            MAX_TEXT_CHARS
        );
    }
}
