// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM streams of an organization, created with the schema the contract declares instead of
//! the schema the first batch would suggest.

use std::{collections::HashSet, sync::Arc, time::Duration};

use moka::sync::Cache;

use crate::{
    domain::{
        rum::event::{RumStream, stream_schema},
        stream::{Schema, StreamDefinition, StreamRepository, StreamType},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// How long an organization's streams are taken to be in place. A stream deleted by hand is made
/// again within this time; until then a write to it is a plain write to a missing stream.
const PROVISIONED_FOR: Duration = Duration::from_secs(60);

pub struct StreamProvisioner {
    streams: Arc<dyn StreamRepository>,
    provisioned: Cache<Id, ()>,
}

impl StreamProvisioner {
    pub fn new(streams: Arc<dyn StreamRepository>) -> Self {
        Self {
            streams,
            provisioned: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(PROVISIONED_FOR)
                .build(),
        }
    }

    /// Creates the RUM streams the organization lacks and adds to the others the columns the
    /// contract declares since they were made.
    pub async fn provision(&self, org_id: &Id) -> Result<()> {
        if self.provisioned.contains_key(org_id) {
            return Ok(());
        }

        for stream in RumStream::ALL {
            self.provision_stream(org_id, stream).await?;
        }

        self.provisioned.insert(org_id.clone(), ());
        Ok(())
    }

    async fn provision_stream(&self, org_id: &Id, stream: RumStream) -> Result<()> {
        match self
            .streams
            .get(org_id, stream.name(), StreamType::LOGS)
            .await
        {
            Ok(existing) => {
                if let Some(schema) = with_missing_columns(&existing.schema, stream) {
                    self.streams
                        .update_schema_internal(&existing.id, schema)
                        .await?;
                }

                Ok(())
            }
            Err(Error::NotFound(_)) => self.create(org_id, stream).await,
            Err(error) => Err(error),
        }
    }

    async fn create(&self, org_id: &Id, stream: RumStream) -> Result<()> {
        let now = TimestampMicros::now();

        let definition = StreamDefinition {
            id: Id::new(),
            org_id: org_id.clone(),
            name: stream.name().to_string(),
            stream_type: StreamType::LOGS,
            schema: stream_schema(stream),
            retention: None,
            created_at: now,
            updated_at: now,
        };

        match self.streams.create(definition).await {
            // Another request made it first; it was made from the same table.
            Ok(_) | Err(Error::Conflict(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// The schema of an existing stream with the columns it lacks added, or `None` when it has them
/// all.
fn with_missing_columns(existing: &Schema, stream: RumStream) -> Option<Schema> {
    let present = existing
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<HashSet<_>>();

    let missing = stream_schema(stream)
        .fields
        .into_iter()
        .filter(|field| !present.contains(field.name.as_str()))
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return None;
    }

    let mut schema = existing.clone();

    schema.fields.extend(missing);
    Some(schema)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use parking_lot::Mutex;

    use super::*;
    use crate::domain::stream::{FieldDef, FieldType};

    #[derive(Default)]
    struct MemStreams {
        streams: Mutex<Vec<StreamDefinition>>,
        creates: AtomicUsize,
        updates: AtomicUsize,
        conflict_on_create: bool,
    }

    #[async_trait]
    impl StreamRepository for MemStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            self.creates.fetch_add(1, Ordering::Relaxed);

            if self.conflict_on_create {
                return Err(Error::conflict("exists"));
            }

            self.streams.lock().push(def.clone());
            Ok(def)
        }

        async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
            self.updates.fetch_add(1, Ordering::Relaxed);

            let mut streams = self.streams.lock();
            let stream = streams
                .iter_mut()
                .find(|stream| &stream.id == id)
                .ok_or_else(|| Error::not_found("stream"))?;

            stream.schema = schema;
            Ok(())
        }

        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            _stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            self.streams
                .lock()
                .iter()
                .find(|stream| &stream.org_id == org_id && stream.name == name)
                .cloned()
                .ok_or_else(|| Error::not_found("stream"))
        }

        async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(self
                .streams
                .lock()
                .iter()
                .filter(|stream| &stream.org_id == org_id)
                .cloned()
                .collect())
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn org() -> Id {
        Id::from_string("org-1")
    }

    #[tokio::test]
    async fn every_stream_is_created_with_the_declared_schema() {
        let repository = Arc::new(MemStreams::default());
        let provisioner = StreamProvisioner::new(repository.clone());

        provisioner.provision(&org()).await.unwrap();

        let streams = repository.streams.lock();

        assert_eq!(streams.len(), RumStream::ALL.len());

        for stream in RumStream::ALL {
            let created = streams.iter().find(|s| s.name == stream.name()).unwrap();

            assert_eq!(created.stream_type, StreamType::LOGS);
            assert_eq!(
                created.schema.fields.len(),
                stream_schema(stream).fields.len()
            );
        }
    }

    #[tokio::test]
    async fn a_second_call_within_the_minute_asks_nothing() {
        let repository = Arc::new(MemStreams::default());
        let provisioner = StreamProvisioner::new(repository.clone());

        provisioner.provision(&org()).await.unwrap();
        provisioner.provision(&org()).await.unwrap();

        assert_eq!(
            repository.creates.load(Ordering::Relaxed),
            RumStream::ALL.len()
        );
    }

    #[tokio::test]
    async fn another_request_creating_a_stream_first_is_not_an_error() {
        let repository = Arc::new(MemStreams {
            conflict_on_create: true,
            ..MemStreams::default()
        });

        StreamProvisioner::new(repository)
            .provision(&org())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_stream_from_an_older_version_gets_the_missing_columns() {
        let repository = Arc::new(MemStreams::default());
        let old = Schema {
            fields: stream_schema(RumStream::Actions)
                .fields
                .into_iter()
                .filter(|field| field.name != "action.type")
                .collect(),
        };

        repository.streams.lock().push(StreamDefinition {
            id: Id::from_string("actions"),
            org_id: org(),
            name: RumStream::Actions.name().into(),
            stream_type: StreamType::LOGS,
            schema: old,
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        });

        StreamProvisioner::new(repository.clone())
            .provision(&org())
            .await
            .unwrap();

        assert_eq!(repository.updates.load(Ordering::Relaxed), 1);

        let streams = repository.streams.lock();
        let actions = streams.iter().find(|s| s.name == "rum_actions").unwrap();
        let column = actions
            .schema
            .fields
            .iter()
            .find(|field| field.name == "action.type")
            .expect("the column is added");

        assert_eq!(column.data_type, FieldType::Utf8);
    }

    #[test]
    fn a_complete_schema_needs_nothing() {
        let schema = stream_schema(RumStream::Views);

        assert!(with_missing_columns(&schema, RumStream::Views).is_none());
        assert!(
            with_missing_columns(
                &Schema {
                    fields: Vec::<FieldDef>::new()
                },
                RumStream::Views
            )
            .is_some()
        );
    }
}
