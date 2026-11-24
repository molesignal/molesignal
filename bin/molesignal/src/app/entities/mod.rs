// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What services run on, read for pages and the agent: entities with how many services run on
//! each, and one entity with what it runs on or is part of, what runs on or is part of it, and
//! the services on it, those on its pods and nodes too. A sweep ([`sweep`]) keeps them current.

mod sweep;

#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, sync::Arc};

use domain::{
    apm::ServiceIdentity,
    entity::{Entity, EntityKind, EntityLink, EntityQuery, EntityRef, EntityRepository, SERVICE},
    stream::StreamRepository,
};
use kernel::{Result, ids::Id};
use serde::Serialize;
pub use sweep::{Found, SWEEP_WINDOW_MICROS};

use super::{apm::ApmQueryService, query::QueryService};

/// How deep the services under an entity are looked for: a cluster's nodes, their pods, the
/// services on those.
const MOST_DEPTH: usize = 3;
/// The most services and neighbours one entity tells.
const MOST_TOLD: usize = 200;

/// Where entities are swept from and read.
#[derive(Clone)]
pub struct EntitySources {
    pub repository: Arc<dyn EntityRepository>,
    pub apm: Arc<ApmQueryService>,
    pub query: Arc<QueryService>,
    pub streams: Arc<dyn StreamRepository>,
}

/// An entity in a list, with how many services run on it directly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRow {
    #[serde(flatten)]
    pub entity: Entity,
    pub services: u64,
}

/// An entity a link leads to, named by the last part of its key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Neighbour {
    pub kind: String,
    pub key: String,
    pub name: String,
}

impl Neighbour {
    fn of(end: &EntityRef) -> Self {
        Self {
            kind: end.kind.clone(),
            key: end.key.clone(),
            name: end.key.rsplit('/').next().unwrap_or(&end.key).to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityDetail {
    pub entity: Entity,
    /// What it runs on or is part of.
    pub parents: Vec<Neighbour>,
    /// What runs on or is part of it, services aside.
    pub children: Vec<Neighbour>,
    /// The services on it, and on what runs on it.
    pub services: Vec<ServiceIdentity>,
}

/// A service's identity from the key its links are kept under: `namespace/name/environment`.
pub fn service_of(key: &str) -> Option<ServiceIdentity> {
    let mut parts = key.splitn(3, '/');
    let (namespace, name, environment) = (parts.next()?, parts.next()?, parts.next()?);

    Some(ServiceIdentity {
        namespace: namespace.to_owned(),
        name: name.to_owned(),
        environment: environment.to_owned(),
    })
}

/// Splits links around `end`: what it leads to, what leads to it, and the services on it.
pub fn around(
    end: &EntityRef,
    links: &[EntityLink],
) -> (Vec<Neighbour>, Vec<Neighbour>, Vec<String>) {
    let mut parents = BTreeSet::new();
    let mut children = BTreeSet::new();
    let mut services = BTreeSet::new();

    for link in links {
        if &link.from == end {
            parents.insert(Neighbour::of(&link.to));
        } else if &link.to == end && link.from.kind == SERVICE {
            services.insert(link.from.key.clone());
        } else if &link.to == end {
            children.insert(Neighbour::of(&link.from));
        }
    }

    (
        parents.into_iter().collect(),
        children.into_iter().take(MOST_TOLD).collect(),
        services.into_iter().collect(),
    )
}

impl EntitySources {
    pub async fn list(&self, org_id: &Id, query: &EntityQuery) -> Result<Vec<EntityRow>> {
        let entities = self.repository.list(org_id, query).await?;
        let ends = entities
            .iter()
            .map(|entity| EntityRef::entity(entity.kind, entity.key.clone()))
            .collect::<Vec<_>>();
        let counts = self.repository.service_counts(org_id, &ends).await?;

        Ok(entities
            .into_iter()
            .zip(ends)
            .map(|(entity, end)| EntityRow {
                services: counts.get(&end).copied().unwrap_or_default(),
                entity,
            })
            .collect())
    }

    /// One entity and what is around it; `None` when it is not known.
    pub async fn detail(
        &self,
        org_id: &Id,
        kind: EntityKind,
        key: &str,
    ) -> Result<Option<EntityDetail>> {
        let Some(entity) = self.repository.get(org_id, kind, key).await? else {
            return Ok(None);
        };

        let end = EntityRef::entity(kind, key);
        let (parents, children, direct) = around(&end, &self.repository.links(org_id, &end).await?);
        let mut services = direct.into_iter().collect::<BTreeSet<_>>();

        // What runs on its children: a node's pods, a cluster's nodes and their pods.
        let mut frontier = children
            .iter()
            .map(|child| EntityRef {
                kind: child.kind.clone(),
                key: child.key.clone(),
            })
            .collect::<Vec<_>>();

        for _ in 1..MOST_DEPTH {
            let mut next = Vec::new();

            for child in frontier.iter().take(MOST_TOLD) {
                let (_, grandchildren, on_child) =
                    around(child, &self.repository.links(org_id, child).await?);
                services.extend(on_child);
                next.extend(grandchildren.into_iter().map(|grandchild| EntityRef {
                    kind: grandchild.kind,
                    key: grandchild.key,
                }));
            }

            if next.is_empty() || services.len() >= MOST_TOLD {
                break;
            }

            frontier = next;
        }

        Ok(Some(EntityDetail {
            entity,
            parents,
            children,
            services: services
                .iter()
                .take(MOST_TOLD)
                .filter_map(|key| service_of(key))
                .collect(),
        }))
    }
}
