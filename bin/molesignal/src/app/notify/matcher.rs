// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;

use crate::{
    domain::notify::policy::{NotifyEvent, NotifyPolicy},
    shared::{Error, Result},
};

pub fn validate_matchers(matchers: &Value) -> Result<()> {
    let fields = matchers
        .as_object()
        .ok_or_else(|| Error::invalid("notify policy matchers must be a JSON object"))?;
    for (path, expected) in fields {
        if path.trim().is_empty() {
            return Err(Error::invalid("notify policy matcher path cannot be empty"));
        }
        if expected.is_object() {
            return Err(Error::invalid(format!(
                "notify policy matcher `{path}` must be a scalar or array"
            )));
        }
        if expected.as_array().is_some_and(Vec::is_empty) {
            return Err(Error::invalid(format!(
                "notify policy matcher `{path}` array cannot be empty"
            )));
        }
    }
    Ok(())
}

pub fn policy_matches(policy: &NotifyPolicy, event: &NotifyEvent) -> Result<bool> {
    if policy.event_type != event.event_type {
        return Ok(false);
    }
    validate_matchers(&policy.matchers)?;
    Ok(policy.matchers.as_object().is_some_and(|matchers| {
        matchers.iter().all(|(path, expected)| {
            value_at_path(&event.attributes, path)
                .is_some_and(|actual| value_matches(actual, expected))
        })
    }))
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, segment| current.get(segment))
}

fn value_matches(actual: &Value, expected: &Value) -> bool {
    match (actual.as_array(), expected.as_array()) {
        (Some(actual), Some(expected)) => actual
            .iter()
            .any(|actual| expected.iter().any(|expected| actual == expected)),
        (Some(actual), None) => actual.iter().any(|actual| actual == expected),
        (None, Some(expected)) => expected.iter().any(|expected| actual == expected),
        (None, None) => actual == expected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::notify::{
            policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyFallbackConfig},
            preference::NotifyCategory,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn policy(matchers: Value) -> NotifyPolicy {
        NotifyPolicy {
            id: Id::new(),
            organization_id: Id::new(),
            name: "critical production".into(),
            event_type: "alert.triggered".into(),
            category: NotifyCategory::Alert,
            matchers,
            recipient_resolver: "fixed_users".into(),
            resolver_config: serde_json::json!({"user_ids": ["user-a"]}),
            delivery_mode: NotifyDeliveryMode::PreferUser,
            delivery_config: NotifyDeliveryConfig::default(),
            template_id: None,
            fallback_config: NotifyFallbackConfig::default(),
            ack_timeout_seconds: None,
            escalation_config: None,
            enabled: true,
            priority: 100,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn event(attributes: Value) -> NotifyEvent {
        NotifyEvent {
            id: "event-a".into(),
            event_type: "alert.triggered".into(),
            organization_id: Id::new(),
            occurred_at: TimestampMicros(1),
            attributes,
        }
    }

    #[test]
    fn matches_nested_scalar_and_any_of_values() {
        let policy = policy(serde_json::json!({
            "severity": ["critical", "high"],
            "resource.environment": "production"
        }));
        let event = event(serde_json::json!({
            "severity": "critical",
            "resource": {"environment": "production"}
        }));
        assert!(policy_matches(&policy, &event).unwrap());
    }

    #[test]
    fn missing_attribute_does_not_match() {
        let policy = policy(serde_json::json!({"severity": "critical"}));
        assert!(!policy_matches(&policy, &event(serde_json::json!({}))).unwrap());
    }

    #[test]
    fn rejects_ambiguous_object_matcher() {
        assert!(validate_matchers(&serde_json::json!({"resource": {"env": "prod"}})).is_err());
    }
}
