// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What of an alert event the steps of an alert pipeline can change: the JSON they see, how it is
//! read back, and what changed between two versions, as an event's timeline records it.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::domain::alerting::{
    discovery::DISCOVERY_DIMENSIONS,
    incident::{Incident, SYNTHETIC_STATE_LABEL, Severity},
    priority::Priority,
    source::is_reserved_label,
    urgency::Urgency,
};

/// The longest title a step may give an event.
const MAX_SUMMARY_CHARS: usize = 512;

/// The longest value a recorded change keeps.
const MAX_CHANGE_VALUE_CHARS: usize = 200;

/// At most this many changes are recorded of one pipeline; the rest are only counted.
pub const MAX_RECORDED_CHANGES: usize = 40;

/// The parts of an alert event that steps can change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertFields {
    pub summary: String,
    pub severity: Severity,
    pub priority: Option<Priority>,
    pub urgency: Option<Urgency>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub affected_services: Vec<String>,
}

/// Whether MoleSignal sets this label itself and goes by it.
fn is_own_label(key: &str) -> bool {
    is_reserved_label(key) || key == SYNTHETIC_STATE_LABEL || DISCOVERY_DIMENSIONS.contains(&key)
}

/// One thing a pipeline changed: `field` is `severity`, `labels.team` and the like. A value that was
/// added has no `from`, one that was taken away no `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlertFieldChange {
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl AlertFields {
    pub fn of(incident: &Incident) -> Self {
        Self {
            summary: incident.summary.clone(),
            severity: incident.severity,
            priority: incident.handling.priority,
            urgency: incident.handling.urgency.clone(),
            labels: incident.labels.clone(),
            annotations: incident.annotations.clone(),
            affected_services: incident.affected_services.clone(),
        }
    }

    /// These fields written onto `incident`.
    pub fn apply_to(self, incident: &mut Incident) {
        incident.summary = self.summary;
        incident.severity = self.severity;
        incident.handling.priority = self.priority;
        incident.handling.urgency = self.urgency;
        incident.labels = self.labels;
        incident.annotations = self.annotations;
        incident.affected_services = self.affected_services;
    }

    /// The event the steps see. `priority` and `urgency` are null until something sets them; a null
    /// urgency is decided by the escalation policy after the pipelines.
    pub fn event(&self) -> Value {
        json!({
            "summary": self.summary,
            "severity": self.severity.as_str(),
            "priority": self.priority,
            "urgency": self.urgency,
            "labels": self.labels,
            "annotations": self.annotations,
            "affected_services": self.affected_services,
        })
    }

    /// These fields as the steps left `event`, or why that event cannot be an alert event. A field
    /// the event no longer has stays as it was.
    pub fn changed(&self, event: &Value) -> std::result::Result<Self, String> {
        let mut changed = self.clone();

        if let Some(summary) = event.get("summary") {
            let summary = summary
                .as_str()
                .map(str::trim)
                .filter(|summary| !summary.is_empty())
                .ok_or("`summary` must be text")?;

            changed.summary = summary.chars().take(MAX_SUMMARY_CHARS).collect();
        }

        if let Some(severity) = event.get("severity") {
            changed.severity = severity
                .as_str()
                .and_then(Severity::from_label)
                .ok_or("`severity` must be info, warning, error or critical")?;
        }

        if let Some(priority) = event.get("priority") {
            changed.priority = match priority {
                Value::Null => None,
                Value::String(word) => {
                    Some(Priority::from_label(word).ok_or("`priority` must be P1 to P9, or null")?)
                }
                _ => return Err("`priority` must be P1 to P9, or null".into()),
            };
        }

        if let Some(urgency) = event.get("urgency") {
            changed.urgency = match urgency {
                Value::Null => None,
                Value::String(word) => Some(
                    Urgency::from_label(word)
                        .ok_or("`urgency` must be an urgency key such as high or low, or null")?,
                ),
                _ => return Err("`urgency` must be high, low or null".into()),
            };
        }

        if let Some(labels) = event.get("labels") {
            changed.labels = self.with_own_labels(texts(labels, "labels")?);
        }

        if let Some(annotations) = event.get("annotations") {
            changed.annotations = texts(annotations, "annotations")?;
        }

        if let Some(services) = event.get("affected_services") {
            changed.affected_services = service_names(services)?;
        }

        Ok(changed)
    }

    /// `labels` with the labels MoleSignal set itself as they were: it goes by them to tell where an
    /// incident came from, so a step can neither take them away nor make them up.
    fn with_own_labels(&self, mut labels: BTreeMap<String, String>) -> BTreeMap<String, String> {
        labels.retain(|key, _| !is_own_label(key));

        for (key, value) in &self.labels {
            if is_own_label(key) {
                labels.insert(key.clone(), value.clone());
            }
        }

        labels
    }

    /// What differs in `after`, in a steady order: the title and importance, then labels, annotations
    /// and services.
    pub fn changes_to(&self, after: &Self) -> Vec<AlertFieldChange> {
        let mut changes = Vec::new();

        let mut text = |field: &str, from: Option<&str>, to: Option<&str>| {
            if from != to {
                changes.push(AlertFieldChange {
                    field: field.to_owned(),
                    from: from.map(short),
                    to: to.map(short),
                });
            }
        };

        text("summary", Some(&self.summary), Some(&after.summary));

        text(
            "severity",
            Some(self.severity.as_str()),
            Some(after.severity.as_str()),
        );

        text(
            "priority",
            self.priority.map(Priority::as_str).as_deref(),
            after.priority.map(Priority::as_str).as_deref(),
        );

        text(
            "urgency",
            self.urgency.as_ref().map(Urgency::as_str),
            after.urgency.as_ref().map(Urgency::as_str),
        );

        for (group, before, now) in [
            ("labels", &self.labels, &after.labels),
            ("annotations", &self.annotations, &after.annotations),
        ] {
            let keys = before
                .keys()
                .chain(now.keys())
                .collect::<std::collections::BTreeSet<_>>();

            for key in keys {
                text(
                    &format!("{group}.{key}"),
                    before.get(key).map(String::as_str),
                    now.get(key).map(String::as_str),
                );
            }
        }

        if self.affected_services != after.affected_services {
            text(
                "affected_services",
                Some(&self.affected_services.join(", ")),
                Some(&after.affected_services.join(", ")),
            );
        }

        changes
    }
}

/// A value as a change records it: not so long that one annotation fills the timeline.
fn short(value: &str) -> String {
    if value.chars().count() <= MAX_CHANGE_VALUE_CHARS {
        return value.to_owned();
    }

    let mut text: String = value.chars().take(MAX_CHANGE_VALUE_CHARS).collect();
    text.push('…');
    text
}

/// A map of text, as labels and annotations are: numbers and booleans become their text, a null
/// takes the key away.
fn texts(value: &Value, name: &str) -> std::result::Result<BTreeMap<String, String>, String> {
    let fields = value
        .as_object()
        .ok_or_else(|| format!("`{name}` must be an object"))?;

    let mut texts = BTreeMap::new();

    for (key, value) in fields {
        let key = key.trim();

        if key.is_empty() {
            continue;
        }

        let text = match value {
            Value::Null => continue,
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            Value::Bool(flag) => flag.to_string(),
            _ => return Err(format!("`{name}.{key}` must be text")),
        };

        texts.insert(key.to_owned(), text);
    }

    Ok(texts)
}

fn service_names(value: &Value) -> std::result::Result<Vec<String>, String> {
    let services = value
        .as_array()
        .ok_or("`affected_services` must be a list of names")?;

    let mut names: Vec<String> = Vec::new();

    for service in services {
        let name = service
            .as_str()
            .ok_or("`affected_services` must be a list of names")?;

        if !name.is_empty() && !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }

    Ok(names)
}

/// `event` with what an outside service answered merged in: only the fields a step can change, the
/// labels and annotations key by key (a null takes a key away), so a service can answer with just
/// what it adds. Anything else in the answer is left out.
pub fn merge_answer(event: &mut Value, answer: &Map<String, Value>) {
    for field in [
        "summary",
        "severity",
        "priority",
        "urgency",
        "affected_services",
    ] {
        if let Some(value) = answer.get(field) {
            event[field] = value.clone();
        }
    }

    for field in ["labels", "annotations"] {
        let Some(added) = answer.get(field) else {
            continue;
        };

        match (added.as_object(), event[field].as_object_mut()) {
            (Some(added), Some(fields)) => {
                fields.extend(
                    added
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
            _ => event[field] = added.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> AlertFields {
        AlertFields {
            summary: "Checkout errors".into(),
            severity: Severity::Error,
            priority: None,
            urgency: None,
            labels: [("env".to_string(), "dev".to_string())].into(),
            annotations: BTreeMap::new(),
            affected_services: vec!["checkout".into()],
        }
    }

    #[test]
    fn what_a_pipeline_changed_is_named_field_by_field() {
        let before = fields();
        let after = before
            .changed(&json!({
                "severity": "critical",
                "priority": "p2",
                "labels": { "team": "payments", "env": "prod" },
                "annotations": { "runbook": "https://x" },
                "affected_services": ["checkout", "payments"],
            }))
            .unwrap();

        let change = |field: &str, from: Option<&str>, to: Option<&str>| AlertFieldChange {
            field: field.into(),
            from: from.map(str::to_owned),
            to: to.map(str::to_owned),
        };

        assert_eq!(
            before.changes_to(&after),
            vec![
                change("severity", Some("error"), Some("critical")),
                change("priority", None, Some("P2")),
                change("labels.env", Some("dev"), Some("prod")),
                change("labels.team", None, Some("payments")),
                change("annotations.runbook", None, Some("https://x")),
                change(
                    "affected_services",
                    Some("checkout"),
                    Some("checkout, payments")
                ),
            ]
        );

        let without = before.changed(&json!({ "labels": {} })).unwrap();
        assert_eq!(
            before.changes_to(&without),
            vec![change("labels.env", Some("dev"), None)]
        );
        assert!(before.changes_to(&before).is_empty());
    }

    #[test]
    fn a_step_can_neither_take_away_nor_make_up_the_labels_molesignal_goes_by() {
        let mut before = fields();
        before
            .labels
            .insert("alert.source".into(), "external".into());
        before
            .labels
            .insert("external.kind".into(), "alertmanager".into());
        before
            .labels
            .insert("synthetic.state".into(), "down".into());

        let own: BTreeMap<String, String> = [
            ("alert.source".to_string(), "external".to_string()),
            ("external.kind".to_string(), "alertmanager".to_string()),
            ("synthetic.state".to_string(), "down".to_string()),
        ]
        .into();

        assert_eq!(
            before.changed(&json!({ "labels": {} })).unwrap().labels,
            own
        );

        let after = before
            .changed(&json!({ "labels": {
                "team": "payments",
                "alert.source": "manual",
                "alert.condition": "no_data",
                "external.kind": "forged",
                "__molesignal_x": "y",
            } }))
            .unwrap();

        let mut expected = own.clone();
        expected.insert("team".into(), "payments".into());

        assert_eq!(after.labels, expected);
        assert!(
            before
                .changes_to(&after)
                .iter()
                .all(|change| !change.field.contains("alert.")
                    && !change.field.contains("external.kind"))
        );

        // On an event that has none, there is nothing to keep and nothing a step can add.
        let plain = fields()
            .changed(&json!({ "labels": { "alert.source": "external" } }))
            .unwrap();

        assert!(plain.labels.is_empty());
    }

    #[test]
    fn a_long_value_is_cut_where_a_change_records_it() {
        let before = fields();
        let long = "x".repeat(MAX_CHANGE_VALUE_CHARS + 50);
        let after = before
            .changed(&json!({ "annotations": { "why": long } }))
            .unwrap();
        let change = before.changes_to(&after).pop().unwrap();

        assert_eq!(
            change.to.unwrap().chars().count(),
            MAX_CHANGE_VALUE_CHARS + 1
        );
        // The event itself keeps all of it.
        assert_eq!(after.annotations["why"].len(), MAX_CHANGE_VALUE_CHARS + 50);
    }

    #[test]
    fn an_answer_adds_to_the_labels_and_changes_only_what_a_step_can() {
        let mut event = fields().event();

        merge_answer(
            &mut event,
            json!({
                "severity": "critical",
                "labels": { "team": "payments", "env": null },
                "annotations": { "owner": "ada" },
                "status": "resolved",
                "fingerprint": "other",
            })
            .as_object()
            .unwrap(),
        );

        let after = fields().changed(&event).unwrap();

        assert_eq!(after.severity, Severity::Critical);
        assert_eq!(
            after.labels,
            BTreeMap::from([("team".to_string(), "payments".to_string())])
        );
        assert_eq!(after.annotations["owner"], "ada");
        assert!(event.get("status").is_none() && event.get("fingerprint").is_none());
    }
}
