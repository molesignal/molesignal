// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Webhook 的默认请求体里写给机器读的那部分：版本、事件，告警事件还有告警、规则与升级。时间写成
//! RFC 3339（UTC），人员与团队写 ID。

use chrono::{TimeZone, Utc};
use serde_json::{Map, Value, json};

use crate::domain::notify::policy::NotifyEvent;

/// 请求体结构的版本：字段只增不改，改了就加一。2 起键名是 camelCase；`data` 仍是事件属性的原样。
pub(super) const WEBHOOK_BODY_VERSION: u32 = 2;

/// 一个通知事件写给 Webhook 的样子。不是告警的事件（换班、缺人）在 `data` 里原样带上它的属性。
pub(super) fn webhook_event(event: &NotifyEvent) -> Value {
    let mut body = Map::new();
    body.insert("version".into(), json!(WEBHOOK_BODY_VERSION));

    body.insert(
        "event".into(),
        json!({
            "id": event.id,
            "type": event.event_type,
            "occurredAt": rfc3339(event.occurred_at.0),
            "organizationId": event.organization_id,
        }),
    );

    let attributes = &event.attributes;

    if !event.event_type.starts_with("alert.") {
        body.insert("data".into(), attributes.clone());
        return Value::Object(body);
    }

    let text = |key: &str| {
        attributes
            .get(key)
            .filter(|value| !value.is_null())
            .cloned()
    };
    let time = |key: &str| {
        attributes
            .get(key)
            .and_then(Value::as_i64)
            .map(|micros| Value::String(rfc3339(micros)))
    };

    body.insert(
        "alert".into(),
        json!({
            "id": text("alert_id"),
            "status": text("status"),
            "severity": text("severity"),
            "title": text("summary"),
            "url": text("incident_url"),
            "startedAt": time("started_at"),
            "acknowledgedAt": time("acknowledged_at"),
            "resolvedAt": time("resolved_at"),
            "acknowledgedBy": text("acknowledged_by"),
            "resolvedBy": text("resolved_by"),
            "assignees": text("assignee_user_ids").unwrap_or_else(|| json!([])),
            "teamId": text("team_id"),
            "service": text("service"),
            "environment": text("environment"),
            "affectedServices": text("affected_services").unwrap_or_else(|| json!([])),
            "labels": text("labels").unwrap_or_else(|| json!({})),
            "annotations": text("annotations").unwrap_or_else(|| json!({})),
            "value": text("value"),
            "threshold": text("threshold"),
        }),
    );

    if let Some(rule) = attributes.get("rule").filter(|rule| rule.is_object()) {
        body.insert("rule".into(), rule.clone());
    }

    if event.event_type == "alert.escalated" {
        let target_id = text("user_ids")
            .and_then(|ids| ids.get(0).cloned())
            .or_else(|| text("schedule_id"))
            .or_else(|| text("team_id"));

        body.insert(
            "escalation".into(),
            json!({
                "step": text("step_index"),
                "loop": text("loop_index"),
                "reason": text("escalation_reason"),
                "target": { "kind": text("target_kind"), "id": target_id },
            }),
        );
    }

    Value::Object(body)
}

fn rfc3339(micros: i64) -> String {
    Utc.timestamp_micros(micros)
        .single()
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn event(event_type: &str, attributes: Value) -> NotifyEvent {
        NotifyEvent {
            id: format!("{event_type}:inc-1"),
            event_type: event_type.into(),
            organization_id: Id::from_string("org-a"),
            occurred_at: TimestampMicros(1_761_609_600_000_000),
            attributes,
        }
    }

    #[test]
    fn an_alert_is_written_with_its_rule_and_times() {
        let body = webhook_event(&event(
            "alert.triggered",
            json!({
                "alert_id": "inc-1",
                "status": "open",
                "severity": "critical",
                "summary": "Checkout errors",
                "started_at": 1_761_609_600_000_000_i64,
                "assignee_user_ids": ["u1"],
                "labels": {"service": "checkout"},
                "rule": {"id": "rule-1", "name": "Checkout errors", "description": ""},
            }),
        ));

        assert_eq!(body["version"], 2);
        assert_eq!(body["event"]["type"], "alert.triggered");
        assert_eq!(body["event"]["occurredAt"], "2025-10-28T00:00:00Z");
        assert_eq!(body["event"]["organizationId"], "org-a");
        assert_eq!(body["alert"]["severity"], "critical");
        assert_eq!(body["alert"]["startedAt"], "2025-10-28T00:00:00Z");
        assert_eq!(body["alert"]["resolvedAt"], Value::Null);
        assert_eq!(body["alert"]["assignees"], json!(["u1"]));
        assert_eq!(body["rule"]["id"], "rule-1");
        assert!(body.get("escalation").is_none());
    }

    #[test]
    fn an_escalation_says_where_and_why() {
        let body = webhook_event(&event(
            "alert.escalated",
            json!({
                "alert_id": "inc-1",
                "step_index": 1,
                "loop_index": 0,
                "target_kind": "user",
                "user_ids": ["u2"],
                "escalation_reason": "reassigned",
            }),
        ));

        assert_eq!(body["escalation"]["step"], 1);
        assert_eq!(body["escalation"]["reason"], "reassigned");
        assert_eq!(
            body["escalation"]["target"],
            json!({"kind": "user", "id": "u2"})
        );
    }

    #[test]
    fn other_events_carry_their_attributes() {
        let body = webhook_event(&event(
            "oncall.shift.starting",
            json!({"schedule_id": "sch-1"}),
        ));

        assert_eq!(body["data"]["schedule_id"], "sch-1");
        assert!(body.get("alert").is_none());
    }
}
