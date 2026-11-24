// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Cloud Logging `LogEntry` as a log event.
//!
//! A Log Router sink publishes each entry as the JSON of
//! <https://cloud.google.com/logging/docs/reference/v2/rest/v2/LogEntry>. Its fields become:
//!
//! - the payload: the fields of `jsonPayload`; `textPayload` as `message`; `protoPayload` (audit
//!   logs) kept whole as `proto_payload`, with `method_name`, `service_name` and `principal_email`
//!   taken out of it;
//! - `level` from `severity`, lower-cased; `log_name`; `resource_type`; the labels of the resource
//!   and of the entry under their own names; `trace_id` and `span_id`, the trace without its
//!   `projects/<id>/traces/` prefix; `insert_id`; `http_request` kept whole;
//! - the time from `timestamp`, else `receiveTimestamp`, else the time Pub/Sub took the message.
//!
//! Data that is not a `LogEntry` is read like any pushed line: a JSON object gives the fields,
//! anything else is `message`.

use chrono::DateTime;
use domain::intake::RawEvent;
use kernel::time::TimestampMicros;
use serde_json::{Map, Value};

/// The log event `data` holds; `None` when it is empty.
pub fn event(data: &[u8], published_at: Option<TimestampMicros>) -> Option<RawEvent> {
    let text = String::from_utf8_lossy(data);
    let text = text.trim();

    if text.is_empty() {
        return None;
    }

    let event = match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(entry)) if is_log_entry(&entry) => from_log_entry(entry, published_at),
        Ok(Value::Object(fields)) => RawEvent {
            timestamp: published_at.unwrap_or_else(TimestampMicros::now),
            fields,
        },
        _ => RawEvent {
            timestamp: published_at.unwrap_or_else(TimestampMicros::now),
            fields: Map::from_iter([("message".to_string(), Value::String(text.into()))]),
        },
    };

    Some(event)
}

/// Whether an object reads as a `LogEntry`: it names its log and its resource.
fn is_log_entry(entry: &Map<String, Value>) -> bool {
    entry.get("logName").is_some_and(Value::is_string) && entry.get("resource").is_some()
}

fn from_log_entry(
    mut entry: Map<String, Value>,
    published_at: Option<TimestampMicros>,
) -> RawEvent {
    let timestamp = ["timestamp", "receiveTimestamp"]
        .iter()
        .find_map(|key| entry.get(*key).and_then(Value::as_str).and_then(rfc3339))
        .or(published_at)
        .unwrap_or_else(TimestampMicros::now);

    let mut fields = match entry.remove("jsonPayload") {
        Some(Value::Object(payload)) => payload,
        _ => Map::new(),
    };

    if let Some(Value::String(text)) = entry.remove("textPayload") {
        fields.insert("message".into(), Value::String(text));
    }

    if let Some(payload) = entry.remove("protoPayload") {
        for (field, pointer) in [
            ("method_name", "/methodName"),
            ("service_name", "/serviceName"),
            ("principal_email", "/authenticationInfo/principalEmail"),
        ] {
            if let Some(value) = payload.pointer(pointer).filter(|value| value.is_string()) {
                fields.insert(field.into(), value.clone());
            }
        }

        fields.insert("proto_payload".into(), payload);
    }

    let resource = entry.remove("resource");

    for labels in [
        entry.get("labels"),
        resource
            .as_ref()
            .and_then(|resource| resource.get("labels")),
    ] {
        for (key, value) in labels.and_then(Value::as_object).into_iter().flatten() {
            fields.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }

    let text = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };

    let named = [
        ("log_name", text("logName").map(str::to_string)),
        (
            "resource_type",
            resource
                .as_ref()
                .and_then(|resource| resource.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        ("level", text("severity").map(str::to_ascii_lowercase)),
        (
            "trace_id",
            text("trace").map(|trace| trace.rsplit('/').next().unwrap_or(trace).to_string()),
        ),
        ("span_id", text("spanId").map(str::to_string)),
        ("insert_id", text("insertId").map(str::to_string)),
    ];

    for (field, value) in named {
        if let Some(value) = value {
            fields.insert(field.into(), Value::String(value));
        }
    }

    if let Some(request) = entry.remove("httpRequest") {
        fields.insert("http_request".into(), request);
    }

    RawEvent { timestamp, fields }
}

/// An RFC 3339 time, as Cloud Logging and Pub/Sub write them.
pub fn rfc3339(text: &str) -> Option<TimestampMicros> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| TimestampMicros(time.timestamp_micros()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_payload_entry_keeps_its_fields_and_where_it_came_from() {
        let entry = br#"{
            "logName": "projects/acme/logs/stdout",
            "resource": {"type": "k8s_container",
                         "labels": {"project_id": "acme", "pod_name": "checkout-1"}},
            "labels": {"k8s-pod/app": "checkout"},
            "timestamp": "2026-11-24T10:00:00.250Z",
            "receiveTimestamp": "2026-11-24T10:00:01Z",
            "severity": "ERROR",
            "insertId": "abc",
            "trace": "projects/acme/traces/4bf92f3577b34da6a3ce929d0e0e4736",
            "spanId": "00f067aa0ba902b7",
            "jsonPayload": {"message": "payment failed", "order": 42}
        }"#;

        let event = event(entry, None).unwrap();
        let fields = &event.fields;

        assert_eq!(
            event.timestamp,
            rfc3339("2026-11-24T10:00:00.250Z").unwrap()
        );
        assert_eq!(fields["message"], "payment failed");
        assert_eq!(fields["order"], 42);
        assert_eq!(fields["level"], "error");
        assert_eq!(fields["log_name"], "projects/acme/logs/stdout");
        assert_eq!(fields["resource_type"], "k8s_container");
        assert_eq!(fields["pod_name"], "checkout-1");
        assert_eq!(fields["k8s-pod/app"], "checkout");
        assert_eq!(fields["trace_id"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(fields["span_id"], "00f067aa0ba902b7");
        assert_eq!(fields["insert_id"], "abc");
    }

    #[test]
    fn an_audit_entry_keeps_its_proto_payload_and_names_who_did_what() {
        let entry = br#"{
            "logName": "projects/acme/logs/cloudaudit.googleapis.com%2Factivity",
            "resource": {"type": "gce_instance", "labels": {}},
            "receiveTimestamp": "2026-11-24T10:00:01Z",
            "protoPayload": {"methodName": "v1.compute.instances.delete",
                             "serviceName": "compute.googleapis.com",
                             "authenticationInfo": {"principalEmail": "ops@acme.com"}}
        }"#;

        let event = event(entry, None).unwrap();

        assert_eq!(event.timestamp, rfc3339("2026-11-24T10:00:01Z").unwrap());
        assert_eq!(event.fields["method_name"], "v1.compute.instances.delete");
        assert_eq!(event.fields["service_name"], "compute.googleapis.com");
        assert_eq!(event.fields["principal_email"], "ops@acme.com");
        assert!(event.fields["proto_payload"].is_object());
    }

    #[test]
    fn a_text_payload_is_the_message() {
        let entry =
            br#"{"logName": "projects/acme/logs/syslog", "resource": {"type": "gce_instance"},
                         "textPayload": "kernel: oom-killer"}"#;

        let published = rfc3339("2026-11-24T09:00:00Z");
        let event = event(entry, published).unwrap();

        assert_eq!(event.fields["message"], "kernel: oom-killer");
        assert_eq!(Some(event.timestamp), published);
    }

    #[test]
    fn other_data_is_read_like_any_pushed_line() {
        let published = rfc3339("2026-11-24T09:00:00Z");

        assert_eq!(event(b"{\"a\":1}", published).unwrap().fields["a"], 1);
        assert_eq!(
            event(b"plain", published).unwrap().fields["message"],
            "plain"
        );
        assert!(event(b"  ", published).is_none());
    }
}
