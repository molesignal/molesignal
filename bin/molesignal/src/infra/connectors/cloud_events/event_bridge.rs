// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Amazon EventBridge events, as an API destination sends them
//! (<https://docs.aws.amazon.com/eventbridge/latest/ref/overiew-event-structure.html>).
//!
//! `id`, `source`, `detail-type`, `account`, `region` and `time` become `event_id`,
//! `event_source`, `event_type`, `account_id`, `region` and the event's time; `resources` is kept
//! as an array and `detail` is the payload. `version` is left out; any other field keeps its name.

use serde_json::{Map, Value};

use super::{CloudEvent, Named, payload, text, time_of};

/// Fields read into the named ones, or left out.
const ENVELOPE: [&str; 8] = [
    "version",
    "id",
    "detail-type",
    "source",
    "account",
    "time",
    "region",
    "detail",
];

/// Whether an object reads as an EventBridge event: it has a `detail-type`.
pub(super) fn is_event(object: &Map<String, Value>) -> bool {
    object.get("detail-type").is_some_and(Value::is_string)
}

pub(super) fn event(mut object: Map<String, Value>) -> CloudEvent {
    let mut fields = Map::new();

    if let Some(detail) = object.remove("detail") {
        payload(&mut fields, detail, None);
    }

    for (name, value) in &object {
        if !ENVELOPE.contains(&name.as_str()) && !value.is_null() {
            fields.insert(name.clone(), value.clone());
        }
    }

    let named = Named {
        id: text(object.get("id")),
        source: text(object.get("source")),
        kind: text(object.get("detail-type")),
        subject: None,
        account: text(object.get("account")),
        region: text(object.get("region")),
        time: time_of(object.get("time")),
    };

    named.into_event("eventbridge", fields)
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::super::{Delivery, Request, read};

    #[test]
    fn an_ec2_state_change_keeps_its_detail_and_resources() {
        // The example event of the EventBridge user guide.
        let body = br#"{
            "version": "0",
            "id": "6a7e8feb-b491-4cf7-a9f1-bf3703467718",
            "detail-type": "EC2 Instance State-change Notification",
            "source": "aws.ec2",
            "account": "111122223333",
            "time": "2017-12-22T18:43:48Z",
            "region": "us-west-1",
            "resources": ["arn:aws:ec2:us-west-1:123456789012:instance/i-1234567890abcdef0"],
            "detail": {"instance-id": "i-1234567890abcdef0", "state": "terminated"}
        }"#;

        let Delivery::Events(events) = read(body, &Request::default()).unwrap() else {
            panic!("no events");
        };

        let event = &events[0];
        let fields = &event.event.fields;

        assert_eq!(
            event.dedup_key.as_deref(),
            Some("aws.ec2\n6a7e8feb-b491-4cf7-a9f1-bf3703467718")
        );
        assert_eq!(
            event.event.timestamp,
            TimestampMicros::from_secs(1_513_968_228)
        );
        assert_eq!(
            fields["event_type"],
            "EC2 Instance State-change Notification"
        );
        assert_eq!(fields["event_source"], "aws.ec2");
        assert_eq!(fields["account_id"], "111122223333");
        assert_eq!(fields["region"], "us-west-1");
        assert_eq!(
            fields["resources"][0],
            "arn:aws:ec2:us-west-1:123456789012:instance/i-1234567890abcdef0"
        );
        assert_eq!(fields["data"]["state"], "terminated");
        assert_eq!(fields["event_format"], "eventbridge");
        assert!(!fields.contains_key("version"));
        assert!(!fields.contains_key("detail-type"));
    }
}
