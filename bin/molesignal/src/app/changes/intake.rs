// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Changes as webhooks that cannot be shaped send them: read by a preset for a tool's own body
//! (LaunchDarkly, Unleash, Nacos, Apollo), or by JSON pointers the address names (`generic`).
//! What the body leaves unsaid comes from the address: the service, namespace, environment and
//! kind. A body may be one change or a list of them.

use domain::apm::ChangeKind;
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

use super::report::ReportedChange;

/// The most changes one request may carry.
pub const MAX_INTAKE_CHANGES: usize = 100;

/// Whose body a request carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeFormat {
    /// Read by the pointers the address names, else by the fields `POST /apm/deployments` takes.
    #[default]
    Generic,
    Launchdarkly,
    Unleash,
    Nacos,
    Apollo,
}

/// What the address says: what the body leaves unsaid, and where a generic body says the rest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeDefaults {
    #[serde(default)]
    pub format: IntakeFormat,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub kind: Option<ChangeKind>,
    /// JSON pointers into a generic body, such as `/app/name`.
    #[serde(default)]
    pub service_field: String,
    #[serde(default)]
    pub environment_field: String,
    #[serde(default)]
    pub version_field: String,
    #[serde(default)]
    pub description_field: String,
    #[serde(default)]
    pub author_field: String,
}

/// The changes a request's body tells.
pub fn read_changes(body: &Value, defaults: &IntakeDefaults) -> Result<Vec<ReportedChange>> {
    let items = match body {
        Value::Array(items) => items.iter().collect::<Vec<_>>(),
        other => vec![other],
    };

    if items.is_empty() {
        return Err(Error::invalid("the body tells no change"));
    }

    if items.len() > MAX_INTAKE_CHANGES {
        return Err(Error::invalid(format!(
            "a request carries at most {MAX_INTAKE_CHANGES} changes"
        )));
    }

    items
        .into_iter()
        .map(|item| {
            let mut change = match defaults.format {
                IntakeFormat::Generic => generic(item, defaults),
                IntakeFormat::Launchdarkly => launchdarkly(item),
                IntakeFormat::Unleash => unleash(item),
                IntakeFormat::Nacos => nacos(item),
                IntakeFormat::Apollo => apollo(item),
            };

            fill(&mut change, defaults);

            if change.service.trim().is_empty() {
                return Err(Error::invalid(
                    "the change names no service: add ?service= to the address",
                ));
            }

            Ok(change)
        })
        .collect()
}

/// What the body left unsaid, from the address; the address's kind wins.
fn fill(change: &mut ReportedChange, defaults: &IntakeDefaults) {
    let unsaid = |value: &mut String, default: &str| {
        if value.trim().is_empty() {
            *value = default.trim().to_owned();
        }
    };

    unsaid(&mut change.service, &defaults.service);
    unsaid(&mut change.namespace, &defaults.namespace);
    unsaid(&mut change.environment, &defaults.environment);

    if let Some(kind) = defaults.kind {
        change.kind = kind;
    }
}

/// Text at a JSON pointer, or at a field of the top level when the pointer has no slash.
fn at(item: &Value, pointer: &str) -> Option<String> {
    let pointer = pointer.trim();

    let value = if pointer.starts_with('/') {
        item.pointer(pointer)
    } else {
        item.get(pointer)
    }?;

    match value {
        Value::String(text) => Some(text.trim().to_owned()).filter(|text| !text.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The first of several places that says something.
fn first(item: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| at(item, pointer))
}

/// A time as milliseconds, microseconds or RFC 3339, in microseconds.
fn micros(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => {
            let number = number.as_i64()?;
            // Before the year 33658 a time in milliseconds has at most 13 digits.
            Some(if number.abs() < 10_000_000_000_000 {
                number * 1_000
            } else {
                number
            })
        }
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text.trim())
            .ok()
            .map(|time| time.timestamp_micros()),
        _ => None,
    }
}

fn generic(item: &Value, defaults: &IntakeDefaults) -> ReportedChange {
    let pointed = |pointer: &str, field: &str| {
        if pointer.trim().is_empty() {
            at(item, field)
        } else {
            at(item, pointer)
        }
    };

    ReportedChange {
        kind: item
            .get("kind")
            .and_then(Value::as_str)
            .and_then(ChangeKind::parse)
            .unwrap_or_default(),
        service: pointed(&defaults.service_field, "service").unwrap_or_default(),
        namespace: at(item, "namespace").unwrap_or_default(),
        environment: pointed(&defaults.environment_field, "environment").unwrap_or_default(),
        version: pointed(&defaults.version_field, "version").unwrap_or_default(),
        status: item
            .get("status")
            .cloned()
            .and_then(|status| serde_json::from_value(status).ok()),
        commit: at(item, "commit"),
        repository: at(item, "repository"),
        pull_request: at(item, "pullRequest"),
        author: pointed(&defaults.author_field, "author"),
        ci_url: at(item, "ciUrl"),
        description: pointed(&defaults.description_field, "description"),
        deployed_at: micros(item.get("deployedAt")),
    }
}

/// A LaunchDarkly webhook: an audit log entry of a flag. The environment is the `env/` part of the
/// resource it acted on.
fn launchdarkly(item: &Value) -> ReportedChange {
    let resource = item
        .pointer("/accesses/0/resource")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let environment = resource
        .split(':')
        .find_map(|part| part.strip_prefix("env/"))
        .map(|part| part.split(';').next().unwrap_or(part).to_owned())
        .unwrap_or_default();

    ReportedChange {
        kind: ChangeKind::FeatureFlag,
        environment,
        description: first(item, &["title", "description", "name"]),
        author: first(item, &["/member/email", "/member/firstName"]),
        deployed_at: micros(item.get("date")),
        ..ReportedChange::default()
    }
}

/// An Unleash webhook: an event of a feature toggle, as the add-on sends it by default.
fn unleash(item: &Value) -> ReportedChange {
    let feature = first(item, &["featureName", "/data/name"]).unwrap_or_default();
    let event = first(item, &["type"]).unwrap_or_default().replace('-', " ");

    ReportedChange {
        kind: ChangeKind::FeatureFlag,
        environment: first(item, &["environment"]).unwrap_or_default(),
        description: Some(format!("{event} {feature}").trim().to_owned())
            .filter(|text| !text.is_empty()),
        author: first(item, &["createdBy"]),
        deployed_at: micros(item.get("createdAt")),
        ..ReportedChange::default()
    }
}

/// A Nacos configuration change: the data id and group of what changed.
fn nacos(item: &Value) -> ReportedChange {
    let data_id = first(item, &["dataId", "/data/dataId"]).unwrap_or_default();
    let group = first(item, &["group", "/data/group"]).unwrap_or_default();

    ReportedChange {
        kind: ChangeKind::Configuration,
        namespace: first(item, &["namespace", "tenant"]).unwrap_or_default(),
        description: Some(format!("{data_id} ({group}) changed")),
        author: first(item, &["srcUser", "operator"]),
        deployed_at: micros(
            item.get("lastModifiedTime")
                .or_else(|| item.get("modifyTime")),
        ),
        ..ReportedChange::default()
    }
}

/// An Apollo release: the app is the service, the namespace and the release title what changed.
fn apollo(item: &Value) -> ReportedChange {
    let namespace = first(item, &["namespaceName"]).unwrap_or_default();
    let title = first(item, &["releaseTitle", "releaseComment"]).unwrap_or_default();

    ReportedChange {
        kind: ChangeKind::Configuration,
        service: first(item, &["appId"]).unwrap_or_default(),
        environment: first(item, &["env", "clusterName"]).unwrap_or_default(),
        description: Some(format!("{namespace} released {title}").trim().to_owned()),
        author: first(item, &["operator", "releasedBy"]),
        deployed_at: micros(item.get("releaseTime")),
        ..ReportedChange::default()
    }
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
