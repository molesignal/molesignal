// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::*;
use crate::{
    app::status_page::notification_copy::tests::seeded_pack,
    domain::status_page::{
        StatusPageDeliveryStatus, StatusPageSubscriberOrigin, StatusPageSubscriberStatus,
    },
};

/// 2026-05-25 10:00 UTC.
const PLANNED_START: i64 = 1_779_703_200_000_000;
const TOKEN: &str = "abc.DEF-12345678901234567890";

fn delivery(payload: Value) -> StatusPageNotificationDelivery {
    delivery_to(StatusPageSubscriberChannel::Email, payload)
}

fn delivery_to(
    channel: StatusPageSubscriberChannel,
    payload: Value,
) -> StatusPageNotificationDelivery {
    StatusPageNotificationDelivery {
        id: Id("delivery".into()),
        org_id: Id("org".into()),
        status_page_id: Id("page".into()),
        subscriber_id: Id("subscriber".into()),
        channel,
        target: "ops@example.com".into(),
        subscriber_token: TOKEN.into(),
        subscriber_components: SubscriberComponents::chosen(vec![Id("eu".into())]),
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
        components: vec![
            (Id("api".into()), "Public API".into()),
            (Id("eu".into()), "EU region".into()),
        ],
    }
}

fn links() -> SubscriberLinks {
    SubscriberLinks::new("https://status.acme.example/", TOKEN).unwrap()
}

fn english_footer() -> String {
    let links = links();
    format!(
        "Manage your subscription: {}\nUnsubscribe: {}",
        links.manage, links.unsubscribe
    )
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

fn incident_payload() -> Value {
    json!({
        "type": "incident_update",
        "page_name": "Acme Cloud",
        "page_slug": "acme-cloud",
        "incident_id": "incident-1",
        "incident_kind": "incident",
        "title": "Elevated error rates",
        "impact": "major",
        "status": "identified",
        "message": "A bad deploy is being rolled back.",
        "occurred_at": 1_779_529_260_000_000_i64,
        "next_update_at": PLANNED_START,
        "started_at": 1_779_529_260_000_000_i64,
        "ended_at": null,
        "component_names": ["Public API"],
    })
}

#[test]
fn maintenance_email_states_the_planned_time_and_ends_with_the_subscription_links() {
    let message = delivery_message(
        &delivery(maintenance_payload("scheduled")),
        &page("Europe/Berlin"),
        Some(&seeded_pack("en-us")),
        None,
    )
    .unwrap();

    assert_eq!(message.title, "Acme Cloud: Database upgrade · Scheduled");
    assert_eq!(
        message.text,
        format!(
            "Database upgrade\nScheduled\nScheduled for: 2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/\n\n{}",
            english_footer()
        )
    );
}

#[test]
fn an_update_states_by_when_the_next_one_comes() {
    let message = delivery_message(
        &delivery(incident_payload()),
        &page("Europe/Berlin"),
        Some(&seeded_pack("en-us")),
        None,
    )
    .unwrap();

    assert!(message.text.starts_with(
        "Elevated error rates\nIdentified\n\nA bad deploy is being rolled back.\n\nNext update by 2026-05-25 12:00 UTC+02:00 (Europe/Berlin).\n\nhttps://status.acme.example/"
    ));

    let chinese = delivery_message(
        &delivery(incident_payload()),
        &page("UTC"),
        Some(&seeded_pack("zh-cn")),
        None,
    )
    .unwrap();

    assert!(
        chinese
            .text
            .contains("下次更新不晚于 2026-05-25 10:00 UTC。")
    );
    assert!(
        chinese
            .text
            .contains("管理订阅：https://status.acme.example/#subscription_action=manage")
    );
}

#[test]
fn a_webhook_receives_the_payload_with_the_subscription_links() {
    let payload = incident_payload();

    let message = delivery_message(
        &delivery_to(StatusPageSubscriberChannel::Webhook, payload.clone()),
        &page("UTC"),
        None,
        None,
    )
    .unwrap();

    assert_eq!(message.title, "");
    assert_eq!(message.text, "");

    for (key, value) in payload.as_object().unwrap() {
        assert_eq!(
            &message.payload[key], value,
            "{key} must be forwarded unchanged"
        );
    }

    assert_eq!(message.payload["manage_url"], links().manage);
    assert_eq!(message.payload["unsubscribe_url"], links().unsubscribe);
    assert_eq!(
        message.payload["incident_url"],
        "https://status.acme.example/incidents/incident-1"
    );

    let unknown = json!({"type": "page_renamed", "page_name": "Acme Cloud"});
    assert!(delivery_message(&delivery(unknown), &page("UTC"), None, None).is_err());
}

#[test]
fn a_slack_channel_gets_the_email_copy_as_a_slack_message() {
    let message = delivery_message(
        &delivery_to(StatusPageSubscriberChannel::Slack, incident_payload()),
        &page("UTC"),
        Some(&seeded_pack("en-us")),
        None,
    )
    .unwrap();

    assert_eq!(
        message.payload["text"],
        "Acme Cloud: Elevated error rates · Identified"
    );
    assert_eq!(message.payload["blocks"][0]["type"], "header");

    let buttons = &message.payload["blocks"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["elements"];
    assert_eq!(buttons[0]["text"]["text"], "View details");
    assert_eq!(
        buttons[0]["url"],
        "https://status.acme.example/incidents/incident-1"
    );
    assert_eq!(buttons[1]["text"]["text"], "Manage subscription");
    assert!(
        !message.text.contains("Unsubscribe:"),
        "Slack gets buttons, not the email footer"
    );
}

#[test]
fn a_postmortem_email_carries_the_text_and_needs_it() {
    let payload = json!({
        "type": "postmortem_published",
        "page_name": "Acme Cloud",
        "page_slug": "acme-cloud",
        "incident_id": "incident-1",
        "title": "Elevated error rates",
        "impact": "major",
        "revision": false,
        "occurred_at": 1_779_529_960_000_000_i64,
        "component_names": ["Public API"],
    });

    let message = delivery_message(
        &delivery(payload.clone()),
        &page("UTC"),
        Some(&seeded_pack("en-us")),
        Some("## What happened\nA deploy broke the API."),
    )
    .unwrap();

    assert_eq!(
        message.title,
        "Acme Cloud: Postmortem · Elevated error rates"
    );
    assert!(message.text.starts_with(
        "The postmortem of Elevated error rates is published.\n\n## What happened\nA deploy broke the API.\n\nhttps://status.acme.example/incidents/incident-1"
    ));

    let webhook = delivery_message(
        &delivery_to(StatusPageSubscriberChannel::Webhook, payload.clone()),
        &page("UTC"),
        None,
        Some("## What happened"),
    )
    .unwrap();
    assert_eq!(webhook.payload["postmortem"], "## What happened");

    assert!(delivery_message(&delivery(payload), &page("UTC"), None, None).is_err());
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
        None,
    )
    .unwrap();

    assert_eq!(message.title, "Acme Cloud：API · 部分中断");
    assert!(
        message.text.starts_with(
            "API 的状态已变为“部分中断”。\n\nhttps://status.acme.example/\n\n管理订阅："
        )
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
        None,
    )
    .unwrap();
    assert_eq!(message.title, "Acme Cloud: Database upgrade · Reminder");
    assert_eq!(
        message.text,
        format!(
            "Reminder of planned maintenance:\n\nDatabase upgrade\nScheduled for: 2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nhttps://status.acme.example/\n\n{}",
            english_footer()
        )
    );

    let mut without_start = payload;
    without_start.as_object_mut().unwrap().remove("started_at");
    assert!(delivery_message(&delivery(without_start), &page("UTC"), None, None).is_err());
}

#[test]
fn a_queued_confirmation_names_the_components_the_subscriber_chose() {
    let payload = json!({
        "type": "status_page_subscription_confirmation",
        "page_name": "Acme Cloud",
        "page_slug": "acme-cloud",
        "occurred_at": 1_779_529_440_000_000_i64,
    });

    let message = delivery_message(
        &delivery(payload),
        &page("UTC"),
        Some(&seeded_pack("en-us")),
        None,
    )
    .unwrap();

    assert_eq!(message.title, "Confirm updates from Acme Cloud");
    assert!(message.text.contains("You will hear about: EU region."));
    assert!(message.text.contains("subscription_action=confirm"));
    assert_eq!(message.payload["token"], TOKEN);
}

fn subscriber(channel: StatusPageSubscriberChannel) -> StatusPageSubscriber {
    StatusPageSubscriber {
        id: Id("subscriber".into()),
        org_id: Id("org".into()),
        status_page_id: Id("page".into()),
        channel,
        target: "ops@example.com".into(),
        target_label: None,
        status: StatusPageSubscriberStatus::Pending,
        token_hash: "hash".into(),
        token: TOKEN.into(),
        components: SubscriberComponents::all(),
        origin: StatusPageSubscriberOrigin::Page,
        confirmation_sent_at: None,
        confirmed_at: None,
        unsubscribed_at: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn status_page() -> StatusPage {
    StatusPage {
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
        slack_connector_id: None,
    }
}

#[test]
fn confirmation_is_written_for_email_and_left_to_the_payload_for_a_webhook() {
    let page_url = "https://status.acme.example/";

    let email = subscription_message(
        SubscriptionMessage::Confirm,
        &status_page(),
        page_url,
        &subscriber(StatusPageSubscriberChannel::Email),
        &[],
        Some(&seeded_pack("zh-cn")),
    )
    .unwrap();
    assert_eq!(email.title, "确认订阅 Acme Cloud 的状态更新");
    assert!(email.text.contains("subscription_action=confirm"));
    assert!(email.text.contains("subscription_action=unsubscribe"));
    assert!(email.text.contains("你将收到页面上所有组件的更新。"));

    let webhook = subscription_message(
        SubscriptionMessage::Confirm,
        &status_page(),
        page_url,
        &subscriber(StatusPageSubscriberChannel::Webhook),
        &[],
        None,
    )
    .unwrap();
    assert_eq!(webhook.title, "");
    assert_eq!(webhook.payload, email.payload);
    assert_eq!(webhook.payload["token"], TOKEN);
}

#[test]
fn an_address_already_subscribed_is_sent_the_link_that_manages_it() {
    let email = subscription_message(
        SubscriptionMessage::Manage,
        &status_page(),
        "https://status.acme.example/",
        &subscriber(StatusPageSubscriberChannel::Email),
        &[],
        Some(&seeded_pack("en-us")),
    )
    .unwrap();

    assert_eq!(email.title, "Manage your updates from Acme Cloud");
    assert!(email.text.contains(&links().manage));
    assert_eq!(email.payload["type"], "status_page_subscription_manage");
    assert!(email.payload.get("confirm_url").is_none());
}

#[test]
fn subscription_links_are_action_bound_and_url_encoded() {
    let page_url = platform_page_url("https://molesignal.example/base", "acme-cloud").unwrap();
    let links = SubscriberLinks::new(&page_url, TOKEN).unwrap();
    assert!(
        links
            .confirm
            .starts_with("https://molesignal.example/status/acme-cloud#")
    );
    assert!(links.confirm.contains("subscription_action=confirm"));
    assert!(links.manage.contains("subscription_action=manage"));
    assert!(links.unsubscribe.contains("subscription_token=abc.DEF-"));
    assert_eq!(
        incident_url(&page_url, "incident-1"),
        "https://molesignal.example/status/acme-cloud/incidents/incident-1"
    );
}

#[test]
fn active_custom_domain_uses_the_site_root() {
    assert_eq!(
        custom_page_url("status.acme.example").unwrap(),
        "https://status.acme.example/"
    );
    assert_eq!(
        incident_url("https://status.acme.example/", "incident-1"),
        "https://status.acme.example/incidents/incident-1"
    );
}
