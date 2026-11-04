// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a service runs as within the range, read from its instances: how many
//! there are and what they say about where and how they run. A service runs as
//! several instances at once, of one version or of several during a rollout, so
//! each figure is a list ranked by how many of the instances share it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::domain::apm::{INSTANCE_ENTRY_SCOPE, InstanceObservation, TransactionKind};

/// The most values a figure lists.
const MAX_VALUES: usize = 5;

/// Words in an instrumentation scope's name that name the framework it
/// instruments, as people write the framework's name.
const FRAMEWORKS: &[(&str, &str)] = &[
    ("axum", "Axum"),
    ("actix", "Actix Web"),
    ("tonic", "tonic"),
    ("warp", "warp"),
    ("rocket", "Rocket"),
    ("poem", "Poem"),
    ("spring", "Spring"),
    ("tomcat", "Tomcat"),
    ("jetty", "Jetty"),
    ("netty", "Netty"),
    ("vertx", "Vert.x"),
    ("quarkus", "Quarkus"),
    ("express", "Express"),
    ("fastify", "Fastify"),
    ("koa", "Koa"),
    ("nestjs", "NestJS"),
    ("django", "Django"),
    ("flask", "Flask"),
    ("fastapi", "FastAPI"),
    ("starlette", "Starlette"),
    ("tornado", "Tornado"),
    ("aiohttp", "aiohttp"),
    ("gin", "Gin"),
    ("echo", "Echo"),
    ("fiber", "Fiber"),
    ("otelhttp", "net/http"),
    ("aspnetcore", "ASP.NET Core"),
    ("rails", "Rails"),
    ("sinatra", "Sinatra"),
    ("laravel", "Laravel"),
    ("symfony", "Symfony"),
    ("grpc", "gRPC"),
];

/// A value some instances share, and how many.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileValue {
    pub value: String,
    pub instances: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceProfile {
    /// Instances seen within the range, of the version in view when one is.
    pub instance_count: u32,
    /// Hosts the instances ran on, where they said.
    pub host_count: u32,
    pub versions: Vec<ProfileValue>,
    pub languages: Vec<ProfileValue>,
    /// `process.runtime.name` and its version: `rustc 1.86.0`, `OpenJDK Runtime Environment 21`.
    pub runtimes: Vec<ProfileValue>,
    pub frameworks: Vec<ProfileValue>,
    pub regions: Vec<ProfileValue>,
    pub clusters: Vec<ProfileValue>,
    pub k8s_namespaces: Vec<ProfileValue>,
    /// Deployments and stateful sets.
    pub workloads: Vec<ProfileValue>,
    pub repositories: Vec<ProfileValue>,
    pub commits: Vec<ProfileValue>,
    /// What kinds of entry the service serves, as seen ever.
    pub entry_kinds: Vec<TransactionKind>,
}

/// The profile of a service from its instances, those of `version` alone when one is in view.
pub fn service_profile(
    instances: &[&InstanceObservation],
    version: Option<&str>,
    entry_kinds: &[TransactionKind],
) -> ServiceProfile {
    let instances = instances
        .iter()
        .filter(|instance| version.is_none_or(|wanted| instance.version.as_deref() == Some(wanted)))
        .collect::<Vec<_>>();
    let rank = |read: &dyn Fn(&InstanceObservation) -> Option<String>| {
        ranked(instances.iter().filter_map(|instance| read(instance)))
    };
    let attribute = |key: &'static str| {
        move |instance: &InstanceObservation| instance.attributes.get(key).cloned()
    };
    let hosts = instances
        .iter()
        .filter_map(|instance| instance.attributes.get("host.name"))
        .collect::<BTreeSet<_>>();

    ServiceProfile {
        instance_count: count(instances.len()),
        host_count: count(hosts.len()),
        versions: rank(&|instance| instance.version.clone()),
        languages: rank(&attribute("telemetry.sdk.language")),
        runtimes: rank(&|instance| {
            let name = instance.attributes.get("process.runtime.name")?;

            Some(match instance.attributes.get("process.runtime.version") {
                Some(version) => format!("{name} {version}"),
                None => name.clone(),
            })
        }),
        frameworks: rank(&|instance| framework(instance.attributes.get(INSTANCE_ENTRY_SCOPE)?)),
        regions: rank(&attribute("cloud.region")),
        clusters: rank(&attribute("k8s.cluster.name")),
        k8s_namespaces: rank(&attribute("k8s.namespace.name")),
        workloads: rank(&|instance| {
            instance
                .attributes
                .get("k8s.deployment.name")
                .or_else(|| instance.attributes.get("k8s.statefulset.name"))
                .cloned()
        }),
        repositories: rank(&|instance| {
            instance
                .attributes
                .get("vcs.repository.url.full")
                .or_else(|| instance.attributes.get("git.repository_url"))
                .cloned()
        }),
        commits: rank(&|instance| {
            instance
                .attributes
                .get("vcs.ref.head.revision")
                .or_else(|| instance.attributes.get("git.commit.sha"))
                .cloned()
        }),
        entry_kinds: entry_kinds.to_vec(),
    }
}

/// The instances of each service, by its identity.
pub fn instances_by_service(
    instances: &[InstanceObservation],
) -> HashMap<&crate::domain::apm::ServiceIdentity, Vec<&InstanceObservation>> {
    let mut grouped = HashMap::<_, Vec<_>>::new();

    for instance in instances {
        grouped.entry(&instance.service).or_default().push(instance);
    }

    grouped
}

/// The framework an instrumentation scope instruments, when one of its words names one.
fn framework(scope: &str) -> Option<String> {
    scope
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .find_map(|word| {
            FRAMEWORKS
                .iter()
                .find(|(needle, _)| *needle == word)
                .map(|(_, name)| (*name).to_owned())
        })
}

fn ranked(values: impl Iterator<Item = String>) -> Vec<ProfileValue> {
    let mut counts = BTreeMap::<String, u32>::new();

    for value in values {
        *counts.entry(value).or_default() += 1;
    }

    let mut ranked = counts
        .into_iter()
        .map(|(value, instances)| ProfileValue { value, instances })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .instances
            .cmp(&left.instances)
            .then_with(|| left.value.cmp(&right.value))
    });
    ranked.truncate(MAX_VALUES);
    ranked
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::apm::ServiceIdentity,
        shared::{ids::Id, time::TimestampMicros},
    };

    fn instance(id: &str, version: &str, attributes: &[(&str, &str)]) -> InstanceObservation {
        InstanceObservation {
            org_id: Id::from_string("org"),
            service: ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None),
            instance_id: id.into(),
            version: Some(version.into()),
            first_seen_at: TimestampMicros(0),
            last_seen_at: TimestampMicros(1),
            attributes: attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn a_rollout_lists_both_versions_and_the_version_in_view_narrows_everything() {
        let instances = [
            instance(
                "a",
                "1.8.2",
                &[
                    ("cloud.region", "ap-southeast-1"),
                    ("host.name", "n1"),
                    (INSTANCE_ENTRY_SCOPE, "opentelemetry-instrumentation-axum"),
                ],
            ),
            instance(
                "b",
                "1.8.2",
                &[("cloud.region", "ap-southeast-1"), ("host.name", "n2")],
            ),
            instance(
                "c",
                "1.8.1",
                &[
                    ("cloud.region", "eu-west-1"),
                    ("host.name", "n2"),
                    ("process.runtime.name", "rustc"),
                    ("process.runtime.version", "1.86.0"),
                ],
            ),
        ];
        let all = instances.iter().collect::<Vec<_>>();

        let everything = service_profile(&all, None, &[TransactionKind::Http]);

        assert_eq!(everything.instance_count, 3);
        assert_eq!(everything.host_count, 2);
        assert_eq!(
            everything
                .versions
                .iter()
                .map(|value| (value.value.as_str(), value.instances))
                .collect::<Vec<_>>(),
            [("1.8.2", 2), ("1.8.1", 1)]
        );
        assert_eq!(everything.regions[0].value, "ap-southeast-1");
        assert_eq!(everything.frameworks[0].value, "Axum");
        assert_eq!(everything.runtimes[0].value, "rustc 1.86.0");
        assert_eq!(everything.entry_kinds, [TransactionKind::Http]);

        let canary = service_profile(&all, Some("1.8.1"), &[]);

        assert_eq!(canary.instance_count, 1);
        assert_eq!(canary.regions[0].value, "eu-west-1");
        assert!(canary.frameworks.is_empty());
    }

    #[test]
    fn a_framework_is_named_by_a_whole_word_of_the_scope() {
        assert_eq!(
            framework("io.opentelemetry.spring-webmvc-6.0").as_deref(),
            Some("Spring")
        );
        assert_eq!(
            framework("@opentelemetry/instrumentation-express").as_deref(),
            Some("Express")
        );
        assert_eq!(
            framework(
                "go.opentelemetry.io/contrib/instrumentation/github.com/gin-gonic/gin/otelgin"
            )
            .as_deref(),
            Some("Gin")
        );
        assert_eq!(
            framework("Microsoft.AspNetCore").as_deref(),
            Some("ASP.NET Core")
        );
        assert_eq!(framework("molesignal::router"), None);
        assert_eq!(framework("engine-origin"), None);
    }
}
