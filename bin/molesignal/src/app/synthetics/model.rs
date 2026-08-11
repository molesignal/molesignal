// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::validate_revision;
use crate::{
    domain::synthetics::{
        EgressPolicy, MonitorRevision, MonitorSchedule, MonitorSpec, MultiLocationPolicy,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateLocationInput {
    pub name: String,
    pub code: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub egress_policy: EgressPolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAgentConfigurationInput {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
pub struct CreateSecretInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub value: String,
}

impl std::fmt::Debug for CreateSecretInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateSecretInput")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMonitorInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub spec: MonitorSpec,
    pub schedule: MonitorSchedule,
    pub timeout_millis: u32,
    #[serde(default)]
    pub max_retries: u8,
    #[serde(default = "default_failure_threshold")]
    pub consecutive_failures: u32,
    #[serde(default = "default_recovery_threshold")]
    pub consecutive_recoveries: u32,
    #[serde(default = "default_freshness")]
    pub freshness_seconds: u32,
    #[serde(default = "default_location_policy")]
    pub location_policy: MultiLocationPolicy,
    #[serde(default)]
    pub location_ids: Vec<Id>,
    pub team_id: Option<Id>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub escalation_policy_id: Option<Id>,
    #[serde(default)]
    pub alert_on_degraded: bool,
}

const fn default_failure_threshold() -> u32 {
    3
}

const fn default_recovery_threshold() -> u32 {
    2
}

const fn default_freshness() -> u32 {
    300
}

fn default_location_policy() -> MultiLocationPolicy {
    MultiLocationPolicy::Majority
}

pub(super) fn build_revision(
    org_id: &Id,
    actor_id: &Id,
    monitor_id: &Id,
    number: u32,
    input: CreateMonitorInput,
    now: TimestampMicros,
) -> Result<MonitorRevision> {
    let content = serde_json::to_vec(&input)
        .map_err(|error| Error::internal(format!("serialize Monitor Revision: {error}")))?;
    let revision = MonitorRevision {
        id: Id::new(),
        organization_id: org_id.clone(),
        monitor_id: monitor_id.clone(),
        number,
        spec: input.spec,
        schedule: input.schedule,
        timeout_millis: input.timeout_millis,
        max_retries: input.max_retries,
        consecutive_failures: input.consecutive_failures,
        consecutive_recoveries: input.consecutive_recoveries,
        freshness_seconds: input.freshness_seconds,
        location_policy: input.location_policy,
        location_ids: input.location_ids,
        escalation_policy_id: input.escalation_policy_id,
        alert_on_degraded: input.alert_on_degraded,
        last_test_result_id: None,
        last_test_passed_at: None,
        created_by: actor_id.clone(),
        created_at: now,
        content_hash: blake3::hash(&content).to_hex().to_string(),
    };
    validate_revision(&revision)?;
    Ok(revision)
}

pub(super) fn validate_name(value: &str, max: usize, label: &str) -> Result<()> {
    let length = value.trim().chars().count();
    if length == 0 || length > max {
        return Err(Error::invalid(format!(
            "{label} must contain 1 to {max} characters"
        )));
    }
    Ok(())
}

pub(super) fn validate_code(value: &str) -> Result<()> {
    let value = value.trim();
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_alphanumeric()));
    if !valid {
        return Err(Error::invalid(
            "Location code must use lowercase letters, numbers, and single hyphens",
        ));
    }
    Ok(())
}

pub(super) fn validate_tags(tags: &[String]) -> Result<()> {
    if tags.len() > 50
        || tags
            .iter()
            .any(|tag| tag.trim().is_empty() || tag.len() > 128)
    {
        return Err(Error::invalid("Monitors support up to 50 non-empty tags"));
    }
    Ok(())
}

const MAX_AGENT_LABELS: usize = 32;
const MAX_AGENT_LABEL_KEY_CHARS: usize = 64;
const MAX_AGENT_LABEL_VALUE_CHARS: usize = 255;
const RESERVED_AGENT_LABELS: [&str; 2] = ["execution", "system_managed"];

pub(super) fn normalize_agent_labels(
    labels: BTreeMap<String, String>,
    current_labels: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut normalized = BTreeMap::new();
    for (key, value) in labels {
        let key = key.trim();
        if RESERVED_AGENT_LABELS.contains(&key) {
            continue;
        }
        let key_chars = key.chars().count();
        if key_chars == 0 || key_chars > MAX_AGENT_LABEL_KEY_CHARS {
            return Err(Error::invalid(format!(
                "Agent label keys must contain 1 to {MAX_AGENT_LABEL_KEY_CHARS} characters"
            )));
        }
        let value = value.trim();
        if value.chars().count() > MAX_AGENT_LABEL_VALUE_CHARS {
            return Err(Error::invalid(format!(
                "Agent label values cannot exceed {MAX_AGENT_LABEL_VALUE_CHARS} characters"
            )));
        }
        if normalized
            .insert(key.to_string(), value.to_string())
            .is_some()
        {
            return Err(Error::invalid("Agent label keys must be unique"));
        }
    }
    for reserved in RESERVED_AGENT_LABELS {
        if let Some(value) = current_labels.get(reserved) {
            normalized.insert(reserved.to_string(), value.clone());
        }
    }
    if normalized.len() > MAX_AGENT_LABELS {
        return Err(Error::invalid(format!(
            "Agents support up to {MAX_AGENT_LABELS} labels"
        )));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_labels_are_trimmed_and_reserved_values_are_preserved() {
        let current = BTreeMap::from([
            ("execution".into(), "embedded".into()),
            ("system_managed".into(), "true".into()),
        ]);
        let labels = BTreeMap::from([
            (" environment ".into(), " production ".into()),
            ("system_managed".into(), "false".into()),
        ]);

        let normalized = normalize_agent_labels(labels, &current).expect("valid Agent labels");

        assert_eq!(
            normalized.get("environment").map(String::as_str),
            Some("production")
        );
        assert_eq!(
            normalized.get("system_managed").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            normalized.get("execution").map(String::as_str),
            Some("embedded")
        );
    }

    #[test]
    fn agent_label_keys_must_remain_unique_after_trimming() {
        let labels = BTreeMap::from([
            ("environment".into(), "prod".into()),
            (" environment ".into(), "stage".into()),
        ]);

        let error = normalize_agent_labels(labels, &BTreeMap::new())
            .expect_err("trimmed duplicate label key must be rejected");

        assert!(error.to_string().contains("must be unique"));
    }
}
