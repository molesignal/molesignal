// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A CloudWatch Logs subscription message, as a subscription filter sends it to Firehose.
//!
//! ```json
//! {"messageType": "DATA_MESSAGE", "owner": "111111111111", "logGroup": "/aws/lambda/checkout",
//!  "logStream": "2026/11/24/[$LATEST]abc", "subscriptionFilters": ["molesignal"],
//!  "logEvents": [{"id": "3195...", "timestamp": 1432826855000, "message": "..."}]}
//! ```
//!
//! Each entry of `logEvents` is a log event with its own time. A `CONTROL_MESSAGE` is the probe
//! CloudWatch Logs sends when the subscription is made and carries nothing to keep. The fields are
//! those the CloudWatch Logs pull connector writes (`message`, `log_group`, `log_stream`,
//! `source`), with the account and the region the message came from. See
//! <https://docs.aws.amazon.com/AmazonCloudWatch/latest/logs/SubscriptionFilters.html>.

use domain::intake::RawEvent;
use kernel::time::TimestampMicros;
use serde_json::{Map, Value};

use super::Delivery;

/// The message `bytes` hold, if they are one: a JSON object with a `messageType` and `logEvents`.
pub fn message(bytes: &[u8]) -> Option<Value> {
    if bytes.first() != Some(&b'{') {
        return None;
    }

    let message: Value = serde_json::from_slice(bytes).ok()?;

    (message.get("messageType").is_some_and(Value::is_string) && message.get("owner").is_some())
        .then_some(message)
}

/// The log events of a message: none for a `CONTROL_MESSAGE`.
pub fn events(message: &Value, delivery: &Delivery) -> Vec<RawEvent> {
    if message.get("messageType").and_then(Value::as_str) != Some("DATA_MESSAGE") {
        return Vec::new();
    }

    let text = |key: &str| message.get(key).and_then(Value::as_str);

    let mut shared = Map::new();
    shared.insert("source".into(), Value::String("cloudwatch".into()));

    for (field, value) in [
        ("log_group", text("logGroup")),
        ("log_stream", text("logStream")),
        ("account_id", text("owner")),
        ("region", delivery.region.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            shared.insert(field.into(), Value::String(value.into()));
        }
    }

    delivery.stamp(&mut shared);

    message
        .get("logEvents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            let mut fields = shared.clone();

            if let Some(text) = entry.get("message").and_then(Value::as_str) {
                fields.insert("message".into(), Value::String(text.into()));
            }

            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                fields.insert("event_id".into(), Value::String(id.into()));
            }

            let timestamp = entry
                .get("timestamp")
                .and_then(Value::as_i64)
                .map(|ms| TimestampMicros(ms.saturating_mul(1_000)))
                .unwrap_or_else(TimestampMicros::now);

            RawEvent { timestamp, fields }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        super::{
            FirehoseRequest, decode,
            tests::{gzip, record},
        },
        *,
    };

    const DATA: &str = r#"{"messageType":"DATA_MESSAGE","owner":"111111111111",
        "logGroup":"/aws/lambda/checkout","logStream":"2026/11/24/[$LATEST]abc",
        "subscriptionFilters":["molesignal"],
        "logEvents":[{"id":"1","timestamp":1700000000000,"message":"START"},
                     {"id":"2","timestamp":1700000000500,"message":"{\"level\":\"error\"}"}]}"#;

    fn delivery() -> Delivery {
        Delivery::from_headers(
            Some("arn:aws:firehose:us-east-1:111111111111:deliverystream/logs"),
            None,
        )
    }

    #[test]
    fn each_log_event_keeps_its_time_and_where_it_came_from() {
        let request = FirehoseRequest {
            request_id: "r".into(),
            timestamp: 0,
            records: vec![record(&gzip(DATA.as_bytes()))],
        };

        let decoded = decode(request, &delivery());

        assert_eq!(decoded.rejected, 0);
        assert_eq!(decoded.logs.len(), 2);

        let first = &decoded.logs[0];

        assert_eq!(first.timestamp.0, 1_700_000_000_000_000);
        assert_eq!(first.fields["message"], "START");
        assert_eq!(first.fields["log_group"], "/aws/lambda/checkout");
        assert_eq!(first.fields["log_stream"], "2026/11/24/[$LATEST]abc");
        assert_eq!(first.fields["account_id"], "111111111111");
        assert_eq!(first.fields["region"], "us-east-1");
        assert_eq!(first.fields["source"], "cloudwatch");
        assert_eq!(first.fields["event_id"], "1");
        assert_eq!(decoded.logs[1].timestamp.0, 1_700_000_000_500_000);
    }

    #[test]
    fn a_control_message_carries_nothing() {
        let control = br#"{"messageType":"CONTROL_MESSAGE","owner":"CloudwatchLogs",
            "logGroup":"","logStream":"","subscriptionFilters":[],
            "logEvents":[{"id":"","timestamp":1700000000000,"message":"CWL CONTROL MESSAGE: Checking health of destination Firehose."}]}"#;

        let message = message(control).unwrap();

        assert!(events(&message, &Delivery::default()).is_empty());
    }

    #[test]
    fn other_json_is_not_a_subscription_message() {
        assert!(message(br#"{"msg":"hi"}"#).is_none());
        assert!(message(b"plain").is_none());
    }
}
