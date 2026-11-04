// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The instances of a service: each process that reports for it, with what its
//! resource attributes say about where and how it runs. A service runs as many
//! instances at once, of one version or of several during a rollout, so what a
//! page says of "the service" is read from the instances in view.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ServiceIdentity;
use crate::shared::{ids::Id, time::TimestampMicros};

/// The resource attributes an instance is described by, in the order pages
/// read them. Each value is the one last reported, bounded in length.
pub const INSTANCE_ATTRIBUTES: &[&str] = &[
    "host.name",
    "k8s.cluster.name",
    "k8s.namespace.name",
    "k8s.deployment.name",
    "k8s.statefulset.name",
    "k8s.pod.name",
    "k8s.node.name",
    "container.name",
    "container.id",
    "cloud.provider",
    "cloud.region",
    "cloud.availability_zone",
    "process.runtime.name",
    "process.runtime.version",
    "telemetry.sdk.language",
    "telemetry.sdk.name",
    "telemetry.sdk.version",
    "vcs.repository.url.full",
    "git.repository_url",
    "vcs.ref.head.revision",
    "git.commit.sha",
];

/// The instrumentation scope of the instance's entry spans, kept beside its
/// attributes: the web framework or RPC library that served the request.
pub const INSTANCE_ENTRY_SCOPE: &str = "otel.scope.name";

/// One look at an instance, taken from a span now and then rather than from
/// every span: which instance it is and what its resource says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSighting {
    /// `service.instance.id`, else the pod, else the host it runs on.
    pub instance_id: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// An instance of a service as recorded: when it was first and last seen, the
/// version it last ran, and its resource attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceObservation {
    pub org_id: Id,
    pub service: ServiceIdentity,
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

impl InstanceObservation {
    /// Takes in a later look at the instance: its times widen, the version is
    /// the one last reported, and attributes it reports again replace the kept ones.
    pub fn absorb(
        &mut self,
        sighting: &InstanceSighting,
        version: Option<&str>,
        seen_at: TimestampMicros,
    ) {
        self.first_seen_at = self.first_seen_at.min(seen_at);

        if seen_at >= self.last_seen_at {
            self.last_seen_at = seen_at;

            if let Some(version) = version {
                self.version = Some(version.to_owned());
            }
        }

        for (key, value) in &sighting.attributes {
            self.attributes.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_look_moves_the_version_and_replaces_reported_attributes() {
        let mut observation = InstanceObservation {
            org_id: Id::from_string("org"),
            service: ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None),
            instance_id: "pod-a".into(),
            version: Some("1.8.1".into()),
            first_seen_at: TimestampMicros(100),
            last_seen_at: TimestampMicros(200),
            attributes: BTreeMap::from([
                ("cloud.region".into(), "ap-southeast-1".into()),
                ("host.name".into(), "node-1".into()),
            ]),
        };
        let sighting = InstanceSighting {
            instance_id: "pod-a".into(),
            attributes: BTreeMap::from([("host.name".into(), "node-2".into())]),
        };

        observation.absorb(&sighting, Some("1.8.2"), TimestampMicros(300));
        observation.absorb(&sighting, Some("1.8.0"), TimestampMicros(50));

        assert_eq!(observation.version.as_deref(), Some("1.8.2"));
        assert_eq!(
            (observation.first_seen_at, observation.last_seen_at),
            (TimestampMicros(50), TimestampMicros(300))
        );
        assert_eq!(observation.attributes["host.name"], "node-2");
        assert_eq!(observation.attributes["cloud.region"], "ap-southeast-1");
    }
}
