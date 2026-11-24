// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What services run on: clusters, nodes, hosts, pods and containers, as the resource attributes
//! of their telemetry name them, and what runs on or is part of what. An entity is known by its
//! kind and a key within its organization; it is seen again whenever telemetry names it, and
//! forgotten once it has not been seen for a while.

mod sighting;

use std::collections::BTreeMap;

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
pub use sighting::{Sighting, sighting_of};

/// An entity not seen for this long is forgotten.
pub const ENTITY_KEPT_MICROS: i64 = 7 * 86_400_000_000;
/// The most entities a list answers.
pub const MAX_LISTED_ENTITIES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Cluster,
    Node,
    Host,
    Pod,
    Container,
}

impl EntityKind {
    pub const ALL: [Self; 5] = [
        Self::Cluster,
        Self::Node,
        Self::Host,
        Self::Pod,
        Self::Container,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::Node => "node",
            Self::Host => "host",
            Self::Pod => "pod",
            Self::Container => "container",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// One end of a link: an entity, or a service (`namespace/name/environment`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRef {
    /// An entity kind's name, or `service`.
    pub kind: String,
    pub key: String,
}

impl EntityRef {
    pub fn entity(kind: EntityKind, key: impl Into<String>) -> Self {
        Self {
            kind: kind.as_str().to_owned(),
            key: key.into(),
        }
    }

    pub fn service(key: impl Into<String>) -> Self {
        Self {
            kind: SERVICE.to_owned(),
            key: key.into(),
        }
    }
}

/// The kind a service's end of a link is named by.
pub const SERVICE: &str = "service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// A service on a pod or a host, a pod on a node.
    RunsOn,
    /// A container of a pod, a node of a cluster.
    PartOf,
}

impl Relation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunsOn => "runs_on",
            Self::PartOf => "part_of",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "runs_on" => Some(Self::RunsOn),
            "part_of" => Some(Self::PartOf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub kind: EntityKind,
    pub key: String,
    /// What a page calls it: the pod's, node's or host's own name.
    pub name: String,
    /// The resource attributes it was last named with.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityLink {
    pub from: EntityRef,
    pub relation: Relation,
    pub to: EntityRef,
    pub last_seen_at: TimestampMicros,
}

/// Which entities a list keeps.
#[derive(Debug, Clone, Default)]
pub struct EntityQuery {
    pub kind: Option<EntityKind>,
    /// Entities whose name or key holds this text.
    pub search: Option<String>,
    /// Entities seen since then.
    pub seen_since: Option<TimestampMicros>,
    pub limit: usize,
}

#[async_trait]
pub trait EntityRepository: Send + Sync {
    /// Keeps what a sweep saw: an entity seen again keeps its first sighting and takes the latest
    /// attributes; a link seen again takes the latest time.
    async fn record(&self, org_id: &Id, entities: &[Entity], links: &[EntityLink]) -> Result<()>;

    async fn list(&self, org_id: &Id, query: &EntityQuery) -> Result<Vec<Entity>>;

    async fn get(&self, org_id: &Id, kind: EntityKind, key: &str) -> Result<Option<Entity>>;

    /// The links from or to one end, the latest first.
    async fn links(&self, org_id: &Id, end: &EntityRef) -> Result<Vec<EntityLink>>;

    /// How many services run on each of these entities, directly.
    async fn service_counts(
        &self,
        org_id: &Id,
        ends: &[EntityRef],
    ) -> Result<BTreeMap<EntityRef, u64>>;

    /// Forgets entities and links not seen since `cutoff`, every organization's; how many went.
    async fn forget_before(&self, cutoff: TimestampMicros) -> Result<u64>;
}
