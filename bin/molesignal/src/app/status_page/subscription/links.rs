// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Public page links and the messages sent to subscribers. An email is the
//! copy with the links that manage or end the subscription at its foot; a
//! Slack message is the same copy with buttons; a webhook receives the
//! payload, which carries the same links.

use serde_json::{Value, json};
use url::Url;

use super::{
    super::notification_copy::{self, EmailCopy, EventUpdate},
    slack_message::{SlackButton, slack_body},
};
use crate::{
    domain::{
        i18n_language::LanguagePack,
        status_page::{
            StatusPage, StatusPageNotificationDelivery, StatusPageNotificationMessage,
            StatusPageSubscriber, StatusPageSubscriberChannel, SubscriberComponents,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// What a message to a subscriber takes from the page it is sent for.
#[derive(Debug, Clone)]
pub(super) struct NotificationPage {
    pub(super) url: String,
    pub(super) timezone: String,
    pub(super) language: String,
    /// The components the page shows, to name the ones a subscriber chose.
    pub(super) components: Vec<(Id, String)>,
}

impl NotificationPage {
    fn component_names(&self, chosen: &SubscriberComponents) -> Vec<String> {
        self.components
            .iter()
            .filter(|(id, _)| chosen.component_ids.contains(id))
            .map(|(_, name)| name.clone())
            .collect()
    }
}

/// The links a subscriber's messages carry, each bound to its token.
pub(super) struct SubscriberLinks {
    pub(super) confirm: String,
    pub(super) manage: String,
    pub(super) unsubscribe: String,
}

impl SubscriberLinks {
    pub(super) fn new(page_url: &str, token: &str) -> Result<Self> {
        Ok(Self {
            confirm: subscription_action_url(page_url, "confirm", token)?,
            manage: subscription_action_url(page_url, "manage", token)?,
            unsubscribe: subscription_action_url(page_url, "unsubscribe", token)?,
        })
    }
}

/// A message about the subscription itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubscriptionMessage {
    /// Activates a pending subscription.
    Confirm,
    /// Leads an active subscriber to where it changes what it hears about.
    Manage,
}

pub(super) fn platform_page_url(external_url: &str, slug: &str) -> Result<String> {
    let base = http_url(
        external_url,
        "http.external_url is required for status-page subscriptions",
    )?;

    base.join(&format!("/status/{slug}"))
        .map(|url| url.to_string())
        .map_err(|_| Error::internal("status-page URL could not be built"))
}

pub(super) fn custom_page_url(hostname: &str) -> Result<String> {
    http_url(
        &format!("https://{hostname}/"),
        "active status-page custom domain is invalid",
    )
    .map(|url| url.to_string())
}

/// Where the public page shows one incident.
pub(super) fn incident_url(page_url: &str, incident_id: &str) -> String {
    format!("{}/incidents/{incident_id}", page_url.trim_end_matches('/'))
}

/// `pack` holds the copy of an email or a Slack message. A webhook receives
/// the payload alone and takes `None`.
pub(super) fn subscription_message(
    kind: SubscriptionMessage,
    page: &StatusPage,
    page_url: &str,
    subscriber: &StatusPageSubscriber,
    component_names: &[String],
    pack: Option<&LanguagePack>,
) -> Result<StatusPageNotificationMessage> {
    write_subscription_message(
        kind,
        (&page.name, &page.slug, page_url),
        subscriber.channel,
        &subscriber.token,
        component_names,
        pack,
    )
}

/// Posted to a channel the Slack app was just added to: what it will hear
/// about, and the links to the page and to the subscription.
pub(super) fn slack_welcome_message(
    page: &StatusPage,
    page_url: &str,
    subscriber: &StatusPageSubscriber,
    component_names: &[String],
    pack: &LanguagePack,
) -> Result<StatusPageNotificationMessage> {
    let links = SubscriberLinks::new(page_url, &subscriber.token)?;
    let connected =
        notification_copy::slack_text(pack, "connected", &[("page_name", page.name.as_str())])?;
    let scope = notification_copy::subscription_scope(pack, component_names)?;

    let copy = EmailCopy {
        title: page.name.clone(),
        text: format!("{connected}\n\n{scope}"),
    };

    let buttons = button_list(
        Some(pack),
        &[("view_page", page_url), ("manage", links.manage.as_str())],
    )?;

    Ok(for_channel(
        StatusPageSubscriberChannel::Slack,
        Some(copy),
        Value::Null,
        None,
        &buttons,
    ))
}

/// The page address with a subscription action for the page to show, such as
/// `slack_connected`.
pub(super) fn page_action_url(page_url: &str, action: &str) -> Result<String> {
    let mut url = http_url(page_url, "status-page URL is invalid")?;

    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("subscription_action", action)
        .finish();

    url.set_fragment(Some(&fragment));
    Ok(url.to_string())
}

/// `page` is the name, slug and address of the page.
fn write_subscription_message(
    kind: SubscriptionMessage,
    page: (&str, &str, &str),
    channel: StatusPageSubscriberChannel,
    token: &str,
    component_names: &[String],
    pack: Option<&LanguagePack>,
) -> Result<StatusPageNotificationMessage> {
    let (page_name, page_slug, page_url) = page;
    let links = SubscriberLinks::new(page_url, token)?;

    let (payload_type, copy, buttons) = match kind {
        SubscriptionMessage::Confirm => {
            let copy = pack
                .map(|pack| {
                    notification_copy::subscription_confirmation(
                        pack,
                        page_name,
                        &links.confirm,
                        &links.unsubscribe,
                        component_names,
                    )
                })
                .transpose()?;

            let buttons = button_list(pack, &[("confirm", links.confirm.as_str())])?;
            ("status_page_subscription_confirmation", copy, buttons)
        }
        SubscriptionMessage::Manage => {
            let copy = pack
                .map(|pack| notification_copy::subscription_manage(pack, page_name, &links.manage))
                .transpose()?;

            let buttons = button_list(pack, &[("manage", links.manage.as_str())])?;
            ("status_page_subscription_manage", copy, buttons)
        }
    };

    let mut payload = json!({
        "type": payload_type,
        "page_name": page_name,
        "page_slug": page_slug,
        "manage_url": links.manage,
        "unsubscribe_url": links.unsubscribe,
        "components": component_names,
        "token": token,
    });

    if kind == SubscriptionMessage::Confirm {
        payload["confirm_url"] = json!(links.confirm);
    }

    // The message is about the subscription: its own links are in the copy.
    Ok(for_channel(channel, copy, payload, None, &buttons))
}

/// `email_pack` holds the copy of an email or a Slack message. A webhook
/// receives the payload alone and takes `None`; its payload is still checked.
/// `postmortem` is the text of a postmortem the delivery announces.
pub(super) fn delivery_message(
    delivery: &StatusPageNotificationDelivery,
    page: &NotificationPage,
    email_pack: Option<&LanguagePack>,
    postmortem: Option<&str>,
) -> Result<StatusPageNotificationMessage> {
    let payload = &delivery.payload;
    let page_name = payload_text(payload, "page_name", "Status page")?;
    let links = SubscriberLinks::new(&page.url, &delivery.subscriber_token)?;
    let mut enriched = payload.clone();

    let (copy, buttons) = match payload.get("type").and_then(Value::as_str) {
        Some("component_status_changed") => {
            let component = payload_text(payload, "component_name", "component")?;
            let status = payload_text(payload, "status", "component status")?;

            let copy = email_pack
                .map(|pack| {
                    notification_copy::component_status_changed(
                        pack, page_name, &page.url, component, status,
                    )
                })
                .transpose()?;

            (
                copy,
                button_list(
                    email_pack,
                    &[
                        ("view_page", page.url.as_str()),
                        ("manage", links.manage.as_str()),
                    ],
                )?,
            )
        }
        Some("incident_update") => {
            let maintenance =
                payload.get("incident_kind").and_then(Value::as_str) == Some("maintenance");

            let incident = incident_url(
                &page.url,
                payload_text(payload, "incident_id", "incident id")?,
            );

            let update = EventUpdate {
                page_name,
                page_url: &page.url,
                maintenance,
                title: payload_text(payload, "title", "incident title")?,
                status: payload_text(payload, "status", "incident status")?,
                message: payload_text(payload, "message", "incident message")?,
                scheduled_for: planned_start(payload, maintenance)
                    .map(|at| notification_copy::zoned_time(at, &page.timezone)),
                next_update_at: payload
                    .get("next_update_at")
                    .and_then(Value::as_i64)
                    .map(|at| notification_copy::zoned_time(TimestampMicros(at), &page.timezone)),
            };

            let copy = email_pack
                .map(|pack| notification_copy::event_update(pack, &update))
                .transpose()?;

            enriched["incident_url"] = json!(incident);
            (
                copy,
                button_list(
                    email_pack,
                    &[
                        ("view_incident", incident.as_str()),
                        ("manage", links.manage.as_str()),
                    ],
                )?,
            )
        }
        Some("maintenance_reminder") => {
            let title = payload_text(payload, "title", "maintenance title")?;

            let start = planned_start(payload, true)
                .ok_or_else(|| Error::internal("status-page notification missing planned start"))?;

            let scheduled_for = notification_copy::zoned_time(start, &page.timezone);

            let copy = email_pack
                .map(|pack| {
                    notification_copy::maintenance_reminder(
                        pack,
                        page_name,
                        &page.url,
                        title,
                        &scheduled_for,
                    )
                })
                .transpose()?;

            (
                copy,
                button_list(
                    email_pack,
                    &[
                        ("view_page", page.url.as_str()),
                        ("manage", links.manage.as_str()),
                    ],
                )?,
            )
        }
        Some("postmortem_published") => {
            let title = payload_text(payload, "title", "incident title")?;

            let incident = incident_url(
                &page.url,
                payload_text(payload, "incident_id", "incident id")?,
            );

            let postmortem = postmortem
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| Error::internal("status-page notification missing postmortem"))?;

            let copy = email_pack
                .map(|pack| {
                    notification_copy::postmortem_published(
                        pack, page_name, title, postmortem, &incident,
                    )
                })
                .transpose()?;

            enriched["postmortem"] = json!(postmortem);
            enriched["incident_url"] = json!(incident);
            (
                copy,
                button_list(
                    email_pack,
                    &[
                        ("view_incident", incident.as_str()),
                        ("manage", links.manage.as_str()),
                    ],
                )?,
            )
        }
        Some("status_page_subscription_confirmation") => {
            let page_slug = payload_text(payload, "page_slug", "page slug")?;

            return write_subscription_message(
                SubscriptionMessage::Confirm,
                (page_name, page_slug, &page.url),
                delivery.channel,
                &delivery.subscriber_token,
                &page.component_names(&delivery.subscriber_components),
                email_pack,
            );
        }
        _ => return Err(Error::internal("unknown status-page notification payload")),
    };

    enriched["manage_url"] = json!(links.manage);
    enriched["unsubscribe_url"] = json!(links.unsubscribe);

    let footer =
        match (delivery.channel, email_pack) {
            (StatusPageSubscriberChannel::Email, Some(pack)) => Some(
                notification_copy::email_footer(pack, &links.manage, &links.unsubscribe)?,
            ),
            _ => None,
        };

    Ok(for_channel(
        delivery.channel,
        copy,
        enriched,
        footer,
        &buttons,
    ))
}

/// An email gets its copy and footer, a Slack channel the copy as a Slack
/// message with its buttons, and a webhook the payload.
fn for_channel(
    channel: StatusPageSubscriberChannel,
    copy: Option<EmailCopy>,
    payload: Value,
    footer: Option<String>,
    buttons: &[SlackButton],
) -> StatusPageNotificationMessage {
    let copy = copy.unwrap_or_default();

    match channel {
        StatusPageSubscriberChannel::Email => copy
            .with_footer(footer.as_deref().unwrap_or_default())
            .with_payload(payload),
        StatusPageSubscriberChannel::Slack => {
            let body = slack_body(&copy, buttons);
            copy.with_payload(body)
        }
        StatusPageSubscriberChannel::Webhook => EmailCopy::default().with_payload(payload),
    }
}

/// The buttons of a Slack message, by the key of their label. A message
/// written without copy has none.
fn button_list(pack: Option<&LanguagePack>, buttons: &[(&str, &str)]) -> Result<Vec<SlackButton>> {
    let Some(pack) = pack else {
        return Ok(Vec::new());
    };

    buttons
        .iter()
        .map(|(key, url)| {
            Ok(SlackButton {
                text: notification_copy::slack_text(pack, key, &[])?,
                url: (*url).to_string(),
            })
        })
        .collect()
}

/// When maintenance is planned to start. `None` for an incident, and for a
/// delivery queued before the payload carried `started_at`.
fn planned_start(payload: &Value, maintenance: bool) -> Option<TimestampMicros> {
    payload
        .get("started_at")
        .and_then(Value::as_i64)
        .filter(|_| maintenance)
        .map(TimestampMicros)
}

fn subscription_action_url(page_url: &str, action: &str, token: &str) -> Result<String> {
    let mut url = http_url(page_url, "status-page subscription URL is invalid")?;

    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("subscription_action", action)
        .append_pair("subscription_token", token)
        .finish();

    url.set_fragment(Some(&fragment));
    Ok(url.to_string())
}

fn http_url(value: &str, error: &'static str) -> Result<Url> {
    let url = Url::parse(value.trim()).map_err(|_| Error::unavailable(error))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::unavailable(error));
    }

    Ok(url)
}

fn payload_text<'a>(payload: &'a Value, key: &str, label: &str) -> Result<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::internal(format!("status-page notification missing {label}")))
}

#[cfg(test)]
mod tests;
