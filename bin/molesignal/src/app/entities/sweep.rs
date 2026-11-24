// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One organization's sweep: the instances APM saw lately, each with the service that ran on it,
//! and the resource fields of its metrics and log streams, read as the entities they name.

use std::collections::{BTreeMap, HashMap};

use domain::{
    entity::{Entity, EntityKind, EntityLink, EntityRef, Sighting, sighting_of},
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamDefinition, StreamType},
};
use kernel::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

use super::EntitySources;
use crate::app::{
    apm::ApmQueryRequest,
    query::statement::{first_field, ident},
};

/// How far back a sweep reads.
pub const SWEEP_WINDOW_MICROS: i64 = 15 * 60 * 1_000_000;
/// The resource fields that name where telemetry comes from.
const PLACE_FIELDS: [&str; 10] = [
    "host.name",
    "k8s.cluster.name",
    "k8s.node.name",
    "k8s.namespace.name",
    "k8s.pod.name",
    "k8s.deployment.name",
    "container.id",
    "container.name",
    "cloud.provider",
    "cloud.region",
];
/// The fields at least one of which a stream needs to be read.
const NAMING_FIELDS: [&str; 3] = ["host.name", "k8s.node.name", "k8s.pod.name"];
const MOST_STREAMS: usize = 4;
const MOST_ROWS: usize = 2_000;

/// What one sweep found, each entity and link once.
#[derive(Debug, Default)]
pub struct Found {
    entities: HashMap<(EntityKind, String), Entity>,
    links: HashMap<(EntityRef, &'static str, EntityRef), EntityLink>,
}

impl Found {
    /// Takes in a sighting: an entity seen twice keeps its earliest and latest times and the
    /// attributes of its latest sighting.
    pub fn absorb(&mut self, sighting: Sighting) {
        for entity in sighting.entities {
            let key = (entity.kind, entity.key.clone());

            match self.entities.get_mut(&key) {
                Some(kept) => {
                    kept.first_seen_at = kept.first_seen_at.min(entity.first_seen_at);

                    if entity.last_seen_at >= kept.last_seen_at {
                        kept.last_seen_at = entity.last_seen_at;
                        kept.attributes = entity.attributes;
                        kept.name = entity.name;
                    }
                }
                None => {
                    self.entities.insert(key, entity);
                }
            }
        }

        for link in sighting.links {
            let key = (link.from.clone(), link.relation.as_str(), link.to.clone());
            let kept = self.links.entry(key).or_insert_with(|| link.clone());
            kept.last_seen_at = kept.last_seen_at.max(link.last_seen_at);
        }
    }

    pub fn into_parts(self) -> (Vec<Entity>, Vec<EntityLink>) {
        (
            self.entities.into_values().collect(),
            self.links.into_values().collect(),
        )
    }
}

impl EntitySources {
    /// Sweeps one organization and keeps what it found; how many entities that was.
    pub async fn sweep_org(&self, org_id: &Id, now: TimestampMicros) -> Result<usize> {
        let window = TimeRange::new(TimestampMicros(now.0 - SWEEP_WINDOW_MICROS), now);
        let mut found = Found::default();

        let context = self.apm.context(
            org_id.clone(),
            ApmQueryRequest {
                from: window.start.0,
                to: window.end.0,
                ..ApmQueryRequest::default()
            },
            &["name"],
            "name",
        )?;

        for instance in self.apm.inventory(&context).await?.instances {
            let service = &instance.service;
            let key = format!(
                "{}/{}/{}",
                service.namespace, service.name, service.environment
            );
            found.absorb(sighting_of(
                &instance.attributes,
                Some(&key),
                instance.last_seen_at,
            ));
        }

        let streams = self.streams.list(org_id).await?;

        for stream_type in [StreamType::METRICS, StreamType::LOGS] {
            for (stream, fields) in readable(&streams, stream_type) {
                match self
                    .rows(org_id, stream, stream_type, &fields, window)
                    .await
                {
                    Ok(rows) => {
                        for attributes in rows {
                            found.absorb(sighting_of(&attributes, None, now));
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%error, stream, "a stream's resource fields could not be read")
                    }
                }
            }
        }

        let (entities, links) = found.into_parts();
        let count = entities.len();

        self.repository.record(org_id, &entities, &links).await?;
        Ok(count)
    }

    /// The distinct combinations of a stream's resource fields over the window.
    async fn rows(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        fields: &[&'static str],
        window: TimeRange,
    ) -> Result<Vec<BTreeMap<String, String>>> {
        let columns = fields
            .iter()
            .map(|field| format!("\"{}\"", ident(field)))
            .collect::<Vec<_>>()
            .join(", ");
        let statement = format!(
            "SELECT DISTINCT {columns} FROM \"{}\" LIMIT {MOST_ROWS}",
            ident(stream)
        );

        let result = self
            .query
            .run(QueryRequest {
                org_id: org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range: window,
                stream: Some(StreamHint {
                    name: stream.to_owned(),
                    stream_type,
                }),
                limit: Some(MOST_ROWS),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await?;

        Ok(result
            .rows
            .iter()
            .map(|row| {
                fields
                    .iter()
                    .zip(row)
                    .filter_map(|(field, value)| {
                        value
                            .as_str()
                            .filter(|value| !value.trim().is_empty())
                            .map(|value| ((*field).to_owned(), value.to_owned()))
                    })
                    .collect()
            })
            .collect())
    }
}

/// The streams of a type that name where their telemetry comes from, with the fields they have,
/// `default` first and a few at most.
fn readable(
    streams: &[StreamDefinition],
    stream_type: StreamType,
) -> Vec<(&str, Vec<&'static str>)> {
    let mut chosen = streams
        .iter()
        .filter(|stream| {
            stream.stream_type == stream_type && stream.name != MOLESIGNAL_SYSTEM_STREAM
        })
        .filter(|stream| first_field(stream, &NAMING_FIELDS).is_some())
        .map(|stream| {
            let fields = PLACE_FIELDS
                .iter()
                .copied()
                .filter(|field| first_field(stream, &[*field]).is_some())
                .collect::<Vec<_>>();
            (stream.name.as_str(), fields)
        })
        .collect::<Vec<_>>();

    chosen.sort_by_key(|(name, _)| (*name != "default", *name));
    chosen.truncate(MOST_STREAMS);
    chosen
}
