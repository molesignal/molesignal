// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The `to_change` step: Kubernetes objects as an OpenTelemetry Collector's `k8sobjects` receiver
//! watches them into a log stream, read as changes. A workload whose generation moved changed: a
//! new image is a deployment, new replicas a scaling, anything else a configuration change; a
//! ConfigMap whose data moved is a configuration change; a workload deleted is an infrastructure
//! change. The step changes no field.
//!
//! A watch tells an object as it is, not what changed, so what each object looked like when last
//! seen is kept on the node ([`WatchedObjects`]). The first sighting of an object is only kept: a
//! collector that starts again lists every object, and that is no change. A workload created
//! shortly before it is seen is a deployment all the same.

use std::collections::HashMap;

use kernel::{ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::Value;

use crate::domain::apm::ChangeKind;

/// The most objects a node keeps the last sighting of; past it new objects are not watched.
pub const MAX_WATCHED_OBJECTS: usize = 50_000;
/// A workload created this long before it is first seen is new: a deployment.
const NEW_WORKLOAD_MICROS: i64 = 10 * 60 * 1_000_000;
/// The most characters of a change's description.
const MAX_DESCRIPTION_CHARS: usize = 300;

/// Where a `to_change` step takes the environment of the changes it notes.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSpec {
    /// The environment every change is of.
    #[serde(default)]
    pub environment: String,
    /// Else the event field that names it, such as `k8s.cluster.name`.
    #[serde(default)]
    pub environment_field: String,
}

/// A change a step noted, named as the cluster names it; the organization's naming rules are
/// applied where it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotedChange {
    pub kind: ChangeKind,
    pub namespace: String,
    pub service: String,
    pub environment: String,
    pub version: String,
    pub description: String,
    pub at: TimestampMicros,
}

/// What an object looked like when last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    generation: i64,
    images: Vec<String>,
    replicas: Option<i64>,
    /// A ConfigMap's data, as written.
    data: String,
}

/// The last sighting of each object, by organization and object.
#[derive(Debug, Default)]
pub struct WatchedObjects {
    seen: HashMap<(Id, String), Seen>,
}

const WORKLOADS: [&str; 3] = ["Deployment", "StatefulSet", "DaemonSet"];

impl WatchedObjects {
    /// The change `value` tells, if it tells one, and what the object looks like now kept.
    pub fn note(
        &mut self,
        org_id: &Id,
        value: &Value,
        spec: &ChangeSpec,
        at: TimestampMicros,
    ) -> Option<NotedChange> {
        let (verb, object) = watched(value)?;
        let object = &object;
        let kind = object.get("kind")?.as_str()?;
        let metadata = object.get("metadata")?;
        let name = text(metadata, "name")?;
        let namespace = text(metadata, "namespace").unwrap_or_default();
        let key = (org_id.clone(), format!("{kind}/{namespace}/{name}"));
        let workload = WORKLOADS.contains(&kind);

        if !workload && kind != "ConfigMap" {
            return None;
        }

        let noted = |kind: ChangeKind, version: String, description: String| NotedChange {
            kind,
            namespace: namespace.to_owned(),
            service: service_of(metadata, name),
            environment: environment_of(value, spec),
            version,
            description: cut(&description),
            at,
        };

        if verb == "DELETED" {
            self.seen.remove(&key);

            return workload.then(|| {
                noted(
                    ChangeKind::Infrastructure,
                    String::new(),
                    format!("{kind} {name} deleted"),
                )
            });
        }

        let now = Seen {
            generation: metadata
                .get("generation")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            images: images_of(object),
            replicas: object.pointer("/spec/replicas").and_then(Value::as_i64),
            data: object.get("data").map(Value::to_string).unwrap_or_default(),
        };

        let Some(before) = self.seen.get(&key).cloned() else {
            if self.seen.len() < MAX_WATCHED_OBJECTS {
                self.seen.insert(key, now.clone());
            }

            // A workload made just now is new; any other first sighting only starts the watch.
            let created = text(metadata, "creationTimestamp").and_then(micros_of);
            let fresh = created.is_some_and(|created| at.0 - created <= NEW_WORKLOAD_MICROS);

            return (workload && fresh).then(|| {
                noted(
                    ChangeKind::Deployment,
                    version_of(now.images.first().map(String::as_str).unwrap_or_default()),
                    format!("{kind} {name} created"),
                )
            });
        };

        self.seen.insert(key, now.clone());

        if !workload {
            return (now.data != before.data).then(|| {
                noted(
                    ChangeKind::Configuration,
                    String::new(),
                    format!("ConfigMap {name} changed"),
                )
            });
        }

        if now.generation <= before.generation {
            return None;
        }

        if now.images != before.images {
            let changed = now
                .images
                .iter()
                .find(|image| !before.images.contains(image))
                .cloned()
                .unwrap_or_default();

            return Some(noted(
                ChangeKind::Deployment,
                version_of(&changed),
                format!(
                    "{kind} {name}: {} → {}",
                    before.images.join(", "),
                    now.images.join(", ")
                ),
            ));
        }

        if now.replicas != before.replicas {
            let count = |replicas: Option<i64>| {
                replicas.map_or_else(|| "?".to_owned(), |count| count.to_string())
            };

            return Some(noted(
                ChangeKind::Scaling,
                String::new(),
                format!(
                    "{kind} {name}: {} → {} replicas",
                    count(before.replicas),
                    count(now.replicas)
                ),
            ));
        }

        Some(noted(
            ChangeKind::Configuration,
            String::new(),
            format!(
                "{kind} {name}: spec changed (generation {})",
                now.generation
            ),
        ))
    }
}

/// The watch verb and the object of an event: the receiver's `{type, object}` in the body, as an
/// object or as JSON text, or an object alone, which a pull reads as `ADDED`.
fn watched(value: &Value) -> Option<(String, Value)> {
    let body = match value.get("body") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok()?,
        Some(body @ Value::Object(_)) => body.clone(),
        _ => value.clone(),
    };

    match body.get("object") {
        Some(object) => Some((body.get("type")?.as_str()?.to_owned(), object.clone())),
        None => Some(("ADDED".to_owned(), body)),
    }
}

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The service an object belongs to: its `app.kubernetes.io/name` or `app` label, else its name.
fn service_of(metadata: &Value, name: &str) -> String {
    let labels = metadata.get("labels");

    ["app.kubernetes.io/name", "app"]
        .iter()
        .find_map(|label| labels.and_then(|labels| text(labels, label)))
        .unwrap_or(name)
        .to_owned()
}

fn environment_of(value: &Value, spec: &ChangeSpec) -> String {
    let set = spec.environment.trim();

    if !set.is_empty() || spec.environment_field.trim().is_empty() {
        return set.to_owned();
    }

    value
        .get(spec.environment_field.trim())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The images of a workload's containers, sorted.
fn images_of(object: &Value) -> Vec<String> {
    let mut images = object
        .pointer("/spec/template/spec/containers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|container| text(container, "image").map(str::to_owned))
        .collect::<Vec<_>>();

    images.sort();
    images
}

/// The version an image names: its tag, or its digest, or the image itself.
fn version_of(image: &str) -> String {
    let image = image.trim();

    if let Some((_, digest)) = image.split_once('@') {
        return digest.to_owned();
    }

    match image.rsplit_once(':') {
        // A colon in the last path segment is a tag; one before a slash is a registry's port.
        Some((_, tag)) if !tag.contains('/') => tag.to_owned(),
        _ => image.to_owned(),
    }
}

fn micros_of(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|time| time.timestamp_micros())
}

fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_DESCRIPTION_CHARS {
        return text.to_owned();
    }

    text.chars().take(MAX_DESCRIPTION_CHARS).collect::<String>() + "…"
}

#[cfg(test)]
#[path = "change_tests.rs"]
mod tests;
