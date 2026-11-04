// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 的 Webhook v3：`{"event": {"event_type", "occurred_at", "data": {事件}}}`。
//!
//! - 一个 PagerDuty 事件一个事件，去重键是账号域名（`html_url` 的主机名，美国区是
//!   `<子域>.pagerduty.com`，欧洲区是 `<子域>.eu.pagerduty.com`）加 PagerDuty 的事件 id。
//! - `incident.triggered` 与 `incident.reopened` 触发，`incident.priority_updated` 改严重度，
//!   `incident.acknowledged` 确认，`incident.resolved` 解决。其余事件（备注、转派、
//!   `incident.unacknowledged` 等）读懂了但不处理：MoleSignal 这边确认过的事件保持确认。
//! - 严重度先看优先级（P1…P5），没有再看紧急度：high 为 error，low 为 warning。

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Parsed, field, host_of, json, rfc3339, web_address};
use crate::{
    domain::alerting::{
        incident::Severity,
        priority::Priority,
        source::{ExternalAction, ExternalAlert, severity_from_word},
        urgency::Urgency,
    },
    shared::{Error, Result},
};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = json(body)?;

    let Some(event) = payload.get("event").filter(|event| event.is_object()) else {
        return Err(Error::invalid(
            "not a PagerDuty webhook: the body has no `event` object",
        ));
    };

    let event_type = field(event, "event_type").unwrap_or_default();

    let action = match event_type.as_str() {
        "incident.triggered" | "incident.reopened" | "incident.priority_updated" => {
            ExternalAction::Trigger
        }
        "incident.acknowledged" => ExternalAction::Acknowledge,
        "incident.resolved" => ExternalAction::Resolve,
        "pagey.ping" => return Ok(Parsed::nothing("PagerDuty test event")),
        "" => return Err(Error::invalid("the PagerDuty event has no `event_type`")),
        other => {
            return Ok(Parsed::nothing(format!(
                "PagerDuty event {other} does not change the state of an alert"
            )));
        }
    };

    let Some(incident) = event.get("data").filter(|data| data.is_object()) else {
        return Err(Error::invalid("the PagerDuty event has no `data` object"));
    };

    let Some(id) = field(incident, "id") else {
        return Err(Error::invalid("the PagerDuty incident has no `id`"));
    };

    let page = field(incident, "html_url").and_then(|url| web_address(&url));
    let account = page.as_deref().and_then(host_of).unwrap_or_default();
    let priority = incident
        .get("priority")
        .and_then(|priority| field(priority, "summary"));
    let urgency = field(incident, "urgency");

    let severity = priority
        .as_deref()
        .and_then(severity_from_word)
        .or(match urgency.as_deref() {
            Some("high") => Some(Severity::Error),
            Some("low") => Some(Severity::Warning),
            _ => None,
        });

    let summary = field(incident, "title").unwrap_or_else(|| {
        let number = field(incident, "number").unwrap_or_else(|| id.clone());
        format!("PagerDuty incident #{number}")
    });

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{account}|{id}"),
        summary,
        severity,
        priority: priority.as_deref().and_then(Priority::from_label),
        urgency: urgency.as_deref().and_then(Urgency::from_label),
        labels: labels(incident, &account, priority, urgency),
        annotations: annotations(incident, page),
        occurred_at: field(event, "occurred_at").as_deref().and_then(rfc3339),
        no_data: false,
        members: Vec::new(),
    }]))
}

fn labels(
    incident: &Value,
    account: &str,
    priority: Option<String>,
    urgency: Option<String>,
) -> BTreeMap<String, String> {
    let region = if account.ends_with(".eu.pagerduty.com") {
        "eu"
    } else {
        "us"
    };

    let team = incident
        .get("teams")
        .and_then(Value::as_array)
        .and_then(|teams| teams.first())
        .and_then(|team| field(team, "summary"));

    let service = incident
        .get("service")
        .and_then(|service| field(service, "summary"));

    [
        ("pagerduty.region", Some(region.to_string())),
        ("pagerduty.service", service),
        ("pagerduty.team", team),
        ("pagerduty.priority", priority),
        ("pagerduty.urgency", urgency),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

fn annotations(incident: &Value, page: Option<String>) -> BTreeMap<String, String> {
    [
        ("pagerduty_url", page),
        ("incident_key", field(incident, "incident_key")),
        ("incident_number", field(incident, "number")),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: &str, html_url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "event": {
                "id": "5ac64822-4adc-4fda-ade0-410becf0de4f",
                "event_type": event_type,
                "resource_type": "incident",
                "occurred_at": "2020-10-02T18:45:22.169Z",
                "agent": {"id": "PLH1HKV", "type": "user_reference"},
                "client": null,
                "data": {
                    "id": "PGR0VU2",
                    "type": "incident",
                    "self": "https://api.pagerduty.com/incidents/PGR0VU2",
                    "html_url": html_url,
                    "number": 2,
                    "status": "triggered",
                    "incident_key": "d3640fbd41094207a1c11e58e46b1662",
                    "created_at": "2020-04-09T15:16:27Z",
                    "title": "A little bump in the road",
                    "service": {"id": "PF9KMXH", "summary": "API Service", "type": "service_reference"},
                    "teams": [{"id": "PFCVPS0", "summary": "Engineering", "type": "team_reference"}],
                    "priority": {"id": "PSO75BM", "summary": "P1", "type": "priority_reference"},
                    "urgency": "high",
                    "resolve_reason": null
                }
            }
        }))
        .unwrap()
    }

    const US: &str = "https://acme.pagerduty.com/incidents/PGR0VU2";
    const EU: &str = "https://acme.eu.pagerduty.com/incidents/PGR0VU2";

    #[test]
    fn a_triggered_incident_keyed_by_its_account() {
        let alert = parse(&event("incident.triggered", US))
            .unwrap()
            .alerts
            .remove(0);

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.dedup_key, "acme.pagerduty.com|PGR0VU2");
        assert_eq!(alert.summary, "A little bump in the road");
        assert_eq!(alert.severity, Some(Severity::Critical));
        assert_eq!(alert.labels["pagerduty.region"], "us");
        assert_eq!(alert.labels["pagerduty.service"], "API Service");
        assert_eq!(alert.annotations["pagerduty_url"], US);
        assert_eq!(alert.occurred_at, rfc3339("2020-10-02T18:45:22.169Z"));
    }

    #[test]
    fn the_european_service_region_is_another_account() {
        let alert = parse(&event("incident.triggered", EU))
            .unwrap()
            .alerts
            .remove(0);

        assert_eq!(alert.dedup_key, "acme.eu.pagerduty.com|PGR0VU2");
        assert_eq!(alert.labels["pagerduty.region"], "eu");
    }

    #[test]
    fn state_changes_map_to_actions_and_the_rest_is_noted() {
        let action = |event_type: &str| parse(&event(event_type, US)).unwrap().alerts[0].action;

        assert_eq!(action("incident.reopened"), ExternalAction::Trigger);
        assert_eq!(action("incident.acknowledged"), ExternalAction::Acknowledge);
        assert_eq!(action("incident.resolved"), ExternalAction::Resolve);

        let annotated = parse(&event("incident.annotated", US)).unwrap();
        assert!(annotated.alerts.is_empty());
        assert!(annotated.message.unwrap().contains("incident.annotated"));
        assert!(parse(&event("pagey.ping", US)).unwrap().alerts.is_empty());
        assert!(parse(br#"{"messages": []}"#).is_err());
    }
}
