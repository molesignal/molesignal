// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{
    app::notify::NotifyDispatch,
    domain::{
        alerting::{escalation::EscalationTarget, incident::Incident},
        notify::{connector::NotifyMessage, policy::NotifyEvent},
    },
};

pub const ALERT_TRIGGERED_EVENT: &str = "alert.triggered";
pub const ALERT_ACKNOWLEDGED_EVENT: &str = "alert.acknowledged";
pub const ALERT_RESOLVED_EVENT: &str = "alert.resolved";
pub const ALERT_ESCALATED_EVENT: &str = "alert.escalated";

pub fn triggered_event_id(incident_id: &crate::shared::ids::Id) -> String {
    format!("{ALERT_TRIGGERED_EVENT}:{incident_id}")
}

pub fn alert_dispatch(incident: &Incident, event_type: &str) -> NotifyDispatch {
    let mut attributes = Map::new();
    attributes.insert("alert_id".into(), Value::String(incident.id.to_string()));
    attributes.insert(
        "rule_id".into(),
        Value::String(incident.rule_id.to_string()),
    );
    attributes.insert(
        "severity".into(),
        Value::String(incident.severity.as_str().into()),
    );
    attributes.insert(
        "status".into(),
        Value::String(incident.status.as_str().into()),
    );
    attributes.insert("summary".into(), Value::String(incident.summary.clone()));
    attributes.insert("rule_name".into(), Value::String(incident.summary.clone()));
    attributes.insert("incident_id".into(), Value::String(incident.id.to_string()));
    attributes.insert(
        "fingerprint".into(),
        Value::String(incident.fingerprint.clone()),
    );
    attributes.insert(
        "evaluated_at_micros".into(),
        Value::Number(event_occurred_at(incident, event_type).0.into()),
    );
    attributes.insert(
        "rule".into(),
        serde_json::json!({
            "id": incident.rule_id.to_string(),
            "name": incident.summary,
            "description": ""
        }),
    );
    attributes.insert(
        "incident".into(),
        serde_json::json!({
            "id": incident.id.to_string(),
            "fingerprint": incident.fingerprint,
            "status": incident.status.as_str(),
            "summary": incident.summary
        }),
    );
    attributes.insert(
        "assignee_user_ids".into(),
        Value::Array(
            incident
                .assignees
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    attributes.insert(
        "affected_services".into(),
        Value::Array(
            incident
                .affected_services
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    attributes.insert(
        "labels".into(),
        serde_json::to_value(&incident.labels).unwrap_or_else(|_| Value::Object(Map::new())),
    );
    attributes.insert(
        "annotations".into(),
        serde_json::to_value(&incident.annotations).unwrap_or_else(|_| Value::Object(Map::new())),
    );
    copy_context_attribute(&mut attributes, incident, "service");
    copy_context_attribute(&mut attributes, incident, "environment");
    copy_context_attribute(&mut attributes, incident, "schedule_id");
    copy_context_attribute(&mut attributes, incident, "team_id");
    copy_context_attribute(&mut attributes, incident, "owner_user_id");
    if !attributes.contains_key("service")
        && let Some(service) = incident.affected_services.first()
    {
        attributes.insert("service".into(), Value::String(service.clone()));
    }
    if let Some(value) = incident
        .triggering_query
        .as_ref()
        .and_then(|query| query.sample_values.first())
        .map(|sample| sample.value)
        .filter(|value| value.is_finite())
        && let Some(number) = serde_json::Number::from_f64(value)
    {
        attributes.insert("value".into(), Value::Number(number));
    }
    if let Some(user_id) = &incident.acknowledged_by {
        attributes.insert("acknowledged_by".into(), Value::String(user_id.to_string()));
    }
    if let Some(user_id) = &incident.resolved_by {
        attributes.insert("resolved_by".into(), Value::String(user_id.to_string()));
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("alert_id".into(), incident.id.to_string());
    metadata.insert("event_type".into(), event_type.to_string());
    NotifyDispatch {
        event: NotifyEvent {
            id: format!("{event_type}:{}", incident.id),
            event_type: event_type.into(),
            organization_id: incident.org_id.clone(),
            occurred_at: event_occurred_at(incident, event_type),
            attributes: Value::Object(attributes),
        },
        message: NotifyMessage {
            title: format!(
                "{} · {}",
                incident.severity.as_str().to_ascii_uppercase(),
                incident.summary
            ),
            text: format!(
                "Alert {} is {}.",
                incident.summary,
                incident.status.as_str()
            ),
            markdown: Some(format!(
                "**{}**\n\nSeverity: `{}`\nStatus: `{}`",
                incident.summary,
                incident.severity.as_str(),
                incident.status.as_str()
            )),
            html: None,
            metadata,
        },
    }
}

pub fn alert_escalation_dispatch(
    incident: &Incident,
    step_index: usize,
    loop_index: u32,
    target_index: usize,
    target: &EscalationTarget,
) -> NotifyDispatch {
    let mut dispatch = alert_dispatch(incident, ALERT_ESCALATED_EVENT);
    dispatch.event.id = format!(
        "{ALERT_ESCALATED_EVENT}:{}:{loop_index}:{step_index}:{target_index}",
        incident.id
    );
    dispatch.event.occurred_at = incident.current_step_started_at;
    let attributes = dispatch
        .event
        .attributes
        .as_object_mut()
        .expect("alert dispatch attributes are an object");
    attributes.insert("step_index".into(), Value::from(step_index));
    attributes.insert("loop_index".into(), Value::from(loop_index));
    match target {
        EscalationTarget::User { user_id } => {
            attributes.insert("target_kind".into(), Value::String("user".into()));
            attributes.insert(
                "user_ids".into(),
                Value::Array(vec![Value::String(user_id.to_string())]),
            );
        }
        EscalationTarget::Schedule { schedule_id } => {
            attributes.insert("target_kind".into(), Value::String("schedule".into()));
            attributes.insert("schedule_id".into(), Value::String(schedule_id.to_string()));
        }
        EscalationTarget::Team { team_id } => {
            attributes.insert("target_kind".into(), Value::String("team".into()));
            attributes.insert("team_id".into(), Value::String(team_id.to_string()));
        }
    }
    dispatch.message.title = format!("ESCALATION · {}", incident.summary);
    dispatch
        .message
        .metadata
        .insert("step_index".into(), step_index.to_string());
    dispatch
}

fn event_occurred_at(
    incident: &Incident,
    event_type: &str,
) -> crate::shared::time::TimestampMicros {
    match event_type {
        ALERT_ACKNOWLEDGED_EVENT => incident.acknowledged_at.unwrap_or(incident.created_at),
        ALERT_RESOLVED_EVENT => incident.resolved_at.unwrap_or(incident.created_at),
        _ => incident.created_at,
    }
}

fn copy_context_attribute(attributes: &mut Map<String, Value>, incident: &Incident, key: &str) {
    if let Some(value) = incident
        .labels
        .get(key)
        .or_else(|| incident.annotations.get(key))
        .filter(|value| !value.trim().is_empty())
    {
        attributes.insert(key.into(), Value::String(value.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::alerting::incident::{IncidentStatus, Severity},
        shared::{ids::Id, time::TimestampMicros},
    };

    #[test]
    fn alert_event_id_is_stable() {
        let incident = Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("rule"),
            escalation_policy_id: Id::from_string("policy"),
            status: IncidentStatus::Open,
            severity: Severity::Critical,
            summary: "API unavailable".into(),
            fingerprint: "fingerprint".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(1),
            assignees: Vec::new(),
            labels: Default::default(),
            annotations: Default::default(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: vec!["api".into()],
            triggering_query: None,
            created_at: TimestampMicros(1),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
        };
        let dispatch = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(dispatch.event.id, "alert.triggered:incident");
        assert_eq!(dispatch.event.attributes["service"], "api");
        assert_eq!(dispatch.event.attributes["rule"]["id"], "rule");
        assert_eq!(dispatch.event.attributes["rule"]["name"], "API unavailable");
        assert_eq!(dispatch.event.attributes["incident"]["id"], "incident");
        assert_eq!(dispatch.event.attributes["incident"]["status"], "open");
        assert_eq!(dispatch.event.attributes["evaluated_at_micros"], 1);
    }

    #[test]
    fn escalation_event_encodes_recipient_target_without_connector_ids() {
        let incident = Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("rule"),
            escalation_policy_id: Id::from_string("policy"),
            status: IncidentStatus::Open,
            severity: Severity::Critical,
            summary: "API unavailable".into(),
            fingerprint: "fingerprint".into(),
            current_step: 1,
            current_loop: 2,
            current_step_started_at: TimestampMicros(3),
            assignees: Vec::new(),
            labels: Default::default(),
            annotations: Default::default(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: Vec::new(),
            triggering_query: None,
            created_at: TimestampMicros(1),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
        };
        let dispatch = alert_escalation_dispatch(
            &incident,
            1,
            2,
            0,
            &EscalationTarget::User {
                user_id: Id::from_string("user"),
            },
        );
        assert_eq!(dispatch.event.id, "alert.escalated:incident:2:1:0");
        assert_eq!(dispatch.event.attributes["target_kind"], "user");
        assert_eq!(dispatch.event.attributes["user_ids"][0], "user");
    }
}
