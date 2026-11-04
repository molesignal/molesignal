// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A look at the instance a span came from: which instance it is, and what its
//! resource says about where and how it runs. An instance says the same in all
//! its spans, so the projector takes a look now and then, not from every span.

use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
};

use dashmap::DashMap;
use serde_json::Value;

use crate::{
    domain::apm::{ApmSpanFact, INSTANCE_ATTRIBUTES, INSTANCE_ENTRY_SCOPE, InstanceSighting},
    shared::trace_normalization::CanonicalSpan,
};

/// How long, in event time, before an instance is looked at again.
const LOOK_EVERY_MICROS: i64 = 60_000_000;

/// The most instances remembered; past it the memory starts over.
const MAX_REMEMBERED: usize = 100_000;

/// The longest attribute value an instance keeps.
const MAX_VALUE_BYTES: usize = 192;

/// The longest repository link an instance keeps.
const MAX_REPOSITORY_BYTES: usize = 512;

/// The attributes that carry a repository link, cleaned of any credential.
const REPOSITORY_ATTRIBUTES: [&str; 2] = ["vcs.repository.url.full", "git.repository_url"];

/// What tells instances apart, the first one reported winning: an SDK's own
/// instance id, else the pod, else the host it runs on.
pub(super) const INSTANCE_KEYS: [&str; 3] = ["service.instance.id", "k8s.pod.name", "host.name"];

/// When each instance was last looked at, so a look is taken once a minute
/// of its spans rather than from every span.
#[derive(Default)]
pub struct InstanceLooks {
    last: DashMap<u64, i64>,
}

impl InstanceLooks {
    /// Gives the fact a look at the span's instance when that instance was
    /// not looked at within the last minute of event time.
    pub fn attach(&self, fact: &mut ApmSpanFact, span: &CanonicalSpan) {
        let resource = &span.resource.attributes;
        let Some(instance) = INSTANCE_KEYS
            .iter()
            .find_map(|key| resource.get(*key).and_then(Value::as_str))
        else {
            return;
        };

        let mut hasher = DefaultHasher::new();
        (fact.org_id.as_str(), &fact.service, instance).hash(&mut hasher);

        let key = hasher.finish();
        let now = fact.event_time.0;

        if self
            .last
            .get(&key)
            .is_some_and(|at| now - *at < LOOK_EVERY_MICROS)
        {
            return;
        }

        if self.last.len() >= MAX_REMEMBERED {
            self.last.clear();
        }

        self.last.insert(key, now);
        fact.instance = instance_sighting(span).map(Box::new);
    }
}

/// The instance a span came from and its resource attributes; none when the
/// span says nothing that tells its instance apart.
pub fn instance_sighting(span: &CanonicalSpan) -> Option<InstanceSighting> {
    let resource = &span.resource.attributes;
    let instance_id = INSTANCE_KEYS.iter().find_map(|key| text(resource, key))?;
    let mut attributes = BTreeMap::new();

    for key in INSTANCE_ATTRIBUTES {
        let value = if REPOSITORY_ATTRIBUTES.contains(key) {
            resource
                .get(*key)
                .and_then(Value::as_str)
                .and_then(repository_link)
        } else {
            text(resource, key)
        };

        if let Some(value) = value {
            attributes.insert((*key).to_owned(), value);
        }
    }

    // The library that served a request (spring-webmvc, axum, express …) names the framework.
    if matches!(span.kind, 2 | 5)
        && let Some(scope) = bounded(&span.scope.name)
    {
        attributes.insert(INSTANCE_ENTRY_SCOPE.to_owned(), scope);
    }

    Some(InstanceSighting {
        instance_id,
        attributes,
    })
}

fn text(attributes: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    attributes
        .get(key)
        .and_then(Value::as_str)
        .and_then(bounded)
}

/// A value as it is, unless it is empty, too long, or an address; an address
/// can carry a credential, and only repository links are kept, cleaned.
fn bounded(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty() || value.len() > MAX_VALUE_BYTES || value.contains("://") {
        return None;
    }

    Some(value.to_owned())
}

/// A repository as a web link: an SSH address becomes https, and the user,
/// password, query and fragment are dropped so no credential is kept.
fn repository_link(value: &str) -> Option<String> {
    let value = value.trim();
    let web = match value.strip_prefix("git@") {
        Some(rest) => {
            let (host, path) = rest.split_once(':')?;
            format!("https://{host}/{path}")
        }
        None => value.to_owned(),
    };
    let mut url = url::Url::parse(&web).ok()?;

    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }

    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_query(None);
    url.set_fragment(None);

    let link = url
        .as_str()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned();

    (link.len() <= MAX_REPOSITORY_BYTES).then_some(link)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn span(kind: i32, resource: &[(&str, &str)]) -> CanonicalSpan {
        let mut span: CanonicalSpan = serde_json::from_value(json!({
            "trace_id": "t", "span_id": "s", "name": "GET /cart", "kind": kind,
            "start_time_unix_nano": 0, "end_time_unix_nano": 1, "duration_ns": 1, "status_code": "OK",
            "scope": { "name": "opentelemetry-instrumentation-axum" }
        }))
        .expect("span");

        for (key, value) in resource {
            span.resource.attributes.insert((*key).into(), json!(value));
        }

        span
    }

    #[test]
    fn an_instance_is_told_apart_by_its_id_then_its_pod_then_its_host() {
        let reported = span(
            2,
            &[
                ("service.instance.id", "i-1"),
                ("k8s.pod.name", "pod-1"),
                ("host.name", "node-1"),
            ],
        );
        let podded = span(2, &[("k8s.pod.name", "pod-1"), ("host.name", "node-1")]);
        let hosted = span(2, &[("host.name", "node-1")]);

        assert_eq!(
            instance_sighting(&reported).map(|s| s.instance_id),
            Some("i-1".into())
        );
        assert_eq!(
            instance_sighting(&podded).map(|s| s.instance_id),
            Some("pod-1".into())
        );
        assert_eq!(
            instance_sighting(&hosted).map(|s| s.instance_id),
            Some("node-1".into())
        );
        assert!(instance_sighting(&span(2, &[("cloud.region", "eu-west-1")])).is_none());
    }

    #[test]
    fn keeps_where_and_how_it_runs_and_the_entry_library() {
        let sighting = instance_sighting(&span(
            2,
            &[
                ("service.instance.id", "i-1"),
                ("cloud.region", "ap-southeast-1"),
                ("process.runtime.name", "rustc"),
                ("deployment.environment.name", "prod"),
            ],
        ))
        .expect("sighting");

        assert_eq!(sighting.attributes["cloud.region"], "ap-southeast-1");
        assert_eq!(sighting.attributes["process.runtime.name"], "rustc");
        assert_eq!(
            sighting.attributes[INSTANCE_ENTRY_SCOPE],
            "opentelemetry-instrumentation-axum"
        );
        assert!(
            !sighting
                .attributes
                .contains_key("deployment.environment.name")
        );

        let client = instance_sighting(&span(3, &[("host.name", "node-1")])).expect("sighting");
        assert!(!client.attributes.contains_key(INSTANCE_ENTRY_SCOPE));
    }

    #[test]
    fn an_instance_is_looked_at_once_a_minute_of_its_spans() {
        let looks = InstanceLooks::default();
        let span = span(
            2,
            &[
                ("service.instance.id", "i-1"),
                ("cloud.region", "eu-west-1"),
            ],
        );
        let fact_at = |micros: i64| {
            let mut fact = crate::app::apm::extract_apm_fact("org", &span).expect("fact");
            fact.event_time = crate::shared::time::TimestampMicros(micros);
            fact
        };

        let mut first = fact_at(0);
        looks.attach(&mut first, &span);
        let mut soon = fact_at(30_000_000);
        looks.attach(&mut soon, &span);
        let mut later = fact_at(61_000_000);
        looks.attach(&mut later, &span);

        assert_eq!(
            first
                .instance
                .map(|look| look.attributes["cloud.region"].clone()),
            Some("eu-west-1".into())
        );
        assert!(soon.instance.is_none());
        assert!(later.instance.is_some());
    }

    #[test]
    fn a_repository_link_keeps_no_credential() {
        assert_eq!(
            repository_link(
                "https://x-access-token:ghp_secret@github.com/acme/checkout.git?ref=main#readme"
            ),
            Some("https://github.com/acme/checkout".into())
        );
        assert_eq!(
            repository_link("git@github.com:acme/checkout.git"),
            Some("https://github.com/acme/checkout".into())
        );
        assert_eq!(repository_link("file:///srv/checkout"), None);
        assert_eq!(repository_link("not a link"), None);
    }
}
