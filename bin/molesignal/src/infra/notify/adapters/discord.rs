// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Discord: a channel's webhook (`https://discord.com/api/webhooks/<id>/<token>`).
//!
//! Each notification is one embed: the title, linked to the alert in MoleSignal when the
//! notification carries that link; the colour of the card, or of the alert's severity; the text
//! as Markdown with the card's links under it; the card's fields side by side; and the card's note
//! as the footer. Mentions in the text never ping anyone. Discord is asked to answer once it has
//! stored the message, so a refusal comes back as one and a delivery carries the message ID.
//!
//! The webhook URL holds the webhook's token: requests go out like any webhook's (only to where
//! the host resolves, following no redirect) and no error names the URL.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use domain::notify::{
    buttons::AlertButtons,
    connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
        NotifyMessage, NotifyTarget, NotifyTargetType,
    },
};
use kernel::{Error, Result};
use reqwest::header::RETRY_AFTER;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use signals::http_trace::{self, HttpTarget};
use url::Url;

use super::{
    card::{CardColor, card_color, card_title, severity_color},
    common::{
        body, default_timeout_secs, delivered, provider_message, truncate_chars, validate_http_url,
        validate_timeout,
    },
};
use crate::infra::connectors::dispatcher::http::pinned_client;

pub const DISCORD_WEBHOOK_CONNECTOR_TYPE: &str = "discord_webhook";

/// The hosts Discord serves webhooks from: its stable, public test and canary clients, under the
/// current domain and the old one.
const HOSTS: [&str; 6] = [
    "discord.com",
    "ptb.discord.com",
    "canary.discord.com",
    "discordapp.com",
    "ptb.discordapp.com",
    "canary.discordapp.com",
];

// What an embed may hold, in characters.
const MAX_TITLE_CHARS: usize = 256;
const MAX_DESCRIPTION_CHARS: usize = 4_096;
const MAX_FIELDS: usize = 25;
const MAX_FIELD_NAME_CHARS: usize = 256;
const MAX_FIELD_VALUE_CHARS: usize = 1_024;
const MAX_FOOTER_CHARS: usize = 2_048;

/// The title, description, fields and footer of a message's embeds together, in characters.
const MAX_EMBED_CHARS: usize = 6_000;

/// The description keeps this many characters of its own before fields are left out for it.
const MIN_DESCRIPTION_CHARS: usize = 1_500;

/// A webhook's name is at most this many characters.
const MAX_USERNAME_CHARS: usize = 80;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscordConfig {
    #[serde(default)]
    webhook_url: String,
    /// The name messages are posted under; empty keeps the webhook's own.
    #[serde(default)]
    username: Option<String>,
    /// The avatar messages are posted with; empty keeps the webhook's own.
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl DiscordConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {DISCORD_WEBHOOK_CONNECTOR_TYPE} config: {error}"
            ))
        })?;

        webhook_url(&config.webhook_url, "webhookUrl")?;

        if config
            .username()
            .is_some_and(|name| name.chars().count() > MAX_USERNAME_CHARS)
        {
            return Err(Error::invalid(format!(
                "{DISCORD_WEBHOOK_CONNECTOR_TYPE} username must be at most {MAX_USERNAME_CHARS} characters"
            )));
        }

        if let Some(avatar_url) = config.avatar_url() {
            validate_http_url(avatar_url, "discord_webhook avatarUrl")?;
        }

        validate_timeout(config.timeout_secs, DISCORD_WEBHOOK_CONNECTOR_TYPE)?;
        Ok(config)
    }

    fn username(&self) -> Option<&str> {
        filled(self.username.as_deref())
    }

    fn avatar_url(&self) -> Option<&str> {
        filled(self.avatar_url.as_deref())
    }
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// A Discord webhook URL, `https://<host>/api/[v<n>/]webhooks/<id>/<token>`. `field` names where
/// it was written.
fn webhook_url(value: &str, field: &str) -> Result<Url> {
    let invalid = || {
        Error::invalid(format!(
            "{DISCORD_WEBHOOK_CONNECTOR_TYPE} {field} must be a Discord webhook URL (https://discord.com/api/webhooks/…)"
        ))
    };

    let url = Url::parse(value.trim()).map_err(|_| invalid())?;

    let discord =
        url.scheme() == "https" && url.host_str().is_some_and(|host| HOSTS.contains(&host));

    if !(discord || stand_in(&url)) {
        return Err(invalid());
    }

    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();

    let hook = match segments.as_slice() {
        ["api", "webhooks", hook @ ..] => hook,
        ["api", version, "webhooks", hook @ ..] if version.starts_with('v') => hook,
        _ => return Err(invalid()),
    };

    match hook {
        [id, token] | [id, token, ""]
            if !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit())
                && !token.is_empty() =>
        {
            Ok(url)
        }
        _ => Err(invalid()),
    }
}

/// Tests post to a local mock server in Discord's place.
fn stand_in(url: &Url) -> bool {
    cfg!(test) && url.host_str() == Some("127.0.0.1")
}

/// Asks Discord to answer once the message is stored (`wait=true`), keeping the rest of the query
/// (`thread_id` posts into a thread of the channel).
fn wait_for_the_message(url: &mut Url) {
    let kept = url
        .query_pairs()
        .filter(|(name, _)| name != "wait")
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("wait", "true");
}

#[derive(Default)]
pub struct DiscordWebhookConnectorAdapter;

impl DiscordWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for DiscordWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        DISCORD_WEBHOOK_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: false,
            group: true,
            rich_text: true,
            interactive: false,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        DiscordConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => webhook_url(&target.value, "target").map(drop),
            NotifyTargetType::FixedGroup if target.value.trim().is_empty() => {
                Err(Error::invalid("discord_webhook target cannot be empty"))
            }
            NotifyTargetType::FixedGroup => Ok(()),
            NotifyTargetType::DirectUser | NotifyTargetType::FixedAddress => Err(Error::invalid(
                "discord_webhook target_type must be fixed_group or webhook",
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = DiscordConfig::parse(config)?;
        self.validate_target(target)?;

        let mut url = if target.target_type == NotifyTargetType::Webhook {
            webhook_url(&target.value, "target")?
        } else {
            webhook_url(&config.webhook_url, "webhookUrl")?
        };

        wait_for_the_message(&mut url);

        let client = pinned_client(&url, Duration::from_secs(config.timeout_secs)).await?;
        let request = client.post(url).json(&payload(&config, message));
        let started = Instant::now();

        let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
            .await
            .map_err(|error| {
                Error::unavailable(format!(
                    "{DISCORD_WEBHOOK_CONNECTOR_TYPE} request failed: {}",
                    error.without_url()
                ))
            })?;

        let status = response.status();

        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok());

        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(refusal(status.as_u16(), retry_after, &text));
        }

        let id = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|sent| sent["id"].as_str().map(str::to_string));

        Ok(delivered(id, started))
    }
}

/// The message: one embed, under the configured name and avatar, pinging no one.
fn payload(config: &DiscordConfig, message: &NotifyMessage) -> Value {
    let mut payload = Map::new();

    payload.insert("embeds".into(), json!([embed(message)]));
    payload.insert("allowed_mentions".into(), json!({ "parse": [] }));

    if let Some(username) = config.username() {
        payload.insert("username".into(), json!(username));
    }

    if let Some(avatar_url) = config.avatar_url() {
        payload.insert("avatar_url".into(), json!(avatar_url));
    }

    Value::Object(payload)
}

/// The embed of a notification, within Discord's limits: the description keeps part of its text
/// and all of its links, and the fields take what room is left.
fn embed(message: &NotifyMessage) -> Value {
    let card = message.card.as_ref();

    let (title, color) = match card {
        Some(card) => (card_title(card, message), card_color(card, message)),
        None => (message.title.trim(), severity_color(message)),
    };

    let title = truncate_chars(title, MAX_TITLE_CHARS);
    let footer = truncate_chars(card.map_or("", |card| card.note.trim()), MAX_FOOTER_CHARS);
    let text = body(message).trim();

    let links = card
        .map(|card| {
            card.links
                .iter()
                .map(|link| markdown_link(&link.label, &link.url))
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();

    let reserve = (chars(text) + chars(&links) + 2).min(MIN_DESCRIPTION_CHARS);
    let mut used = chars(&title) + chars(&footer) + reserve;
    let mut fields = Vec::new();

    for field in card.into_iter().flat_map(|card| &card.fields) {
        let name = truncate_chars(field.label.trim(), MAX_FIELD_NAME_CHARS);
        let value = truncate_chars(field.value.trim(), MAX_FIELD_VALUE_CHARS);
        let size = chars(&name) + chars(&value);

        // Discord refuses a field without a name or a value.
        if name.is_empty() || value.is_empty() {
            continue;
        }

        if fields.len() == MAX_FIELDS || used + size > MAX_EMBED_CHARS {
            break;
        }

        used += size;
        fields.push(json!({ "name": name, "value": value, "inline": true }));
    }

    let room = (MAX_EMBED_CHARS + reserve - used).min(MAX_DESCRIPTION_CHARS);
    let description = describe(text, &links, room);
    let mut embed = Map::new();

    if !title.is_empty() {
        embed.insert("title".into(), json!(title));
    }

    if let Some((_, url)) = AlertButtons::read(message).and_then(|buttons| buttons.open) {
        embed.insert("url".into(), json!(url));
    }

    if !description.is_empty() {
        embed.insert("description".into(), json!(description));
    }

    embed.insert("color".into(), json!(rgb(color)));

    if !fields.is_empty() {
        embed.insert("fields".into(), Value::Array(fields));
    }

    if !footer.is_empty() {
        embed.insert("footer".into(), json!({ "text": footer }));
    }

    Value::Object(embed)
}

/// The description: the text, cut short to leave room for the links under it.
fn describe(text: &str, links: &str, room: usize) -> String {
    if links.is_empty() {
        return truncate_chars(text, room);
    }

    let room_for_text = room.saturating_sub(chars(links) + 2);

    if text.is_empty() || room_for_text == 0 {
        return truncate_chars(links, room);
    }

    format!("{}\n\n{links}", truncate_chars(text, room_for_text))
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

/// The colour as Discord takes it: `0xRRGGBB` as a number.
fn rgb(color: CardColor) -> u32 {
    u32::from_str_radix(color.hex().trim_start_matches('#'), 16).unwrap_or_default()
}

fn chars(value: &str) -> usize {
    value.chars().count()
}

/// What Discord says when it refuses a message.
#[derive(Debug, Default, Deserialize)]
struct Refusal {
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: Option<i64>,
    /// Seconds to wait before trying again, on a rate limit.
    #[serde(default)]
    retry_after: Option<f64>,
    /// Which parts of the message were wrong, on a bad request.
    #[serde(default)]
    errors: Option<Value>,
}

/// A refusal as an error: a rate limit says when to try again, anything else what Discord said.
fn refusal(status: u16, retry_after: Option<f64>, text: &str) -> Error {
    let said: Refusal = serde_json::from_str(text).unwrap_or_default();

    if status == 429 {
        return Error::unavailable(match said.retry_after.or(retry_after) {
            Some(seconds) => format!(
                "{DISCORD_WEBHOOK_CONNECTOR_TYPE} was rate limited by Discord: retry after {} s",
                seconds.max(1.0).ceil()
            ),
            None => format!("{DISCORD_WEBHOOK_CONNECTOR_TYPE} was rate limited by Discord"),
        });
    }

    let mut reason = match said.message.trim() {
        "" => text.trim().to_string(),
        message => message.to_string(),
    };

    if let Some(code) = said.code {
        reason.push_str(&format!(" ({code})"));
    }

    if let Some(errors) = said.errors {
        reason.push_str(&format!(": {errors}"));
    }

    Error::unavailable(format!(
        "{DISCORD_WEBHOOK_CONNECTOR_TYPE} was refused: HTTP {status} {}",
        provider_message(&reason)
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::{
        buttons::AlertAction,
        template::{NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink},
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;

    const HOOK_PATH: &str = "/api/webhooks/1234567890/s3cr3t-T0ken";

    fn config(url: &str) -> Value {
        json!({
            "webhookUrl": url,
            "username": "MoleSignal",
            "avatarUrl": "https://molesignal.example.com/logo.png",
        })
    }

    fn group() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedGroup,
            value: "#ops".into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message(card: Option<NotifyCard>) -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: Some("**db-1** is at 97%. @everyone".into()),
            html: None,
            metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
            provider_templates: Default::default(),
            event: None,
            card,
        }
    }

    /// The alert's buttons as the engine writes them; only their Open link is drawn here.
    fn with_open_link(mut message: NotifyMessage) -> NotifyMessage {
        AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: None,
            alert_ids: Vec::new(),
            actions: vec![(AlertAction::Acknowledge, "Acknowledge".into())],
            open: Some((
                "Open in MoleSignal".into(),
                "https://molesignal.example.com/alerts/incidents/inc-1".into(),
            )),
            callback: None,
        }
        .write(&mut message);

        message
    }

    fn card() -> NotifyCard {
        NotifyCard {
            title: "Disk full on db-1".into(),
            color: NotifyCardColor::Orange,
            fields: vec![
                NotifyCardField {
                    label: "Host".into(),
                    value: "db-1".into(),
                },
                NotifyCardField {
                    label: "Usage".into(),
                    value: "97%".into(),
                },
            ],
            links: vec![
                NotifyCardLink {
                    label: "Runbook [disk]".into(),
                    url: "https://runbooks.example.com/disk (full)".into(),
                },
                NotifyCardLink {
                    label: "Acknowledge".into(),
                    url: "https://molesignal.example.com/api/v1/public/notify/actions/t1".into(),
                },
            ],
            note: "Sent by MoleSignal".into(),
        }
    }

    fn total_chars(embed: &Value) -> usize {
        let text = |value: &Value| value.as_str().map_or(0, chars);

        text(&embed["title"])
            + text(&embed["description"])
            + text(&embed["footer"]["text"])
            + embed["fields"].as_array().map_or(0, |fields| {
                fields
                    .iter()
                    .map(|field| text(&field["name"]) + text(&field["value"]))
                    .sum()
            })
    }

    #[test]
    fn configs_are_checked() {
        let adapter = DiscordWebhookConnectorAdapter::new();

        for url in [
            "https://discord.com/api/webhooks/123/abc",
            "https://discordapp.com/api/webhooks/123/abc/",
            "https://ptb.discord.com/api/v10/webhooks/123/abc",
            "https://canary.discord.com/api/webhooks/123/abc?thread_id=456",
        ] {
            adapter
                .validate_config(&json!({ "webhookUrl": url }))
                .unwrap_or_else(|error| panic!("{url}: {error}"));
        }

        for url in [
            "",
            "http://discord.com/api/webhooks/123/abc",
            "https://discord.example.com/api/webhooks/123/abc",
            "https://hooks.slack.com/services/T0/B0/x",
            "https://discord.com/api/webhooks/123",
            "https://discord.com/api/webhooks/abc/token",
            "https://discord.com/api/webhooks/123/abc/slack",
            "https://169.254.169.254/api/webhooks/123/abc",
        ] {
            let error = adapter
                .validate_config(&json!({ "webhookUrl": url }))
                .unwrap_err();
            assert!(matches!(error, Error::InvalidArgument(_)), "{url}");
            assert!(error.to_string().contains("webhookUrl"), "{error}");
        }

        let hook = "https://discord.com/api/webhooks/123/abc";
        assert!(
            adapter
                .validate_config(&json!({ "webhookUrl": hook, "username": "x".repeat(81) }))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({ "webhookUrl": hook, "avatarUrl": "file:///a.png" }))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({ "webhookUrl": hook, "timeoutSecs": 0 }))
                .is_err()
        );
        adapter
            .validate_config(&json!({ "webhookUrl": hook, "username": null, "avatarUrl": "" }))
            .unwrap();
    }

    #[test]
    fn targets_are_a_label_or_a_webhook_of_their_own() {
        let adapter = DiscordWebhookConnectorAdapter::new();
        let target = |target_type, value: &str| NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        };

        adapter.validate_target(&group()).unwrap();
        adapter
            .validate_target(&target(
                NotifyTargetType::Webhook,
                "https://discord.com/api/webhooks/9/other",
            ))
            .unwrap();

        for refused in [
            target(NotifyTargetType::FixedGroup, " "),
            target(NotifyTargetType::Webhook, "https://example.com/hook"),
            target(NotifyTargetType::DirectUser, "alice"),
            target(NotifyTargetType::FixedAddress, "alice@example.com"),
        ] {
            assert!(
                adapter.validate_target(&refused).is_err(),
                "{:?}",
                refused.target_type
            );
        }
    }

    #[tokio::test]
    async fn a_card_is_one_embed_that_pings_no_one() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(HOOK_PATH))
            .and(query_param("wait", "true"))
            .and(query_param("thread_id", "77"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "1100", "type": 0 })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let url = format!("{}{HOOK_PATH}?thread_id=77&wait=false", server.uri());
        let message = with_open_link(message(Some(card())));

        let result = DiscordWebhookConnectorAdapter::new()
            .send(&config(&url), &group(), &message)
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("1100"));

        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        let embed = &sent["embeds"][0];

        assert_eq!(sent["allowed_mentions"], json!({ "parse": [] }));
        assert_eq!(sent["username"], "MoleSignal");
        assert_eq!(
            sent["avatar_url"],
            "https://molesignal.example.com/logo.png"
        );
        assert_eq!(sent["embeds"].as_array().unwrap().len(), 1);

        assert_eq!(embed["title"], "Disk full on db-1");
        assert_eq!(
            embed["url"],
            "https://molesignal.example.com/alerts/incidents/inc-1"
        );
        assert_eq!(embed["color"], 0xE8710A);
        assert_eq!(
            embed["description"],
            "**db-1** is at 97%. @everyone\n\n\
             [Runbook \\[disk\\]](https://runbooks.example.com/disk%20%28full%29) · \
             [Acknowledge](https://molesignal.example.com/api/v1/public/notify/actions/t1)"
        );
        assert_eq!(
            embed["fields"],
            json!([
                { "name": "Host", "value": "db-1", "inline": true },
                { "name": "Usage", "value": "97%", "inline": true },
            ])
        );
        assert_eq!(embed["footer"]["text"], "Sent by MoleSignal");

        // The alert's buttons take no presses here: nothing but the Open link is drawn.
        assert!(!sent.to_string().contains("components"));
    }

    #[tokio::test]
    async fn a_plain_message_takes_the_severity_colour_and_its_own_webhook() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v10/webhooks/42/other"))
            .and(query_param("wait", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "7" })))
            .expect(1)
            .mount(&server)
            .await;

        let target = NotifyTarget {
            target_type: NotifyTargetType::Webhook,
            value: format!("{}/api/v10/webhooks/42/other", server.uri()),
            metadata: BTreeMap::new(),
        };

        DiscordWebhookConnectorAdapter::new()
            .send(
                &json!({ "webhookUrl": format!("{}{HOOK_PATH}", server.uri()) }),
                &target,
                &message(None),
            )
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        let embed = &sent["embeds"][0];

        assert_eq!(embed["title"], "CRITICAL · Disk full");
        assert_eq!(embed["description"], "**db-1** is at 97%. @everyone");
        assert_eq!(embed["color"], 0xD93025);
        assert!(embed.get("url").is_none());
        assert!(embed.get("fields").is_none());
        assert!(sent.get("username").is_none());
    }

    #[test]
    fn a_long_card_keeps_within_the_embed_limits_and_keeps_its_links() {
        let mut card = card();
        card.title = "T".repeat(400);
        card.note = "N".repeat(3_000);
        card.fields = (0..40)
            .map(|index| NotifyCardField {
                label: format!("Field {index}"),
                value: "v".repeat(1_500),
            })
            .collect();

        let mut message = message(Some(card));
        message.markdown = Some("x".repeat(10_000));

        let embed = embed(&message);
        let description = embed["description"].as_str().unwrap();

        assert_eq!(chars(embed["title"].as_str().unwrap()), MAX_TITLE_CHARS);
        assert_eq!(
            chars(embed["footer"]["text"].as_str().unwrap()),
            MAX_FOOTER_CHARS
        );
        assert!(chars(description) <= MAX_DESCRIPTION_CHARS);
        assert!(chars(description) >= MIN_DESCRIPTION_CHARS);
        assert!(description.ends_with("actions/t1)"), "{description}");
        assert!(embed["fields"].as_array().unwrap().len() <= MAX_FIELDS);
        assert!(embed["fields"][0]["value"].as_str().unwrap().ends_with('…'));
        assert!(total_chars(&embed) <= MAX_EMBED_CHARS);

        // Without fields the text takes the whole description.
        let mut plain = message.clone();
        plain.card = None;
        let embed = super::embed(&plain);
        assert_eq!(
            chars(embed["description"].as_str().unwrap()),
            MAX_DESCRIPTION_CHARS
        );
    }

    #[tokio::test]
    async fn a_rate_limit_says_when_to_retry() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(HOOK_PATH))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "3")
                    .set_body_json(json!({
                        "message": "You are being rate limited.",
                        "retry_after": 2.5,
                        "global": false,
                    })),
            )
            .mount(&server)
            .await;

        let error = DiscordWebhookConnectorAdapter::new()
            .send(
                &config(&format!("{}{HOOK_PATH}", server.uri())),
                &group(),
                &message(None),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert!(error.to_string().contains("retry after 3 s"), "{error}");
    }

    #[tokio::test]
    async fn a_refusal_says_why_without_the_token() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(HOOK_PATH))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "message": "Unknown Webhook",
                "code": 10015,
            })))
            .mount(&server)
            .await;

        let error = DiscordWebhookConnectorAdapter::new()
            .send(
                &config(&format!("{}{HOOK_PATH}", server.uri())),
                &group(),
                &message(None),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("HTTP 404 Unknown Webhook (10015)"),
            "{error}"
        );
        assert!(!error.contains("s3cr3t"), "{error}");

        let invalid = refusal(
            400,
            None,
            r#"{"message":"Invalid Form Body","code":50035,"errors":{"embeds":{"0":{"description":{"_errors":[{"code":"BASE_TYPE_MAX_LENGTH"}]}}}}}"#,
        )
        .to_string();
        assert!(
            invalid.contains("Invalid Form Body (50035): {\"embeds\""),
            "{invalid}"
        );
    }
}
