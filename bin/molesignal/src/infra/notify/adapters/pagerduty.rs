// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty: events go to a PagerDuty service through the Events API v2 (`POST /v2/enqueue`),
//! with the routing key of one of its integrations. An alert keeps one PagerDuty alert for its
//! whole life, matched by the dedup key `molesignal-<alert id>`: triggering and escalating it
//! trigger that alert, acknowledging and resolving it acknowledge and resolve it. Any other event
//! triggers a PagerDuty alert of its own.

use std::time::Instant;

use async_trait::async_trait;
use domain::{
    alerting::incident::Severity,
    notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        NotifyChannel, NotifyMessage, NotifyTarget,
    },
};
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use signals::http_trace::{self, HttpTarget};

use super::{
    common::{self, default_timeout_secs},
    webhook::{self, WebhookConnectorAdapter},
};

pub const PAGERDUTY_CONNECTOR_TYPE: &str = "pagerduty";

/// Where PagerDuty takes events, by the region of the account (`region` picks one); the first is
/// the default.
const PAGERDUTY_REGIONS: [(&str, &str); 2] = [
    ("us", "https://events.pagerduty.com"),
    ("eu", "https://events.eu.pagerduty.com"),
];

/// An integration's routing key is 32 letters and digits.
const ROUTING_KEY_CHARS: usize = 32;

/// PagerDuty takes a summary of up to 1,024 characters and a dedup key of up to 255.
const MAX_SUMMARY_CHARS: usize = 1_024;
const MAX_DEDUP_KEY_CHARS: usize = 255;

/// The monitoring tool PagerDuty shows an alert came from.
const CLIENT: &str = "MoleSignal";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagerDutyConfig {
    #[serde(default)]
    routing_key: String,
    #[serde(default)]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl PagerDutyConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {PAGERDUTY_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        let key = self.routing_key.trim();

        if key.len() != ROUTING_KEY_CHARS || !key.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(Error::invalid(
                "pagerduty routingKey must be the 32-character integration key of an Events API v2 integration",
            ));
        }

        common::validate_http_url(self.base_url(), "pagerduty apiBaseUrl")?;
        common::validate_timeout(self.timeout_secs, PAGERDUTY_CONNECTOR_TYPE)
    }

    /// The address picked for the account's region; left empty, the default one.
    fn base_url(&self) -> &str {
        match self.api_base_url.trim() {
            "" => PAGERDUTY_REGIONS[0].1,
            url => url,
        }
    }
}

/// The Events API v2 event for `message`. Acknowledging and resolving name the alert by its dedup
/// key alone; a trigger says what the alert is.
fn event(routing_key: &str, message: &NotifyMessage) -> Value {
    let event_type = webhook::event_type(message);
    let action = event_action(event_type);

    let mut event = json!({
        "routing_key": routing_key,
        "event_action": action,
    });

    if let Some(key) = dedup_key(message, event_type) {
        event["dedup_key"] = Value::String(key);
    }

    if action != "trigger" {
        return event;
    }

    event["payload"] = payload(message);
    event["client"] = json!(CLIENT);

    if let Some(url) = event_text(message, "/alert/url")
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
    {
        event["client_url"] = json!(url);
        event["links"] = json!([{ "href": url, "text": CLIENT }]);
    }

    event
}

/// What PagerDuty is asked to do: acknowledging and resolving an alert acknowledge and resolve its
/// PagerDuty alert; anything else (an alert triggered or escalated, another event) triggers one.
fn event_action(event_type: Option<&str>) -> &'static str {
    match event_type {
        Some("alert.acknowledged") => "acknowledge",
        Some("alert.resolved") => "resolve",
        _ => "trigger",
    }
}

/// The key PagerDuty deduplicates on: one per MoleSignal alert; another event's own ID, or the
/// delivery's when there is no event (a connector test).
fn dedup_key(message: &NotifyMessage, event_type: Option<&str>) -> Option<String> {
    let alert = event_type
        .filter(|kind| kind.starts_with("alert."))
        .and_then(|_| {
            message
                .metadata
                .get("alert_id")
                .map(String::as_str)
                .or_else(|| event_text(message, "/alert/id"))
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| format!("molesignal-{id}"));

    let key = alert
        .or_else(|| event_text(message, "/event/id").map(str::to_string))
        .or_else(|| message.metadata.get("delivery_id").cloned())?;

    Some(key.chars().take(MAX_DEDUP_KEY_CHARS).collect())
}

/// A trigger's payload: the title, the affected service, the severity, when it happened, and the
/// event the notify engine wrote for machines.
fn payload(message: &NotifyMessage) -> Value {
    let mut payload = Map::new();
    payload.insert("summary".into(), json!(summary(message)));

    let source = event_text(message, "/alert/service")
        .map(str::trim)
        .filter(|service| !service.is_empty())
        .unwrap_or(CLIENT);

    payload.insert("source".into(), json!(source));
    payload.insert("severity".into(), json!(severity(message)));

    if let Some(at) = event_text(message, "/event/occurredAt") {
        payload.insert("timestamp".into(), json!(at));
    }

    payload.insert("custom_details".into(), custom_details(message));
    Value::Object(payload)
}

/// The notification's title, else the first line of its text: a trigger needs a summary.
fn summary(message: &NotifyMessage) -> String {
    let summary = [message.title.as_str()]
        .into_iter()
        .chain(message.text.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(CLIENT);

    common::truncate_chars(summary, MAX_SUMMARY_CHARS)
}

/// PagerDuty names its severities as MoleSignal does (`critical`, `error`, `warning`, `info`);
/// an event that is no alert is `info`.
fn severity(message: &NotifyMessage) -> &'static str {
    message
        .metadata
        .get("severity")
        .map(String::as_str)
        .or_else(|| event_text(message, "/alert/severity"))
        .and_then(Severity::from_label)
        .map_or("info", |severity| severity.as_str())
}

/// The event as the generic webhook writes it (the alert, its rule and escalation, or another
/// event's data), without its version; the notification's text when there is no event.
fn custom_details(message: &NotifyMessage) -> Value {
    match &message.event {
        Some(Value::Object(event)) => {
            let mut details = event.clone();
            details.remove("version");

            Value::Object(details)
        }
        _ => json!({ "text": message.text }),
    }
}

/// A text in the event the notify engine wrote for machines.
fn event_text<'a>(message: &'a NotifyMessage, pointer: &str) -> Option<&'a str> {
    message.event.as_ref()?.pointer(pointer)?.as_str()
}

/// Why PagerDuty did not take an event: a 400 is the event itself (its errors say what is wrong),
/// a 429 asks to slow down, anything else is on PagerDuty's side.
fn refused(status: u16, answer: &Value) -> Error {
    let message = answer
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();

    let errors = answer
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("; ");

    let said = match (message, errors.as_str()) {
        (message, "") => message.to_string(),
        ("", errors) => errors.to_string(),
        (message, errors) => format!("{message}: {errors}"),
    };

    match status {
        400 => Error::invalid(format!(
            "pagerduty refused the event: {}",
            common::provider_message(&said)
        )),
        429 => Error::unavailable(
            "pagerduty is rate limiting events to this integration (HTTP 429): retry in a few minutes",
        ),
        _ => Error::unavailable(format!(
            "pagerduty returned HTTP {status}: {}",
            common::provider_message(&said)
        )),
    }
}

#[derive(Default)]
pub struct PagerDutyConnectorAdapter;

impl PagerDutyConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for PagerDutyConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        PAGERDUTY_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Webhook
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![ConnectorAddress::picked(
            "apiBaseUrl",
            "region",
            &PAGERDUTY_REGIONS,
        )]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        PagerDutyConfig::parse(config)?.validate()
    }

    /// Events go to the integration of the routing key: a target is taken as the generic webhook
    /// takes one.
    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        WebhookConnectorAdapter::new().validate_target(target)
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = PagerDutyConfig::parse(config)?;
        config.validate()?;
        self.validate_target(target)?;

        let url = format!("{}/v2/enqueue", config.base_url().trim_end_matches('/'));
        let client = common::client(config.timeout_secs, PAGERDUTY_CONNECTOR_TYPE)?;

        let request = client
            .post(url)
            .header("accept", "application/json")
            .json(&event(config.routing_key.trim(), message));

        let started = Instant::now();

        let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("pagerduty request failed"))?;

        let status = response.status();
        let answer = response.json::<Value>().await.unwrap_or(Value::Null);

        if !status.is_success() {
            return Err(refused(status.as_u16(), &answer));
        }

        let dedup_key = answer
            .get("dedup_key")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(common::delivered(dedup_key, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::connector::{NotifyTargetType, validate_addresses};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    use super::*;

    const KEY: &str = "e93facc04764012d7bfb002500d5d1a6";

    fn config(base: &str) -> Value {
        json!({ "routingKey": KEY, "apiBaseUrl": base })
    }

    fn target() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedGroup,
            value: "platform-oncall".into(),
            metadata: BTreeMap::new(),
        }
    }

    /// A notification about alert `inc-1`, as the notify engine sends it to the webhook channel.
    fn alert(event_type: &str) -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Checkout errors".into(),
            text: "Alert Checkout errors is open.".into(),
            markdown: None,
            html: None,
            metadata: [
                ("alert_id", "inc-1"),
                ("event_type", event_type),
                ("severity", "critical"),
                ("delivery_id", "dlv-1"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
            provider_templates: Default::default(),
            event: Some(json!({
                "version": 2,
                "event": {
                    "id": format!("{event_type}:inc-1"),
                    "type": event_type,
                    "occurredAt": "2025-10-28T00:00:00Z",
                    "organizationId": "org-a",
                },
                "alert": {
                    "id": "inc-1",
                    "severity": "critical",
                    "title": "Checkout errors",
                    "url": "https://ms.example.com/alerts/incidents/inc-1",
                    "service": "checkout",
                },
                "rule": {"id": "rule-1", "name": "Checkout errors"},
            })),
            card: None,
        }
    }

    /// The JSON of the one event the server received.
    async fn enqueued(server: &MockServer) -> Value {
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        serde_json::from_slice(&requests[0].body).unwrap()
    }

    async fn accepting(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v2/enqueue"))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "status": "success",
                "message": "Event processed",
                "dedup_key": "molesignal-inc-1",
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[test]
    fn the_routing_key_is_an_integration_key() {
        let adapter = PagerDutyConnectorAdapter::new();

        assert!(
            adapter
                .validate_config(&json!({ "routingKey": KEY }))
                .is_ok()
        );
        assert!(
            adapter
                .validate_config(&json!({ "routingKey": KEY, "apiBaseUrl": "" }))
                .is_ok()
        );
        assert!(adapter.validate_config(&json!({})).is_err());
        assert!(
            adapter
                .validate_config(&json!({ "routingKey": "short" }))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({ "routingKey": format!("{}-", &KEY[..31]) }))
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({ "routingKey": KEY, "timeoutSecs": 0 }))
                .is_err()
        );
    }

    #[test]
    fn the_region_picks_the_events_address() {
        let addresses = PagerDutyConnectorAdapter::new().addresses();
        let admitted = |url: &str| {
            validate_addresses("pagerduty", &addresses, &json!({ "apiBaseUrl": url })).is_ok()
        };

        assert_eq!(addresses[0].picker, Some("region"));
        assert!(admitted("https://events.pagerduty.com"));
        assert!(admitted("https://events.eu.pagerduty.com/"));
        assert!(!admitted("https://events.pagerduty.example"));
    }

    #[test]
    fn severities_are_pagerdutys_own() {
        let with = |label: &str| {
            let mut message = alert("alert.triggered");
            message.metadata.insert("severity".into(), label.into());
            severity(&message)
        };

        assert_eq!(with("critical"), "critical");
        assert_eq!(with("error"), "error");
        assert_eq!(with("warn"), "warning");
        assert_eq!(with("info"), "info");

        let mut shift = alert("oncall.shift.starting");
        shift.metadata.clear();
        shift.event = Some(json!({"version": 2, "data": {"schedule_id": "sch-1"}}));
        assert_eq!(severity(&shift), "info");
    }

    #[test]
    fn the_summary_is_the_title_within_pagerdutys_limit() {
        let mut message = alert("alert.triggered");
        assert_eq!(summary(&message), "CRITICAL · Checkout errors");

        message.title = "x".repeat(2_000);
        assert_eq!(summary(&message).chars().count(), MAX_SUMMARY_CHARS);

        message.title = String::new();
        message.text = "\n  Disk full\nmore".into();
        assert_eq!(summary(&message), "Disk full");
    }

    #[tokio::test]
    async fn a_triggered_alert_triggers_its_pagerduty_alert() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v2/enqueue"))
            .and(header("content-type", "application/json"))
            .and(body_partial_json(json!({
                "routing_key": KEY,
                "event_action": "trigger",
                "dedup_key": "molesignal-inc-1",
                "client": "MoleSignal",
                "client_url": "https://ms.example.com/alerts/incidents/inc-1",
                "links": [{"href": "https://ms.example.com/alerts/incidents/inc-1", "text": "MoleSignal"}],
                "payload": {
                    "summary": "CRITICAL · Checkout errors",
                    "source": "checkout",
                    "severity": "critical",
                    "timestamp": "2025-10-28T00:00:00Z",
                },
            })))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "status": "success",
                "message": "Event processed",
                "dedup_key": "molesignal-inc-1",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = PagerDutyConnectorAdapter::new()
            .send(&config(&server.uri()), &target(), &alert("alert.triggered"))
            .await
            .unwrap();

        assert_eq!(
            result.provider_message_id.as_deref(),
            Some("molesignal-inc-1")
        );

        let event = enqueued(&server).await;
        let details = &event["payload"]["custom_details"];
        assert_eq!(details["alert"]["id"], "inc-1");
        assert_eq!(details["rule"]["name"], "Checkout errors");
        assert!(details.get("version").is_none());
    }

    #[tokio::test]
    async fn an_escalation_triggers_the_same_alert() {
        let server = MockServer::start().await;
        accepting(&server).await;

        PagerDutyConnectorAdapter::new()
            .send(&config(&server.uri()), &target(), &alert("alert.escalated"))
            .await
            .unwrap();

        let event = enqueued(&server).await;
        assert_eq!(event["event_action"], "trigger");
        assert_eq!(event["dedup_key"], "molesignal-inc-1");
    }

    #[tokio::test]
    async fn acknowledging_and_resolving_name_the_alert_alone() {
        for (event_type, action) in [
            ("alert.acknowledged", "acknowledge"),
            ("alert.resolved", "resolve"),
        ] {
            let server = MockServer::start().await;
            accepting(&server).await;

            PagerDutyConnectorAdapter::new()
                .send(&config(&server.uri()), &target(), &alert(event_type))
                .await
                .unwrap();

            assert_eq!(
                enqueued(&server).await,
                json!({
                    "routing_key": KEY,
                    "event_action": action,
                    "dedup_key": "molesignal-inc-1",
                }),
                "{event_type}"
            );
        }
    }

    #[tokio::test]
    async fn another_event_triggers_an_alert_of_its_own() {
        let server = MockServer::start().await;
        accepting(&server).await;

        let shift = NotifyMessage {
            title: "Your on-call shift starts in 15 minutes".into(),
            text: String::new(),
            markdown: None,
            html: None,
            metadata: [(
                "event_type".to_string(),
                "oncall.shift.starting".to_string(),
            )]
            .into(),
            provider_templates: Default::default(),
            event: Some(json!({
                "version": 2,
                "event": {"id": "oncall.shift.starting:sch-1:42", "type": "oncall.shift.starting"},
                "data": {"schedule_id": "sch-1"},
            })),
            card: None,
        };

        PagerDutyConnectorAdapter::new()
            .send(&config(&server.uri()), &target(), &shift)
            .await
            .unwrap();

        let event = enqueued(&server).await;
        assert_eq!(event["event_action"], "trigger");
        assert_eq!(event["dedup_key"], "oncall.shift.starting:sch-1:42");
        assert_eq!(event["payload"]["severity"], "info");
        assert_eq!(event["payload"]["source"], "MoleSignal");
        assert_eq!(
            event["payload"]["custom_details"]["data"]["schedule_id"],
            "sch-1"
        );
        assert!(event.get("links").is_none());
    }

    #[tokio::test]
    async fn a_bad_event_is_invalid_and_a_rate_limit_asks_to_wait() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_partial_json(json!({"event_action": "trigger"})))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "status": "invalid event",
                "message": "Event object is invalid",
                "errors": ["Length of 'routing_key' is incorrect (should be 32 characters)"],
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(body_partial_json(json!({"event_action": "resolve"})))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let adapter = PagerDutyConnectorAdapter::new();

        let invalid = adapter
            .send(&config(&server.uri()), &target(), &alert("alert.triggered"))
            .await
            .unwrap_err();

        assert!(matches!(invalid, Error::InvalidArgument(_)), "{invalid}");
        assert!(
            invalid
                .to_string()
                .contains("Event object is invalid: Length of 'routing_key' is incorrect"),
            "{invalid}"
        );

        let limited = adapter
            .send(&config(&server.uri()), &target(), &alert("alert.resolved"))
            .await
            .unwrap_err();

        assert!(matches!(limited, Error::Unavailable(_)), "{limited}");
        assert!(limited.to_string().contains("retry"), "{limited}");
    }

    #[tokio::test]
    async fn a_test_without_an_event_still_triggers_with_a_summary() {
        let server = MockServer::start().await;
        accepting(&server).await;

        let test = NotifyMessage {
            title: "MoleSignal test".into(),
            text: "It works.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };

        PagerDutyConnectorAdapter::new()
            .send(&config(&server.uri()), &target(), &test)
            .await
            .unwrap();

        let event = enqueued(&server).await;
        assert_eq!(event["event_action"], "trigger");
        assert!(event.get("dedup_key").is_none());
        assert_eq!(event["payload"]["summary"], "MoleSignal test");
        assert_eq!(event["payload"]["custom_details"]["text"], "It works.");
    }
}
