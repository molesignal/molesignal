// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Cloud Pub/Sub subscriptions: pushed one message per request, or read by pull
//! ([`pull`]).
//!
//! A push request is wrapped by default:
//!
//! ```json
//! {"message": {"data": "<base64>", "attributes": {...}, "messageId": "...",
//!              "publishTime": "2026-11-24T10:00:00.123Z"},
//!  "subscription": "projects/acme/subscriptions/molesignal"}
//! ```
//!
//! With payload unwrapping on, the body is the message data itself and the metadata travels in
//! `x-goog-pubsub-*` headers. Either way the data is read by [`log_entry`]: a Cloud Logging
//! `LogEntry`, as a Log Router sink publishes it, or any other JSON or text. See
//! <https://cloud.google.com/pubsub/docs/push> and
//! <https://cloud.google.com/pubsub/docs/payload-unwrapping>.

use base64::Engine as _;
use domain::intake::RawEvent;
use kernel::{Error, Result, time::TimestampMicros};
use serde::Deserialize;

pub mod log_entry;
pub mod oidc;
pub mod pull;

#[derive(Debug, Deserialize)]
struct Envelope {
    message: Message,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    #[serde(default)]
    data: String,
    #[serde(default)]
    message_id: String,
    #[serde(default, alias = "publish_time")]
    publish_time: String,
}

/// One pushed message, unwrapped.
#[derive(Debug, PartialEq)]
pub struct Pushed {
    pub message_id: String,
    pub data: Vec<u8>,
    /// When Pub/Sub took the message: the event's time when the data has none.
    pub published_at: Option<TimestampMicros>,
}

/// The message of a push request. `unwrapped` is the `x-goog-pubsub-message-id` and
/// `x-goog-pubsub-publish-time` of a subscription that unwraps payloads; without it the body is
/// read as the wrapped envelope.
pub fn pushed(body: &[u8], unwrapped: Option<(&str, &str)>) -> Result<Pushed> {
    if let Some((message_id, publish_time)) = unwrapped {
        return Ok(Pushed {
            message_id: message_id.to_string(),
            data: body.to_vec(),
            published_at: log_entry::rfc3339(publish_time),
        });
    }

    let envelope: Envelope = serde_json::from_slice(body)
        .map_err(|e| Error::invalid(format!("pub/sub push body: {e}")))?;

    let data = base64::engine::general_purpose::STANDARD
        .decode(envelope.message.data.as_bytes())
        .map_err(|e| Error::invalid(format!("pub/sub message data: {e}")))?;

    Ok(Pushed {
        message_id: envelope.message.message_id,
        data,
        published_at: log_entry::rfc3339(&envelope.message.publish_time),
    })
}

/// The log event a pushed message holds; `None` when its data is empty.
pub fn event(pushed: &Pushed) -> Option<RawEvent> {
    log_entry::event(&pushed.data, pushed.published_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    #[test]
    fn a_wrapped_message_is_unpacked() {
        let body = format!(
            r#"{{"message":{{"data":"{}","attributes":{{"a":"b"}},"messageId":"42",
                "publishTime":"2026-11-24T10:00:00.5Z"}},"subscription":"projects/p/subscriptions/s"}}"#,
            encode("hello")
        );

        let pushed = pushed(body.as_bytes(), None).unwrap();

        assert_eq!(pushed.message_id, "42");
        assert_eq!(pushed.data, b"hello");
        assert_eq!(pushed.published_at, rfc("2026-11-24T10:00:00.5Z"));
    }

    #[test]
    fn an_unwrapped_message_is_the_body_itself() {
        let pushed = pushed(b"{\"msg\":\"hi\"}", Some(("7", "2026-11-24T10:00:00Z"))).unwrap();

        assert_eq!(pushed.message_id, "7");
        assert_eq!(pushed.data, b"{\"msg\":\"hi\"}");
    }

    #[test]
    fn a_body_that_is_no_envelope_is_refused() {
        assert!(pushed(b"{\"msg\":\"hi\"}", None).is_err());
        assert!(pushed(br#"{"message":{"data":"not base64!"}}"#, None).is_err());
    }

    fn rfc(text: &str) -> Option<TimestampMicros> {
        log_entry::rfc3339(text)
    }
}
