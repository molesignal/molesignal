// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Cloud Monitoring incidents: a webhook notification channel, schema version 1.2
//! (<https://cloud.google.com/monitoring/support/notification-options#webhooks>).
//!
//! - The Pub/Sub channel publishes the same JSON. Through a push subscription it arrives in the
//!   Pub/Sub envelope, `{"message": {"data": "<base64>"}}`, which is unpacked first.
//! - `incident.state`: `open` triggers, `closed` resolves.
//! - One incident per Monitoring incident: the dedup key is the scoping project and
//!   `incident_id`.
//! - `severity` (`Critical`, `Error`, `Warning`) when the policy sets one; `No severity` takes the
//!   source's default.
//! - `started_at` and `ended_at` are Unix seconds.

use std::collections::BTreeMap;

use base64::Engine as _;
use domain::alerting::source::{ExternalAction, ExternalAlert, severity_from_word};
use kernel::{Error, Result};
use serde_json::Value;

use super::{Parsed, epoch, field, json, strings, web_address};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = unpacked(json(body)?)?;

    let Some(incident) = payload.get("incident").filter(|value| value.is_object()) else {
        return Err(Error::invalid(
            "not a Cloud Monitoring notification: the body has no `incident`",
        ));
    };

    let Some(incident_id) = field(incident, "incident_id") else {
        return Err(Error::invalid(
            "the Cloud Monitoring incident has no `incident_id`",
        ));
    };

    let (action, time_key) = match field(incident, "state").as_deref() {
        Some("open") => (ExternalAction::Trigger, "started_at"),
        Some("closed") => (ExternalAction::Resolve, "ended_at"),
        other => {
            return Ok(Parsed::nothing(format!(
                "Cloud Monitoring incident state {} is neither open nor closed",
                other.unwrap_or("(none)")
            )));
        }
    };

    let project = field(incident, "scoping_project_id").unwrap_or_default();

    let occurred_at = incident
        .get(time_key)
        .and_then(epoch)
        .or_else(|| incident.get("started_at").and_then(epoch));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{project}|{incident_id}"),
        summary: summary(incident),
        severity: field(incident, "severity")
            .as_deref()
            .and_then(severity_from_word),
        labels: labels(incident),
        annotations: annotations(incident),
        occurred_at,
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// The notification itself, or the one a Pub/Sub push envelope carries.
fn unpacked(payload: Value) -> Result<Value> {
    if payload.get("incident").is_some() {
        return Ok(payload);
    }

    let Some(data) = payload.pointer("/message/data").and_then(Value::as_str) else {
        return Ok(payload);
    };

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| Error::invalid("the Pub/Sub message data is not Base64"))?;

    json(&decoded)
}

/// `policy · resource`; the policy alone without a resource name.
fn summary(incident: &Value) -> String {
    let policy = field(incident, "policy_name")
        .or_else(|| field(incident, "condition_name"))
        .unwrap_or_else(|| "Cloud Monitoring incident".into());

    match field(incident, "resource_display_name").or_else(|| field(incident, "resource_name")) {
        Some(resource) => format!("{policy} · {resource}"),
        None => policy,
    }
}

fn labels(incident: &Value) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();

    for set in [
        incident.pointer("/resource/labels"),
        incident.pointer("/metric/labels"),
        incident.pointer("/metadata/user_labels"),
        incident.get("policy_user_labels"),
    ] {
        for (key, value) in strings(set) {
            labels.entry(key).or_insert(value);
        }
    }

    let extra = [
        ("gcp.project_id", field(incident, "scoping_project_id")),
        (
            "gcp.resource_type",
            incident
                .pointer("/resource/type")
                .and_then(super::text)
                .or_else(|| field(incident, "resource_type")),
        ),
        (
            "gcp.metric",
            incident.pointer("/metric/type").and_then(super::text),
        ),
        ("gcp.policy", field(incident, "policy_name")),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    labels
}

fn annotations(incident: &Value) -> BTreeMap<String, String> {
    [
        ("message", field(incident, "summary")),
        ("condition", field(incident, "condition_name")),
        ("value", field(incident, "observed_value")),
        ("threshold", field(incident, "threshold_value")),
        (
            "documentation",
            incident
                .pointer("/documentation/content")
                .and_then(super::text),
        ),
        (
            "gcp_url",
            field(incident, "url").and_then(|url| web_address(&url)),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

#[cfg(test)]
mod tests {
    use domain::alerting::incident::Severity;
    use kernel::time::TimestampMicros;
    use serde_json::json;

    use super::*;

    /// The schema 1.2 example of the Cloud Monitoring webhook docs.
    fn notification(state: &str) -> Value {
        let ended_at = if state == "closed" {
            json!(1577877071)
        } else {
            Value::Null
        };

        json!({
            "incident": {
                "incident_id": "0.opqiw61fsv7p",
                "scoping_project_id": "internal-project",
                "scoping_project_number": 12345,
                "url": "https://console.cloud.google.com/monitoring/alerting/incidents/0.lxfiw61fsv7p?project=internal-project",
                "severity": "Critical",
                "started_at": 1577840461,
                "ended_at": ended_at,
                "state": state,
                "summary": "CPU utilization for internal-project my-instance is above the threshold of 0.8 with a value of 0.994.",
                "apigee_url": "https://apigee.com/dashboard/resource?project=internal-project",
                "observed_value": "0.994",
                "resource": {
                    "type": "gce_instance",
                    "labels": {"instance_id": "12345", "project_id": "internal-project", "zone": "us-central1-c"}
                },
                "resource_type_display_name": "VM Instance",
                "resource_id": "12345",
                "resource_display_name": "my-instance",
                "resource_name": "internal-project my-instance",
                "metric": {
                    "type": "compute.googleapis.com/instance/cpu/utilization",
                    "displayName": "CPU utilization",
                    "labels": {"instance_name": "my-instance"}
                },
                "metadata": {
                    "system_labels": {"labelkey": "labelvalue"},
                    "user_labels": {"team": "checkout"}
                },
                "policy_name": "Monitor-Project-Cluster",
                "policy_user_labels": {"user-label-1": "important label"},
                "documentation": {
                    "content": "CPU is high: check the instance.",
                    "mime_type": "text/markdown",
                    "subject": "ALERT - Critical"
                },
                "condition": {
                    "name": "projects/internal-project/alertPolicies/1234567890123456789/conditions/1234567890123456789",
                    "displayName": "VM Instance - CPU utilization"
                },
                "condition_name": "VM Instance - CPU utilization",
                "threshold_value": "0.8"
            },
            "version": "1.2"
        })
    }

    fn one(body: &[u8]) -> ExternalAlert {
        parse(body).unwrap().alerts.into_iter().next().unwrap()
    }

    #[test]
    fn an_open_incident_triggers_with_its_resource_and_policy() {
        let alert = one(notification("open").to_string().as_bytes());

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.dedup_key, "internal-project|0.opqiw61fsv7p");
        assert_eq!(alert.summary, "Monitor-Project-Cluster · my-instance");
        assert_eq!(alert.severity, Some(Severity::Critical));
        assert_eq!(alert.labels["zone"], "us-central1-c");
        assert_eq!(alert.labels["instance_name"], "my-instance");
        assert_eq!(alert.labels["team"], "checkout");
        assert_eq!(alert.labels["user-label-1"], "important label");
        assert_eq!(alert.labels["gcp.project_id"], "internal-project");
        assert_eq!(alert.labels["gcp.resource_type"], "gce_instance");
        assert_eq!(
            alert.labels["gcp.metric"],
            "compute.googleapis.com/instance/cpu/utilization"
        );
        assert_eq!(alert.annotations["value"], "0.994");
        assert_eq!(alert.annotations["threshold"], "0.8");
        assert!(alert.annotations["gcp_url"].starts_with("https://console.cloud.google.com/"));
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_secs(1_577_840_461))
        );
    }

    #[test]
    fn a_closed_incident_resolves_the_same_alert_when_it_ended() {
        let opened = one(notification("open").to_string().as_bytes());
        let closed = one(notification("closed").to_string().as_bytes());

        assert_eq!(closed.action, ExternalAction::Resolve);
        assert_eq!(closed.dedup_key, opened.dedup_key);
        assert_eq!(
            closed.occurred_at,
            Some(TimestampMicros::from_secs(1_577_877_071))
        );
    }

    #[test]
    fn no_severity_takes_the_default() {
        let mut payload = notification("open");
        payload["incident"]["severity"] = json!("No severity");

        assert_eq!(one(payload.to_string().as_bytes()).severity, None);
    }

    #[test]
    fn the_pub_sub_channel_arrives_in_a_push_envelope() {
        let data =
            base64::engine::general_purpose::STANDARD.encode(notification("open").to_string());

        let envelope = json!({
            "message": {"data": data, "messageId": "1", "publishTime": "2020-01-01T01:01:01Z"},
            "subscription": "projects/internal-project/subscriptions/molesignal"
        });

        let alert = one(envelope.to_string().as_bytes());

        assert_eq!(alert.dedup_key, "internal-project|0.opqiw61fsv7p");
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(parse(br#"{"alert": "x"}"#).is_err());
        assert!(parse(br#"{"message": {"data": "not base64!"}}"#).is_err());
    }
}
