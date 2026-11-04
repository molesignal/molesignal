// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{
    app::notify::NotifyDispatch,
    domain::{
        alerting::{
            escalation::EscalationTarget,
            handling::EscalationReason,
            incident::{DESCRIPTION_ANNOTATION, Incident, SYNTHETIC_STATE_LABEL},
        },
        notify::{connector::NotifyMessage, policy::NotifyEvent},
        pipeline::alert::AlertFields,
    },
    shared::ids::Id,
};

pub const ALERT_TRIGGERED_EVENT: &str = "alert.triggered";
pub const ALERT_ACKNOWLEDGED_EVENT: &str = "alert.acknowledged";
pub const ALERT_RESOLVED_EVENT: &str = "alert.resolved";
pub const ALERT_ESCALATED_EVENT: &str = "alert.escalated";

pub fn triggered_event_id(incident_id: &crate::shared::ids::Id) -> String {
    format!("{ALERT_TRIGGERED_EVENT}:{incident_id}")
}

/// 一个告警事件各次升级的通知事件 ID 的共同前缀（见 [`alert_escalation_dispatch`]）。
pub fn escalated_event_prefix(incident_id: &crate::shared::ids::Id) -> String {
    format!("{ALERT_ESCALATED_EVENT}:{incident_id}:")
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

    // 紧急度决定接收人用哪一组个人通知步骤、免打扰挡不挡；优先级给通知策略匹配和模板用。
    attributes.insert(
        "urgency".into(),
        Value::String(incident.urgency().as_str().into()),
    );

    if let Some(priority) = incident.handling.priority {
        attributes.insert("priority".into(), Value::String(priority.as_str()));
    }

    attributes.insert("summary".into(), Value::String(incident.summary.clone()));
    attributes.insert("rule_name".into(), Value::String(incident.summary.clone()));
    attributes.insert("incident_id".into(), Value::String(incident.id.to_string()));

    attributes.insert(
        "fingerprint".into(),
        Value::String(incident.fingerprint.clone()),
    );

    attributes.insert(
        "evaluated_at".into(),
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
    // 路由定下的团队优先；没有时沿用标签或注解里写的 `team_id`。
    match &incident.team_id {
        Some(team_id) => {
            attributes.insert("team_id".into(), Value::String(team_id.to_string()));
        }
        None => copy_context_attribute(&mut attributes, incident, "team_id"),
    }
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

    // 告警开始、被确认、被解决的时刻（微秒）：模板里写成可读的时间与持续时长。
    attributes.insert(
        "started_at".into(),
        Value::Number(incident.created_at.0.into()),
    );

    if let Some(at) = incident.acknowledged_at {
        attributes.insert("acknowledged_at".into(), Value::Number(at.0.into()));
    }

    if let Some(at) = incident.resolved_at {
        attributes.insert("resolved_at".into(), Value::Number(at.0.into()));
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("alert_id".into(), incident.id.to_string());
    metadata.insert("event_type".into(), event_type.to_string());
    metadata.insert("severity".into(), incident.severity.as_str().to_string());
    metadata.insert("urgency".into(), incident.urgency().as_str().to_string());
    metadata.insert("status".into(), incident.status.as_str().to_string());

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
                title_prefix(incident, event_type),
                incident.summary
            ),
            text: message_text(incident, event_type),
            markdown: Some(format!(
                "**{}**\n\nSeverity: `{}`\nStatus: `{}`",
                incident.summary,
                incident.severity.as_str(),
                incident.status.as_str()
            )),
            html: None,
            metadata,
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
    }
}

/// 升级策略某一步的一个对象。通知 ID 由轮次、遍数、步骤和对象组成：同一轮里同一步只通知一次，
/// 手动升级、转派给团队或升级策略、调高严重度换一轮后可以再通知（第 0 轮的 ID 不写轮次）。
pub fn alert_escalation_dispatch(
    incident: &Incident,
    step_index: usize,
    loop_index: u32,
    target_index: usize,
    target: &EscalationTarget,
) -> NotifyDispatch {
    let reason = incident.handling.dispatch_reason;
    let mut dispatch = escalated(incident, step_index, loop_index, reason);
    let round = incident.handling.dispatch_round;

    dispatch.event.id = if round == 0 {
        format!(
            "{ALERT_ESCALATED_EVENT}:{}:{loop_index}:{step_index}:{target_index}",
            incident.id
        )
    } else {
        format!(
            "{ALERT_ESCALATED_EVENT}:{}:r{round}:{loop_index}:{step_index}:{target_index}",
            incident.id
        )
    };

    let attributes = dispatch
        .event
        .attributes
        .as_object_mut()
        .expect("alert dispatch attributes are an object");

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

    dispatch
}

/// 直接通知一个人：转派给他、增派他，或者事件退回未确认时通知负责人。`key` 取时间线上那条记录的
/// ID，每次通知的 ID 都不同。
pub fn alert_page_dispatch(
    incident: &Incident,
    user_id: &Id,
    reason: EscalationReason,
    key: &str,
) -> NotifyDispatch {
    let mut dispatch = escalated(
        incident,
        incident.current_step as usize,
        incident.current_loop,
        reason,
    );

    dispatch.event.id = format!("{ALERT_ESCALATED_EVENT}:{}:{key}:{user_id}", incident.id);

    let attributes = dispatch
        .event
        .attributes
        .as_object_mut()
        .expect("alert dispatch attributes are an object");

    attributes.insert("target_kind".into(), Value::String("user".into()));

    attributes.insert(
        "user_ids".into(),
        Value::Array(vec![Value::String(user_id.to_string())]),
    );

    dispatch
}

/// 通知前处理改过事件之后，这一份通知跟着改：事件属性里的这些字段（模板和 Webhook 从这里读），以及
/// 没有模板时发出去的标题和正文。接收人读的属性（团队、值班表、负责人）不动。
pub fn retold_by_pipeline(
    event: &mut NotifyEvent,
    message: &mut NotifyMessage,
    before: &AlertFields,
    after: &AlertFields,
) {
    if let Some(attributes) = event.attributes.as_object_mut() {
        write_fields(attributes, after);
    }

    let (old, new) = (before.summary.as_str(), after.summary.as_str());

    // 标题是「开头 · 事件标题」，开头是严重度时跟着严重度。
    if let Some(prefix) = message
        .title
        .strip_suffix(old)
        .and_then(|prefix| prefix.strip_suffix(" · "))
    {
        let prefix = if prefix == before.severity.as_str().to_ascii_uppercase() {
            after.severity.as_str().to_ascii_uppercase()
        } else {
            prefix.to_owned()
        };

        message.title = format!("{prefix} · {new}");
    }

    message.text = message
        .text
        .replacen(&format!(" {old} "), &format!(" {new} "), 1);

    if let Some(markdown) = &mut message.markdown {
        *markdown = markdown
            .replacen(&format!("**{old}**"), &format!("**{new}**"), 1)
            .replacen(
                &format!("Severity: `{}`", before.severity.as_str()),
                &format!("Severity: `{}`", after.severity.as_str()),
                1,
            );
    }

    message
        .metadata
        .insert("severity".into(), after.severity.as_str().to_string());

    if let Some(urgency) = &after.urgency {
        message
            .metadata
            .insert("urgency".into(), urgency.as_str().to_string());
    }
}

/// 事件属性里说这些字段的地方：标题写在几处，服务与环境从标签和注解读出。
fn write_fields(attributes: &mut Map<String, Value>, fields: &AlertFields) {
    let summary = Value::String(fields.summary.clone());

    attributes.insert("summary".into(), summary.clone());
    attributes.insert("rule_name".into(), summary.clone());

    for group in ["rule", "incident"] {
        let key = if group == "rule" { "name" } else { "summary" };

        if let Some(Value::Object(snapshot)) = attributes.get_mut(group) {
            snapshot.insert(key.into(), summary.clone());
        }
    }

    attributes.insert(
        "severity".into(),
        Value::String(fields.severity.as_str().into()),
    );

    match &fields.urgency {
        Some(urgency) => {
            attributes.insert("urgency".into(), Value::String(urgency.as_str().into()));
        }
        None => {
            attributes.remove("urgency");
        }
    }

    match fields.priority {
        Some(priority) => {
            attributes.insert("priority".into(), Value::String(priority.as_str()));
        }
        None => {
            attributes.remove("priority");
        }
    }

    attributes.insert(
        "labels".into(),
        serde_json::to_value(&fields.labels).unwrap_or_else(|_| Value::Object(Map::new())),
    );

    attributes.insert(
        "annotations".into(),
        serde_json::to_value(&fields.annotations).unwrap_or_else(|_| Value::Object(Map::new())),
    );

    attributes.insert(
        "affected_services".into(),
        Value::Array(
            fields
                .affected_services
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );

    for key in ["service", "environment"] {
        attributes.remove(key);

        if let Some(value) = fields
            .labels
            .get(key)
            .or_else(|| fields.annotations.get(key))
            .filter(|value| !value.trim().is_empty())
        {
            attributes.insert(key.into(), Value::String(value.clone()));
        }
    }

    if !attributes.contains_key("service")
        && let Some(service) = fields.affected_services.first()
    {
        attributes.insert("service".into(), Value::String(service.clone()));
    }
}

/// 升级通知的共同部分：在哪一步、第几遍、为什么，标题开头跟着原因。
fn escalated(
    incident: &Incident,
    step_index: usize,
    loop_index: u32,
    reason: EscalationReason,
) -> NotifyDispatch {
    let mut dispatch = alert_dispatch(incident, ALERT_ESCALATED_EVENT);
    dispatch.event.occurred_at = incident.current_step_started_at;

    let attributes = dispatch
        .event
        .attributes
        .as_object_mut()
        .expect("alert dispatch attributes are an object");

    attributes.insert("step_index".into(), Value::from(step_index));
    attributes.insert("loop_index".into(), Value::from(loop_index));

    attributes.insert(
        "escalation_reason".into(),
        Value::String(reason.as_str().into()),
    );

    dispatch.message.title = format!("{} · {}", reason_prefix(reason), incident.summary);

    dispatch
        .message
        .metadata
        .insert("step_index".into(), step_index.to_string());

    dispatch
}

/// 升级通知标题的开头。
fn reason_prefix(reason: EscalationReason) -> &'static str {
    match reason {
        EscalationReason::Step | EscalationReason::Manual | EscalationReason::SeverityRaised => {
            "ESCALATION"
        }
        EscalationReason::Reassigned | EscalationReason::ResponderAdded => "ASSIGNED",
        EscalationReason::SnoozeEnded => "SNOOZE ENDED",
        EscalationReason::AckTimeout => "UNRESOLVED",
        EscalationReason::UrgencyRaised => "URGENCY RAISED",
    }
}

/// 通知正文：无数据事件说没有数据（拨测的无数据事件用它注解里按工作区语言写的那句），拨测事件说
/// 监控项的状态，其余说事件的状态；解决、确认时都说事件的状态。
fn message_text(incident: &Incident, event_type: &str) -> String {
    let opened = matches!(event_type, ALERT_TRIGGERED_EVENT | ALERT_ESCALATED_EVENT);

    if incident.is_no_data() && opened {
        if incident.synthetic_monitor_id().is_some()
            && let Some(description) = incident.annotations.get(DESCRIPTION_ANNOTATION)
        {
            return description.clone();
        }

        return format!("Alert {} has no data.", incident.summary);
    }

    if incident.synthetic_monitor_id().is_some() {
        let state = incident
            .labels
            .get(SYNTHETIC_STATE_LABEL)
            .filter(|_| opened)
            .map_or(incident.status.as_str(), String::as_str);

        return format!("Synthetic check {} is {state}.", incident.summary);
    }

    format!(
        "Alert {} is {}.",
        incident.summary,
        incident.status.as_str()
    )
}

/// 通知标题的开头：升级是 `ESCALATION`，无数据事件是 `NO DATA`，其余是严重度。
fn title_prefix(incident: &Incident, event_type: &str) -> String {
    if event_type == ALERT_ESCALATED_EVENT {
        "ESCALATION".into()
    } else if incident.is_no_data() {
        "NO DATA".into()
    } else {
        incident.severity.as_str().to_ascii_uppercase()
    }
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
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        };
        let dispatch = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(dispatch.event.id, "alert.triggered:incident");
        assert_eq!(dispatch.event.attributes["service"], "api");
        assert_eq!(dispatch.event.attributes["rule"]["id"], "rule");
        assert_eq!(dispatch.event.attributes["rule"]["name"], "API unavailable");
        assert_eq!(dispatch.event.attributes["incident"]["id"], "incident");
        assert_eq!(dispatch.event.attributes["incident"]["status"], "open");
        assert_eq!(dispatch.event.attributes["evaluated_at"], 1);
        assert_eq!(dispatch.message.title, "CRITICAL · API unavailable");
        assert!(dispatch.event.attributes.get("team_id").is_none());

        // The routed team comes before a `team_id` written in the labels.
        let routed = Incident {
            team_id: Some(Id::from_string("payments")),
            labels: [("team_id".to_string(), "from-label".to_string())].into(),
            ..incident
        };
        let dispatch = alert_dispatch(&routed, ALERT_TRIGGERED_EVENT);
        assert_eq!(dispatch.event.attributes["team_id"], "payments");
    }

    #[test]
    fn a_pipeline_before_a_notification_retells_it_and_leaves_who_receives_it() {
        use crate::domain::alerting::priority::Priority;

        let incident = Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("rule"),
            escalation_policy_id: Id::from_string("policy"),
            status: IncidentStatus::Open,
            severity: Severity::Error,
            summary: "API unavailable".into(),
            fingerprint: "fingerprint".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(1),
            assignees: Vec::new(),
            labels: [
                ("service".to_string(), "api".to_string()),
                ("team_id".to_string(), "from-label".to_string()),
            ]
            .into(),
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
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        };

        let mut dispatch = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        let before = AlertFields::of(&incident);

        let after = before
            .changed(&serde_json::json!({
                "summary": "Payments down",
                "severity": "critical",
                "priority": "P1",
                "labels": { "service": "payments", "env": "prod" },
                "annotations": { "environment": "production" },
                "affected_services": [],
            }))
            .unwrap();

        retold_by_pipeline(&mut dispatch.event, &mut dispatch.message, &before, &after);

        let attributes = &dispatch.event.attributes;

        assert_eq!(attributes["summary"], "Payments down");
        assert_eq!(attributes["rule"]["name"], "Payments down");
        assert_eq!(attributes["incident"]["summary"], "Payments down");
        assert_eq!(attributes["severity"], "critical");
        assert_eq!(attributes["priority"], "P1");
        assert_eq!(attributes["labels"]["env"], "prod");
        assert_eq!(attributes["service"], "payments");
        assert_eq!(attributes["environment"], "production");
        assert_eq!(attributes["affected_services"], serde_json::json!([]));
        // Who it goes to was worked out from the event as it came.
        assert_eq!(attributes["team_id"], "from-label");

        assert_eq!(dispatch.message.title, "CRITICAL · Payments down");
        assert_eq!(dispatch.message.text, "Alert Payments down is open.");
        assert_eq!(
            dispatch.message.markdown.as_deref(),
            Some("**Payments down**\n\nSeverity: `critical`\nStatus: `open`")
        );
        assert_eq!(dispatch.message.metadata["severity"], "critical");

        // A title that does not start with the severity keeps its start.
        let mut escalated = alert_dispatch(&incident, ALERT_ESCALATED_EVENT);
        retold_by_pipeline(
            &mut escalated.event,
            &mut escalated.message,
            &before,
            &after,
        );
        assert_eq!(escalated.message.title, "ESCALATION · Payments down");

        let cleared = after
            .changed(&serde_json::json!({ "priority": null }))
            .unwrap();
        let mut dispatch = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        retold_by_pipeline(
            &mut dispatch.event,
            &mut dispatch.message,
            &before,
            &cleared,
        );
        assert!(dispatch.event.attributes.get("priority").is_none());
        assert_eq!(cleared.priority, None::<Priority>);
    }

    #[test]
    fn a_no_data_alert_says_so_in_its_title_and_text() {
        let mut incident = Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("rule"),
            escalation_policy_id: Id::from_string("policy"),
            status: IncidentStatus::Open,
            severity: Severity::Warning,
            summary: "Disk usage · web-1".into(),
            fingerprint: "fingerprint".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(1),
            assignees: Vec::new(),
            labels: [("alert.condition".to_string(), "no_data".to_string())].into(),
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
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        };

        let triggered = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(triggered.message.title, "NO DATA · Disk usage · web-1");
        assert_eq!(
            triggered.message.text,
            "Alert Disk usage · web-1 has no data."
        );

        incident.status = IncidentStatus::Resolved;
        let resolved = alert_dispatch(&incident, ALERT_RESOLVED_EVENT);
        assert_eq!(
            resolved.message.text,
            "Alert Disk usage · web-1 is resolved."
        );
    }

    #[test]
    fn a_synthetic_alert_names_the_state_of_its_check() {
        let mut incident = Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("monitor-1"),
            escalation_policy_id: Id::from_string(""),
            status: IncidentStatus::Open,
            severity: Severity::Error,
            summary: "Checkout API".into(),
            fingerprint: "fingerprint".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(1),
            assignees: Vec::new(),
            labels: [
                ("alert.source".to_string(), "synthetic".to_string()),
                ("synthetic.state".to_string(), "failing".to_string()),
            ]
            .into(),
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
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        };

        let triggered = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(triggered.message.title, "ERROR · Checkout API");
        assert_eq!(
            triggered.message.text,
            "Synthetic check Checkout API is failing."
        );

        incident.status = IncidentStatus::Resolved;
        let resolved = alert_dispatch(&incident, ALERT_RESOLVED_EVENT);
        assert_eq!(
            resolved.message.text,
            "Synthetic check Checkout API is resolved."
        );

        // A check that gets no results says where and why, in the words its incident was
        // opened with; without them, that it has no data.
        incident.status = IncidentStatus::Open;
        incident.severity = Severity::Warning;
        incident
            .labels
            .insert("synthetic.state".into(), "no_data".into());
        incident
            .labels
            .insert("alert.condition".into(), "no_data".into());

        let bare = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(bare.message.title, "NO DATA · Checkout API");
        assert_eq!(bare.message.text, "Alert Checkout API has no data.");

        incident.annotations.insert(
            "description".into(),
            "检查 Checkout API 没有结果：法兰克福 连续 3 次没有运行。".into(),
        );

        let described = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
        assert_eq!(
            described.message.text,
            "检查 Checkout API 没有结果：法兰克福 连续 3 次没有运行。"
        );
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
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
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
        assert_eq!(dispatch.message.title, "ESCALATION · API unavailable");
        assert_eq!(
            alert_dispatch(&incident, ALERT_ESCALATED_EVENT)
                .message
                .title,
            "ESCALATION · API unavailable"
        );
    }
}
