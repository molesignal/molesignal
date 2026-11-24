// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The changes read from each place, put in one list: kept by the query,
//! a version APM saw that a pipeline also reported told once, the latest
//! first.

use std::collections::HashSet;

use domain::{
    alerting::rule::AlertRule,
    apm::{DeploymentEvent, VersionObservation},
};
use kernel::time::TimestampMicros;
use postgres::repositories::annotations::Annotation;
use serde::Serialize;

use super::{ChangeFeedKind, ChangeFeedQuery, MOST_CHANGES};

/// One change as a list tells it; each part is said only when known.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    pub id: String,
    pub kind: ChangeFeedKind,
    pub at: TimestampMicros,
    /// When a change over a range ended: an annotation's end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<TimestampMicros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// How a reported change went: `succeeded`, `failed` or `rolled_back`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'static str>,
    /// An annotation's or a rule's title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

impl ChangeEntry {
    fn new(id: String, kind: ChangeFeedKind, at: TimestampMicros) -> Self {
        Self {
            id,
            kind,
            at,
            end: None,
            service: None,
            namespace: None,
            environment: None,
            version: None,
            status: None,
            title: None,
            description: None,
            tags: Vec::new(),
            commit: None,
            repository_url: None,
            pull_request_url: None,
            author: None,
            ci_url: None,
            dashboard_id: None,
            stream: None,
            rule_id: None,
        }
    }
}

/// What each place answered.
#[derive(Debug, Default)]
pub struct Sources {
    pub reported: Vec<DeploymentEvent>,
    pub annotations: Vec<Annotation>,
    pub versions: Vec<VersionObservation>,
    pub rules: Vec<AlertRule>,
}

/// The changes of `sources` that `query` keeps and are of a `wanted` kind, the latest first, at
/// most the query's limit.
pub fn merge(
    sources: Sources,
    query: &ChangeFeedQuery,
    wanted: &[ChangeFeedKind],
) -> Vec<ChangeEntry> {
    let in_range = |at: TimestampMicros| at >= query.range.start && at <= query.range.end;

    let of_service = |service: &str| {
        service.is_empty()
            || query.services.is_empty()
            || query.services.iter().any(|name| name == service)
    };

    let reported = sources
        .reported
        .into_iter()
        .filter(|event| in_range(event.deployed_at) && of_service(&event.service))
        .filter_map(|event| {
            let kind = ChangeFeedKind::parse(event.kind.as_str())?;
            wanted.contains(&kind).then(|| reported_entry(event, kind))
        })
        .collect::<Vec<_>>();

    // A version a pipeline reported is told once, as reported.
    let told = reported
        .iter()
        .filter_map(|entry| Some((entry.service.clone()?, entry.version.clone()?)))
        .collect::<HashSet<_>>();

    let versions = sources
        .versions
        .into_iter()
        .filter(|_| wanted.contains(&ChangeFeedKind::Version))
        .filter(|version| in_range(version.first_seen_at) && of_service(&version.service.name))
        .filter(|version| !told.contains(&(version.service.name.clone(), version.version.clone())))
        .map(|version| {
            let mut entry = ChangeEntry::new(
                format!(
                    "version:{}:{}",
                    version.service.stable_key(),
                    version.version
                ),
                ChangeFeedKind::Version,
                version.first_seen_at,
            );

            entry.service = Some(version.service.name);
            entry.namespace = Some(version.service.namespace);
            entry.environment = Some(version.service.environment);
            entry.version = Some(version.version);
            entry
        });

    let annotations = sources
        .annotations
        .into_iter()
        .filter(|_| wanted.contains(&ChangeFeedKind::Annotation))
        .filter(|annotation| {
            annotation.time_end >= query.range.start && annotation.time_start <= query.range.end
        })
        .filter(|annotation| {
            let dashboard = annotation.dashboard_id.as_ref().map(|id| id.0.as_str());
            let stream = annotation.stream_name.as_deref();

            (dashboard.is_none() || dashboard == query.dashboard_id.as_deref())
                && (stream.is_none() || stream == query.stream.as_deref())
        })
        .map(annotation_entry);

    let rules = sources
        .rules
        .into_iter()
        .filter(|_| wanted.contains(&ChangeFeedKind::Rule))
        .filter(|rule| in_range(rule.updated_at))
        .map(|rule| {
            let mut entry = ChangeEntry::new(
                format!("rule:{}:{}", rule.id.0, rule.updated_at.0),
                ChangeFeedKind::Rule,
                rule.updated_at,
            );

            entry.title = Some(rule.name);
            entry.rule_id = Some(rule.id.0);
            entry
        });

    let mut entries = reported
        .into_iter()
        .chain(versions)
        .chain(annotations)
        .chain(rules)
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| right.at.cmp(&left.at).then_with(|| left.id.cmp(&right.id)));
    entries.truncate(query.limit.clamp(1, MOST_CHANGES));
    entries
}

fn reported_entry(event: DeploymentEvent, kind: ChangeFeedKind) -> ChangeEntry {
    let mut entry = ChangeEntry::new(event.id.0, kind, event.deployed_at);
    let present = |value: String| (!value.is_empty()).then_some(value);

    entry.service = present(event.service);
    entry.namespace = present(event.namespace);
    entry.environment = present(event.environment);
    entry.version = present(event.version);
    entry.status = Some(event.status.as_str());
    entry.description = event.description;
    entry.commit = event.commit;
    entry.repository_url = event.repository_url;
    entry.pull_request_url = event.pull_request_url;
    entry.author = event.author;
    entry.ci_url = event.ci_url;
    entry
}

fn annotation_entry(annotation: Annotation) -> ChangeEntry {
    let mut entry = ChangeEntry::new(
        annotation.id.0,
        ChangeFeedKind::Annotation,
        annotation.time_start,
    );

    entry.end = (annotation.time_end > annotation.time_start).then_some(annotation.time_end);
    entry.title = Some(annotation.title);
    entry.description = annotation.description;
    entry.tags = annotation.tags;
    entry.dashboard_id = annotation.dashboard_id.map(|id| id.0);
    entry.stream = annotation.stream_name;
    entry
}
