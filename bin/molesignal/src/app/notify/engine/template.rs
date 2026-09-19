// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde_json::Value;

use super::NotifyEngine;
use crate::{
    domain::notify::{connector::NotifyMessage, policy::NotifyEvent, template::NotifyTemplate},
    shared::{Error, Result},
};

impl NotifyEngine {
    pub(super) async fn message_for_policy(
        &self,
        policy: &crate::domain::notify::policy::NotifyPolicy,
        event: &NotifyEvent,
        message: &NotifyMessage,
    ) -> Result<NotifyMessage> {
        let Some(template_id) = &policy.template_id else {
            return Ok(message.clone());
        };
        let template = self
            .templates
            .get(&event.organization_id, template_id)
            .await?;
        validate_template_for_notify(&template)?;
        render_message(&template, event, message)
    }
}

pub(super) fn validate_template_for_notify(template: &NotifyTemplate) -> Result<()> {
    validate_notify_template_body(&template.body)
}

pub fn validate_notify_template_body(body: &str) -> Result<()> {
    if body.trim().is_empty() {
        return Err(Error::invalid("notify template body must not be empty"));
    }
    if let Some(path) = placeholder_paths(body).find(|path| path.len() > 128) {
        return Err(Error::invalid(format!(
            "notify template placeholder is too long: {path}"
        )));
    }
    Ok(())
}

fn render_message(
    template: &NotifyTemplate,
    event: &NotifyEvent,
    message: &NotifyMessage,
) -> Result<NotifyMessage> {
    let body = render_template(&template.body, event, message);
    match template.format.as_str() {
        "text" => Ok(NotifyMessage {
            title: message.title.clone(),
            text: body,
            markdown: None,
            html: None,
            metadata: message.metadata.clone(),
        }),
        "markdown" => Ok(NotifyMessage {
            title: message.title.clone(),
            text: body.clone(),
            markdown: Some(body),
            html: None,
            metadata: message.metadata.clone(),
        }),
        "html" => Ok(NotifyMessage {
            title: message.title.clone(),
            text: body.clone(),
            markdown: None,
            html: Some(body),
            metadata: message.metadata.clone(),
        }),
        _ => Err(Error::invalid("notify template format is not supported")),
    }
}

fn placeholder_paths(body: &str) -> impl Iterator<Item = &str> {
    body.split("{{").skip(1).filter_map(|tail| {
        tail.split_once("}}")
            .map(|(path, _)| path.trim())
            .filter(|path| !path.is_empty())
    })
}

fn collect_attribute_values(values: &mut BTreeMap<String, String>, value: &Value, prefix: &str) {
    let Some(fields) = value.as_object() else {
        return;
    };
    for (key, value) in fields {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if value.is_object() {
            collect_attribute_values(values, value, &path);
        } else {
            let rendered = scalar(value);
            values.insert(path.clone(), rendered.clone());
            values.insert(format!("event.attributes.{path}"), rendered);
        }
    }
}

fn alias(values: &mut BTreeMap<String, String>, target: &str, sources: &[&str]) {
    if values.contains_key(target) {
        return;
    }
    if let Some(value) = sources
        .iter()
        .find_map(|source| values.get(*source))
        .cloned()
    {
        values.insert(target.into(), value);
    }
}

fn template_values(event: &NotifyEvent, message: &NotifyMessage) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    values.insert("event.id".into(), event.id.clone());
    values.insert("event.type".into(), event.event_type.clone());
    values.insert("event.occurred_at".into(), event.occurred_at.0.to_string());
    values.insert("message.title".into(), message.title.clone());
    values.insert("message.text".into(), message.text.clone());
    collect_attribute_values(&mut values, &event.attributes, "");

    alias(&mut values, "occurred_at", &["event.occurred_at"]);
    alias(&mut values, "rule.id", &["rule_id"]);
    alias(
        &mut values,
        "rule.name",
        &["rule_name", "summary", "incident.summary"],
    );
    alias(&mut values, "rule.description", &["rule_description"]);
    values.entry("rule.description".into()).or_default();
    alias(&mut values, "incident.id", &["incident_id", "alert_id"]);
    alias(&mut values, "incident.fingerprint", &["fingerprint"]);
    alias(&mut values, "incident.status", &["status"]);
    alias(&mut values, "incident.summary", &["summary"]);
    alias(
        &mut values,
        "evaluated_at",
        &["evaluated_at_micros", "event.occurred_at"],
    );
    alias(
        &mut values,
        "evaluated_at_micros",
        &["evaluated_at", "event.occurred_at"],
    );
    alias(&mut values, "rule_name", &["rule.name", "summary"]);
    alias(&mut values, "incident_id", &["incident.id", "alert_id"]);
    values
        .entry("value".into())
        .or_insert_with(|| "null".into());
    values
        .entry("threshold".into())
        .or_insert_with(|| "null".into());

    alias(&mut values, "schedule.id", &["schedule_id"]);
    alias(&mut values, "schedule.name", &["schedule_name"]);
    alias(&mut values, "schedule.team_id", &["team_id"]);
    alias(&mut values, "schedule.timezone", &["timezone"]);
    alias(&mut values, "oncall.current_user_id", &["current_user_id"]);
    alias(&mut values, "oncall.next_user_id", &["next_user_id"]);
    alias(
        &mut values,
        "oncall.transition_at",
        &["transition_at_micros"],
    );
    alias(&mut values, "override.id", &["override_id"]);
    alias(&mut values, "override.user_id", &["override_user_id"]);
    alias(
        &mut values,
        "override.original_user_id",
        &["original_user_id"],
    );
    alias(&mut values, "override.start_at", &["start_at_micros"]);
    alias(&mut values, "override.end_at", &["end_at_micros"]);
    alias(&mut values, "override.reason", &["reason"]);
    values
}

fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn render_template(body: &str, event: &NotifyEvent, message: &NotifyMessage) -> String {
    let values = template_values(event, message);
    let mut output = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let raw_path = &after_open[..end];
        let path = raw_path.trim();
        if let Some(value) = values.get(path) {
            output.push_str(value);
        } else {
            output.push_str("{{");
            output.push_str(raw_path);
            output.push_str("}}");
        }
        rest = &after_open[end + 2..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        domain::notify::{preference::NotifyCategory, template::notify_template_field_catalog},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "Title".into(),
            text: "Text".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
        }
    }

    fn catalog_path(token: &str, category: NotifyCategory) -> String {
        let path = token
            .strip_prefix("{{")
            .and_then(|value| value.strip_suffix("}}"))
            .expect("catalog token");
        match path {
            "labels.<key>" => "labels.service".into(),
            "annotations.<key>" => "annotations.runbook_url".into(),
            "event.attributes.<key>"
                if matches!(category, NotifyCategory::Alert | NotifyCategory::Escalation) =>
            {
                "event.attributes.summary".into()
            }
            "event.attributes.<key>" => "event.attributes.schedule_name".into(),
            _ => path.into(),
        }
    }

    #[test]
    fn catalog_fields_are_renderable_for_alert_escalation_and_oncall() {
        let fixtures = [
            (
                NotifyCategory::Alert,
                serde_json::json!({
                    "rule_id": "rule",
                    "rule_name": "CPU high",
                    "rule_description": "CPU threshold exceeded",
                    "alert_id": "incident",
                    "fingerprint": "cpu-high",
                    "status": "open",
                    "summary": "CPU high",
                    "severity": "critical",
                    "value": 90,
                    "threshold": 80,
                    "evaluated_at_micros": 2,
                    "labels": {"service": "api"},
                    "annotations": {"runbook_url": "https://example.test"}
                }),
            ),
            (
                NotifyCategory::Escalation,
                serde_json::json!({
                    "rule_id": "rule",
                    "rule_name": "CPU high",
                    "rule_description": "CPU threshold exceeded",
                    "alert_id": "incident",
                    "fingerprint": "cpu-high",
                    "status": "open",
                    "summary": "CPU high",
                    "severity": "critical",
                    "value": 90,
                    "threshold": 80,
                    "evaluated_at_micros": 2,
                    "labels": {"service": "api"},
                    "annotations": {"runbook_url": "https://example.test"}
                }),
            ),
            (
                NotifyCategory::Oncall,
                serde_json::json!({
                    "schedule_id": "schedule",
                    "schedule_name": "Primary",
                    "team_id": "team",
                    "timezone": "UTC",
                    "current_user_id": "alice",
                    "next_user_id": "bob",
                    "transition_at_micros": 2,
                    "override_id": "override",
                    "override_user_id": "bob",
                    "original_user_id": "alice",
                    "start_at_micros": 1,
                    "end_at_micros": 3,
                    "reason": "handoff"
                }),
            ),
        ];

        for (category, attributes) in fixtures {
            let event = NotifyEvent {
                id: "event:1".into(),
                event_type: category.as_str().into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes,
            };
            let values = template_values(&event, &message());
            for field in notify_template_field_catalog()
                .into_iter()
                .filter(|field| field.categories.contains(&category))
            {
                let path = catalog_path(&field.token, category);
                assert!(
                    values.contains_key(&path),
                    "catalog field {} is not renderable for {}",
                    field.token,
                    category.as_str()
                );
            }
        }
    }

    #[test]
    fn renders_notify_event_attributes() {
        let rendered = render_message(
            &NotifyTemplate {
                id: Id::new(),
                organization_id: Id::new(),
                category: crate::domain::notify::preference::NotifyCategory::Alert,
                body: "[{{severity}}] {{summary}} · {{fingerprint}} · {{labels.service}} @ {{event.occurred_at}}".into(),
                format: "markdown".into(),
            },
            &NotifyEvent {
                id: "alert.triggered:1".into(),
                event_type: "alert.triggered".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "severity": "critical",
                    "summary": "API unavailable",
                    "fingerprint": "api-unavailable",
                    "labels": {"service": "api"}
                }),
            },
            &NotifyMessage {
                title: "Alert".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
            },
        )
        .unwrap();
        assert_eq!(
            rendered.markdown.as_deref(),
            Some("[critical] API unavailable · api-unavailable · api @ 1")
        );
    }

    #[test]
    fn renders_alert_placeholders() {
        let template = NotifyTemplate {
            id: Id::new(),
            organization_id: Id::new(),
            category: crate::domain::notify::preference::NotifyCategory::Alert,
            body: "{{rule.name}} {{incident.summary}} {{evaluated_at}}".into(),
            format: "text".into(),
        };
        let rendered = render_message(
            &template,
            &NotifyEvent {
                id: "alert.triggered:1".into(),
                event_type: "alert.triggered".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "rule_id": "rule",
                    "summary": "CPU high",
                    "status": "open",
                    "fingerprint": "cpu-high"
                }),
            },
            &NotifyMessage {
                title: "Alert".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
            },
        )
        .unwrap();

        assert_eq!(rendered.text, "CPU high CPU high 1");
    }

    #[test]
    fn renders_oncall_placeholders_and_whitespace() {
        let rendered = render_message(
            &NotifyTemplate {
                id: Id::new(),
                organization_id: Id::new(),
                category: crate::domain::notify::preference::NotifyCategory::Oncall,
                body: "{{ schedule.name }} {{oncall.current_user_id}} -> {{oncall.next_user_id}} @ {{oncall.transition_at}}".into(),
                format: "text".into(),
            },
            &NotifyEvent {
                id: "oncall.shift.starting:1".into(),
                event_type: "oncall.shift.starting".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "schedule_name": "Primary",
                    "current_user_id": "alice",
                    "next_user_id": "bob",
                    "transition_at_micros": 2
                }),
            },
            &NotifyMessage {
                title: "On-call shift starting".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
            },
        )
        .unwrap();

        assert_eq!(rendered.text, "Primary alice -> bob @ 2");
    }
}
