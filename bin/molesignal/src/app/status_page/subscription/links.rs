// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Public page links and the messages sent to subscribers.

use serde_json::{Value, json};
use url::Url;

use super::super::notification_copy::{self, EmailCopy, EventUpdate};
use crate::{
    domain::{
        i18n_language::LanguagePack,
        status_page::{StatusPage, StatusPageNotificationDelivery, StatusPageNotificationMessage},
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// What a message to a subscriber takes from the page it is sent for.
#[derive(Debug, Clone)]
pub(super) struct NotificationPage {
    pub(super) url: String,
    pub(super) timezone: String,
    pub(super) language: String,
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

/// `email_pack` holds the copy an email is written from. A webhook receives the
/// payload alone and takes `None`.
pub(super) fn confirmation_message(
    page: &StatusPage,
    page_url: &str,
    token: &str,
    email_pack: Option<&LanguagePack>,
) -> Result<StatusPageNotificationMessage> {
    let confirm_url = subscription_action_url(page_url, "confirm", token)?;
    let unsubscribe_url = subscription_action_url(page_url, "unsubscribe", token)?;

    let copy = match email_pack {
        Some(pack) => notification_copy::subscription_confirmation(
            pack,
            &page.name,
            &confirm_url,
            &unsubscribe_url,
        )?,
        None => EmailCopy::default(),
    };

    Ok(copy.with_payload(json!({
        "type": "status_page_subscription_confirmation",
        "page_name": page.name,
        "page_slug": page.slug,
        "confirm_url": confirm_url,
        "unsubscribe_url": unsubscribe_url,
        "token": token,
    })))
}

/// `email_pack` holds the copy an email is written from. A webhook receives the
/// payload alone and takes `None`; its payload is still checked.
pub(super) fn delivery_message(
    delivery: &StatusPageNotificationDelivery,
    page: &NotificationPage,
    email_pack: Option<&LanguagePack>,
) -> Result<StatusPageNotificationMessage> {
    let payload = &delivery.payload;
    let page_name = payload_text(payload, "page_name", "Status page")?;

    let copy = match payload.get("type").and_then(Value::as_str) {
        Some("component_status_changed") => {
            let component = payload_text(payload, "component_name", "component")?;
            let status = payload_text(payload, "status", "component status")?;

            email_pack
                .map(|pack| {
                    notification_copy::component_status_changed(
                        pack, page_name, &page.url, component, status,
                    )
                })
                .transpose()?
        }
        Some("incident_update") => {
            let maintenance =
                payload.get("incident_kind").and_then(Value::as_str) == Some("maintenance");

            let update = EventUpdate {
                page_name,
                page_url: &page.url,
                maintenance,
                title: payload_text(payload, "title", "incident title")?,
                status: payload_text(payload, "status", "incident status")?,
                message: payload_text(payload, "message", "incident message")?,
                scheduled_for: planned_start(payload, maintenance)
                    .map(|at| notification_copy::zoned_time(at, &page.timezone)),
            };

            email_pack
                .map(|pack| notification_copy::event_update(pack, &update))
                .transpose()?
        }
        Some("maintenance_reminder") => {
            let title = payload_text(payload, "title", "maintenance title")?;

            let start = planned_start(payload, true)
                .ok_or_else(|| Error::internal("status-page notification missing planned start"))?;

            let scheduled_for = notification_copy::zoned_time(start, &page.timezone);

            email_pack
                .map(|pack| {
                    notification_copy::maintenance_reminder(
                        pack,
                        page_name,
                        &page.url,
                        title,
                        &scheduled_for,
                    )
                })
                .transpose()?
        }
        _ => return Err(Error::internal("unknown status-page notification payload")),
    };

    Ok(copy
        .unwrap_or_default()
        .with_payload(delivery.payload.clone()))
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
mod tests {
    use super::*;
    use crate::{
        app::status_page::notification_copy::tests::seeded_pack,
        domain::status_page::{StatusPageDeliveryStatus, StatusPageSubscriberChannel},
        shared::ids::Id,
    };

    /// 2026-05-25 10:00 UTC.
    const PLANNED_START: i64 = 1_779_703_200_000_000;

    fn delivery(payload: Value) -> StatusPageNotificationDelivery {
        StatusPageNotificationDelivery {
            id: Id("delivery".into()),
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            subscriber_id: Id("subscriber".into()),
            channel: StatusPageSubscriberChannel::Email,
            target: "ops@example.com".into(),
            event_key: "incident_update:update".into(),
            payload,
            status: StatusPageDeliveryStatus::Processing,
            attempts: 1,
            next_attempt_at: TimestampMicros(0),
            last_error: None,
            delivered_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn page(timezone: &str) -> NotificationPage {
        NotificationPage {
            url: "https://status.acme.example/".into(),
            timezone: timezone.into(),
            language: "en-us".into(),
        }
    }

    fn maintenance_payload(status: &str) -> Value {
        json!({
            "type": "incident_update",
            "page_name": "Acme Cloud",
            "page_slug": "acme-cloud",
            "incident_id": "maintenance-1",
            "incident_kind": "maintenance",
            "title": "Database upgrade",
            "impact": "maintenance",
            "status": status,
            "message": "Writes can take a few seconds longer.",
            "occurred_at": 1_779_443_280_000_000_i64,
            "started_at": PLANNED_START,
            "ended_at": null,
            "component_names": ["EU region"],
        })
    }

    #[test]
    fn maintenance_email_states_the_planned_time_in_the_page_time_zone() {
        let message = delivery_message(
            &delivery(maintenance_payload("scheduled")),
            &page("Europe/Berlin"),
            Some(&seeded_pack("en-us")),
        )
        .unwrap();

        assert_eq!(message.title, "Acme Cloud: Database upgrade · Scheduled");
        assert_eq!(
            message.text,
            "Database upgrade\nScheduled\nScheduled for: 2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn later_maintenance_emails_keep_the_planned_time() {
        let mut payload = maintenance_payload("completed");
        payload["ended_at"] = json!(PLANNED_START + 1_800_000_000);

        let message = delivery_message(
            &delivery(payload),
            &page("UTC"),
            Some(&seeded_pack("zh-cn")),
        )
        .unwrap();
        assert_eq!(message.title, "Acme Cloud：Database upgrade · 已完成");
        assert_eq!(
            message.text,
            "Database upgrade\n已完成\n计划时间：2026-05-25 10:00 UTC\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn incident_email_has_no_planned_time_and_the_payload_is_forwarded_unchanged() {
        let payload = json!({
            "type": "incident_update",
            "page_name": "Acme Cloud",
            "page_slug": "acme-cloud",
            "incident_id": "incident-1",
            "incident_kind": "incident",
            "title": "Elevated error rates",
            "impact": "major",
            "status": "resolved",
            "message": "The rollback is complete.",
            "occurred_at": 1_779_529_680_000_000_i64,
            "started_at": 1_779_529_260_000_000_i64,
            "ended_at": 1_779_529_680_000_000_i64,
            "component_names": ["Public API"],
        });

        let message = delivery_message(
            &delivery(payload.clone()),
            &page("Europe/Berlin"),
            Some(&seeded_pack("en-us")),
        )
        .unwrap();
        assert_eq!(
            message.text,
            "Elevated error rates\nResolved\n\nThe rollback is complete.\n\nhttps://status.acme.example/"
        );
        assert_eq!(message.payload, payload);
    }

    #[test]
    fn maintenance_delivery_queued_without_a_start_time_still_reads() {
        let mut payload = maintenance_payload("scheduled");
        let fields = payload.as_object_mut().unwrap();
        fields.remove("started_at");
        fields.remove("ended_at");

        let message = delivery_message(
            &delivery(payload),
            &page("Europe/Berlin"),
            Some(&seeded_pack("en-us")),
        )
        .unwrap();
        assert_eq!(
            message.text,
            "Database upgrade\nScheduled\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn component_email_names_the_component_and_its_status() {
        let payload = json!({
            "type": "component_status_changed",
            "page_name": "Acme Cloud",
            "page_slug": "acme-cloud",
            "component_id": "component-1",
            "component_name": "API",
            "status": "partial_outage",
            "occurred_at": 1_779_529_440_000_000_i64,
        });

        let message = delivery_message(
            &delivery(payload),
            &page("UTC"),
            Some(&seeded_pack("zh-cn")),
        )
        .unwrap();
        assert_eq!(message.title, "Acme Cloud：API · 部分中断");
        assert_eq!(
            message.text,
            "API 的状态已变为“部分中断”。\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn reminder_email_states_the_planned_time_and_needs_it_in_the_payload() {
        let payload = json!({
            "type": "maintenance_reminder",
            "page_name": "Acme Cloud",
            "page_slug": "acme-cloud",
            "incident_id": "maintenance-1",
            "title": "Database upgrade",
            "occurred_at": PLANNED_START - 3_600_000_000,
            "started_at": PLANNED_START,
            "component_names": ["EU region"],
        });

        let message = delivery_message(
            &delivery(payload.clone()),
            &page("Europe/Berlin"),
            Some(&seeded_pack("en-us")),
        )
        .unwrap();
        assert_eq!(message.title, "Acme Cloud: Database upgrade · Reminder");
        assert_eq!(
            message.text,
            "Reminder of planned maintenance:\n\nDatabase upgrade\nScheduled for: 2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nhttps://status.acme.example/"
        );
        assert_eq!(message.payload, payload);

        let mut without_start = payload;
        without_start.as_object_mut().unwrap().remove("started_at");
        assert!(delivery_message(&delivery(without_start), &page("UTC"), None).is_err());
    }

    #[test]
    fn webhook_delivery_carries_the_payload_alone_and_still_checks_it() {
        let payload = maintenance_payload("scheduled");

        let message = delivery_message(&delivery(payload.clone()), &page("UTC"), None).unwrap();
        assert_eq!(message.title, "");
        assert_eq!(message.text, "");
        assert_eq!(message.payload, payload);

        let unknown = json!({"type": "page_renamed", "page_name": "Acme Cloud"});
        assert!(delivery_message(&delivery(unknown), &page("UTC"), None).is_err());
    }

    #[test]
    fn confirmation_is_written_for_email_and_left_to_the_payload_for_a_webhook() {
        let page = StatusPage {
            id: Id("page".into()),
            org_id: Id("org".into()),
            name: "Acme Cloud".into(),
            slug: "acme-cloud".into(),
            logo_url: None,
            brand_color: "#4F46E5".into(),
            custom_domain: None,
            timezone: "UTC".into(),
            language: "zh-cn".into(),
            languages: vec!["zh-cn".into()],
            history_days: 90,
            delivery_retention_days: 90,
            private_session_days: 7,
            visibility: crate::domain::status_page::StatusPageVisibility::Public,
            lifecycle: crate::domain::status_page::StatusPageLifecycle::Active,
            archived_at: None,
            purge_after: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };
        let page_url = "https://status.acme.example/";
        let token = "abc.DEF-12345678901234567890";

        let email =
            confirmation_message(&page, page_url, token, Some(&seeded_pack("zh-cn"))).unwrap();
        assert_eq!(email.title, "确认订阅 Acme Cloud 的状态更新");
        assert!(email.text.contains("subscription_action=confirm"));
        assert!(email.text.contains("subscription_action=unsubscribe"));

        let webhook = confirmation_message(&page, page_url, token, None).unwrap();
        assert_eq!(webhook.title, "");
        assert_eq!(webhook.payload, email.payload);
        assert_eq!(webhook.payload["token"], token);
    }

    #[test]
    fn subscription_links_are_action_bound_and_url_encoded() {
        let page_url = platform_page_url("https://molesignal.example/base", "acme-cloud").unwrap();
        let url =
            subscription_action_url(&page_url, "confirm", "abc.DEF-12345678901234567890").unwrap();
        assert!(url.starts_with("https://molesignal.example/status/acme-cloud#"));
        assert!(url.contains("subscription_action=confirm"));
        assert!(url.contains("subscription_token=abc.DEF-"));
    }

    #[test]
    fn active_custom_domain_uses_the_site_root() {
        assert_eq!(
            custom_page_url("status.acme.example").unwrap(),
            "https://status.acme.example/"
        );
    }
}
