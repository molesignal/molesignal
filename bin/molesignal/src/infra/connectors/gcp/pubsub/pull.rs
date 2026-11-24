// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reading a Pub/Sub subscription by pull, for more logs than a push subscription carries well:
//! a push request holds one message, a pull up to [`MAX_MESSAGES`].
//!
//! Each pull asks the subscription for what it holds (`returnImmediately`, so an idle one answers
//! at once), reads every message as a pushed one is read ([`super::event`]), and hands the
//! acknowledgement IDs back as the batch's receipt. The runner acknowledges them only once the
//! batch is written ([`PullConnector::acknowledge`]); what was not written comes back when its
//! acknowledgement deadline passes, so a message may be written twice but is not lost. Give the
//! subscription a deadline of 60 seconds or more.
//!
//! Settings (`config_json`): `serviceAccountKey` (a key of a service account with
//! `roles/pubsub.subscriber` on the subscription), `subscription` (its full name,
//! `projects/<project>/subscriptions/<name>`, or its name in the key's project), `targetStream`.
//! See <https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/pull>.

use async_trait::async_trait;
use base64::Engine as _;
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use super::{Pushed, event, log_entry::rfc3339};
use crate::infra::connectors::{
    ConnectorKind, PullBatch, PullConnector, PullContext, cloud_metrics,
    gcp::service_account::{ServiceAccount, TokenSource},
    repo::Connector,
};

const KIND: ConnectorKind = ConnectorKind::GcpPubsubPull;
const SCOPE: &str = "https://www.googleapis.com/auth/pubsub";
const API: &str = "google cloud pub/sub";

/// How many messages one pull asks for: Pub/Sub's own limit.
pub const MAX_MESSAGES: usize = 1_000;

/// The settings a pull needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    account: ServiceAccount,
    /// `projects/<project>/subscriptions/<name>`.
    subscription: String,
}

impl Settings {
    fn parse(config: &Value) -> Result<Self> {
        let account = ServiceAccount::parse(config, KIND)?;
        let named = cloud_metrics::required(config, "subscription", KIND)?;

        let subscription = if named.starts_with("projects/") {
            named.to_string()
        } else if account.project_id.is_empty() {
            return Err(Error::invalid(
                "`subscription` must be a full name, projects/<project>/subscriptions/<name>, when \
                 the service account key names no project",
            ));
        } else {
            format!("projects/{}/subscriptions/{named}", account.project_id)
        };

        let parts: Vec<&str> = subscription.split('/').collect();
        let valid_part = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "-_.~+%".contains(ch))
        };

        // The name goes into a URL path: nothing but a project and a subscription may pass.
        if parts.len() != 4
            || parts[0] != "projects"
            || parts[2] != "subscriptions"
            || !valid_part(parts[1])
            || !valid_part(parts[3])
        {
            return Err(Error::invalid(format!(
                "`subscription` must be projects/<project>/subscriptions/<name>, not `{subscription}`"
            )));
        }

        Ok(Self {
            account,
            subscription,
        })
    }
}

/// Rejects settings that could never be pulled with.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct GcpPubsubPull {
    settings: std::result::Result<Settings, String>,
}

impl GcpPubsubPull {
    pub fn new(connector: &Connector) -> Self {
        Self {
            settings: Settings::parse(&connector.config_json).map_err(|e| e.to_string()),
        }
    }

    fn settings(&self) -> Result<&Settings> {
        self.settings
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))
    }

    /// Whether the key may consume the subscription, asked without taking any message.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let answer = call(
            settings,
            ":testIamPermissions",
            json!({ "permissions": ["pubsub.subscriptions.consume"] }),
        )
        .await?;

        let held = answer
            .get("permissions")
            .and_then(Value::as_array)
            .is_some_and(|held| held.iter().any(|p| p == "pubsub.subscriptions.consume"));

        if held {
            Ok(())
        } else {
            Err(Error::forbidden(format!(
                "the service account may not consume {}: grant it roles/pubsub.subscriber",
                settings.subscription
            )))
        }
    }
}

#[async_trait]
impl PullConnector for GcpPubsubPull {
    async fn run_once(&self, _ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let answer = call(
            settings,
            ":pull",
            json!({ "maxMessages": MAX_MESSAGES, "returnImmediately": true }),
        )
        .await?;

        let (events, ack_ids, skipped) = read(&answer);

        Ok(PullBatch {
            events,
            more: ack_ids.len() >= MAX_MESSAGES,
            warning: (skipped > 0)
                .then(|| format!("{skipped} messages whose data was not base64 were left out")),
            receipt: (!ack_ids.is_empty()).then(|| json!(ack_ids)),
            ..PullBatch::default()
        })
    }

    async fn acknowledge(&self, receipt: Value) -> Result<()> {
        let settings = self.settings()?;
        let ack_ids: Vec<String> = serde_json::from_value(receipt).unwrap_or_default();

        for chunk in ack_ids.chunks(MAX_MESSAGES) {
            call(settings, ":acknowledge", json!({ "ackIds": chunk })).await?;
        }

        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Received {
    #[serde(default)]
    ack_id: String,
    #[serde(default)]
    message: Message,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    #[serde(default)]
    data: String,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    publish_time: String,
}

/// The log events of a pull answer, the acknowledgement IDs of every message (unreadable ones
/// too, so they are not delivered again and again), and how many were unreadable.
fn read(answer: &Value) -> (Vec<domain::intake::RawEvent>, Vec<String>, usize) {
    let received: Vec<Received> = answer
        .get("receivedMessages")
        .cloned()
        .and_then(|messages| serde_json::from_value(messages).ok())
        .unwrap_or_default();

    let mut events = Vec::with_capacity(received.len());
    let mut ack_ids = Vec::with_capacity(received.len());
    let mut skipped = 0;

    for message in received {
        ack_ids.push(message.ack_id);

        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(&message.message.data)
        else {
            skipped += 1;
            continue;
        };

        let pushed = Pushed {
            message_id: message.message.message_id,
            data,
            published_at: rfc3339(&message.message.publish_time),
        };

        events.extend(event(&pushed));
    }

    (events, ack_ids, skipped)
}

/// One `POST https://pubsub.googleapis.com/v1/<subscription><verb>`.
async fn call(settings: &Settings, verb: &str, body: Value) -> Result<Value> {
    let http = cloud_metrics::http_client()?;
    let tokens = TokenSource::new(&settings.account, SCOPE);
    let token = tokens.token(&http).await?;
    let url = format!(
        "https://pubsub.googleapis.com/v1/{}{verb}",
        settings.subscription
    );

    let request = http.post(url).bearer_auth(token).json(&body);

    let response = http_trace::send(&http, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{API} request: {e}")))?;

    let status = response.status().as_u16();

    let text = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{API} response: {e}")))?;

    if status == 401 {
        tokens.forget().await;
    }

    if !(200..300).contains(&status) {
        return Err(cloud_metrics::refusal(API, status, &text));
    }

    serde_json::from_str(&text).map_err(|e| Error::unavailable(format!("{API} response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::connectors::gcp::service_account::test_key;

    fn config(subscription: &str) -> Value {
        json!({
            "serviceAccountKey": {
                "client_email": "reader@acme.iam.gserviceaccount.com",
                "private_key": test_key(),
                "project_id": "acme",
            },
            "subscription": subscription,
        })
    }

    #[test]
    fn a_subscription_is_a_full_name_or_one_in_the_keys_project() {
        assert_eq!(
            Settings::parse(&config("logs")).unwrap().subscription,
            "projects/acme/subscriptions/logs"
        );
        assert_eq!(
            Settings::parse(&config("projects/other/subscriptions/logs"))
                .unwrap()
                .subscription,
            "projects/other/subscriptions/logs"
        );
        assert!(Settings::parse(&config("projects/acme/topics/logs")).is_err());
        assert!(Settings::parse(&config("logs?alt=media")).is_err());
        assert!(Settings::parse(&config("")).is_err());
    }

    #[test]
    fn a_pull_answer_gives_log_events_and_every_acknowledgement_id() {
        let entry = base64::engine::general_purpose::STANDARD.encode(
            r#"{"logName":"projects/acme/logs/stdout","resource":{"type":"k8s_container"},
                "textPayload":"hello","timestamp":"2026-11-24T10:00:00Z"}"#,
        );

        let answer = json!({"receivedMessages": [
            {"ackId": "a1", "message": {"data": entry, "messageId": "1",
                                         "publishTime": "2026-11-24T10:00:01Z"}},
            {"ackId": "a2", "message": {"data": "not base64!", "messageId": "2"}},
        ]});

        let (events, ack_ids, skipped) = read(&answer);

        assert_eq!(ack_ids, ["a1", "a2"]);
        assert_eq!(skipped, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].fields["message"], "hello");
        assert_eq!(events[0].fields["resource_type"], "k8s_container");
    }

    #[test]
    fn an_empty_answer_reads_nothing() {
        let (events, ack_ids, skipped) = read(&json!({}));

        assert!(events.is_empty() && ack_ids.is_empty());
        assert_eq!(skipped, 0);
    }
}
