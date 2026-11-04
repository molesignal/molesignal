// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The filters of a sealed partition, all in one object beside its manifest: one filter per
//! segment for every exact field, built from the segment's Tantivy dictionary, and the filters of
//! `bloom` and `ngram` fields copied from each segment's own filter artifact (see
//! [`crate::infra::storage::filters`]).
//!
//! A query for `trace_id = '…'` over many sealed partitions reads one object per partition and
//! drops the segments whose filter says the value is absent, instead of opening the index or the
//! filters of every segment. A sealed partition does not change, so neither do its filters; the
//! next generation of the manifest gets its own object and reuses the filters of segments it keeps.

use std::{collections::HashMap, sync::Arc};

use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use crate::{
    domain::{
        storage::{ArtifactRole, ArtifactState, DataSegment, ObjectKey, type_id},
        stream::{FieldType, StreamDefinition, StreamIndexType},
    },
    infra::{
        search::tantivy_index::{TantivyArchiveOpener, key_value_field},
        storage::filters::{Bloom, FilterKind, Filters},
    },
    shared::{Error, Result},
};

/// The key of the filters of a manifest: beside it, so it shares its prefix and is collected with
/// it once the manifest is superseded.
pub(crate) fn bloom_key(manifest_key: &ObjectKey) -> String {
    format!("{}.bloom", manifest_key.as_str())
}

/// The filters of `segments`: the ones `previous` holds are kept, the rest are read from each
/// segment's Tantivy archive and filter artifact. A segment without either gets none. A segment in
/// `rebuilt` was indexed again under the stream's current settings: its exact fields come from its
/// new archive and the rest from the filters it was rebuilt with, never from what came before.
pub(crate) async fn build(
    store: Arc<dyn ObjectStore>,
    stream: &StreamDefinition,
    segments: &[DataSegment],
    previous: Option<&Filters>,
    rebuilt: &HashMap<String, Filters>,
) -> Filters {
    // 归档里按整值建索引的字段：精确字段，与 JSON 字段的键值索引。
    let fields = stream
        .schema
        .fields
        .iter()
        .filter(|field| !field.encrypted)
        .filter_map(
            |field| match (field.data_type, field.effective_index_type()) {
                (FieldType::Utf8 | FieldType::Json, StreamIndexType::Exact) => {
                    Some(field.name.clone())
                }
                (FieldType::Json, StreamIndexType::KeyValue) => Some(key_value_field(&field.name)),
                _ => None,
            },
        )
        .collect::<Vec<_>>();

    let mut filters = Filters::default();

    for segment in segments {
        let id = segment.id.as_str();

        let kept = previous
            .map(|previous| previous.of_segment(id))
            .filter(|kept| !kept.is_empty());

        let built = match (rebuilt.get(id), kept) {
            (Some(filters), _) => {
                let mut built = archive_filters(store.clone(), segment, &fields).await;
                built.extend(filters.of_segment(id));
                built
            }
            (None, Some(kept)) => kept,
            (None, None) => segment_filters(store.clone(), segment, &fields).await,
        };

        for (kind, field, bloom) in built {
            filters.insert(kind, &field, id, bloom);
        }
    }

    filters
}

/// The filters of one segment: its exact `fields` from its Tantivy archive, everything its filter
/// artifact holds. Either that cannot be read is only logged.
async fn segment_filters(
    store: Arc<dyn ObjectStore>,
    segment: &DataSegment,
    fields: &[String],
) -> Vec<(FilterKind, String, Bloom)> {
    let id = segment.id.as_str();
    let mut built = archive_filters(store.clone(), segment, fields).await;

    match artifact_filters(store, segment).await {
        Ok(Some(filters)) => built.extend(filters.of_segment(id)),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(segment_id = id, %error, "segment filter artifact skipped");
        }
    }

    built
}

/// The filters of a segment's exact `fields`, from its Tantivy archive. That it cannot be read is
/// only logged.
async fn archive_filters(
    store: Arc<dyn ObjectStore>,
    segment: &DataSegment,
    fields: &[String],
) -> Vec<(FilterKind, String, Bloom)> {
    if fields.is_empty() {
        return Vec::new();
    }

    match archive_blooms(store, segment, fields).await {
        Ok(blooms) => blooms
            .into_iter()
            .map(|(field, bloom)| (FilterKind::Value, field, bloom))
            .collect(),
        Err(error) => {
            tracing::warn!(segment_id = %segment.id, %error, "segment bloom filters skipped");
            Vec::new()
        }
    }
}

/// What the segment's own filter artifact holds, when it has a ready one.
async fn artifact_filters(
    store: Arc<dyn ObjectStore>,
    segment: &DataSegment,
) -> Result<Option<Filters>> {
    let Some(artifact) = segment.auxiliaries.iter().find(|artifact| {
        artifact.role == ArtifactRole::Index
            && artifact.state == ArtifactState::Ready
            && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_BLOOM
    }) else {
        return Ok(None);
    };

    let bytes = store
        .get(&Path::from(artifact.object.key.as_str()))
        .await
        .map_err(|error| Error::internal(format!("read filter artifact: {error}")))?
        .bytes()
        .await
        .map_err(|error| Error::internal(format!("collect filter artifact: {error}")))?;

    Filters::decode(&bytes).map(Some)
}

/// One filter per field from the term dictionaries of the segment's Tantivy archive.
async fn archive_blooms(
    store: Arc<dyn ObjectStore>,
    segment: &DataSegment,
    fields: &[String],
) -> Result<Vec<(String, Bloom)>> {
    let Some(archive) = segment.auxiliaries.iter().find(|artifact| {
        artifact.role == ArtifactRole::Index
            && artifact.state == ArtifactState::Ready
            && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
    }) else {
        return Ok(Vec::new());
    };

    let handle = TantivyArchiveOpener::open_from_object_store(
        store,
        Path::from(archive.object.key.as_str()),
        archive.object.size_bytes,
    )
    .await
    .map_err(|error| Error::internal(format!("open index for blooms: {error}")))?;

    let fields = fields.to_vec();

    tokio::task::spawn_blocking(move || {
        let mut built = Vec::new();

        for field in fields {
            let Some(values) = handle
                .term_count(&field)
                .map_err(|error| Error::internal(format!("count terms: {error}")))?
            else {
                continue;
            };

            let Some(mut bloom) = Bloom::for_values(values) else {
                continue;
            };

            handle
                .visit_terms(&field, &mut |value| bloom.insert(value))
                .map_err(|error| Error::internal(format!("read terms: {error}")))?;

            built.push((field, bloom));
        }

        Ok(built)
    })
    .await
    .map_err(|error| Error::internal(format!("bloom build join: {error}")))?
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{RecordBatch, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
    };
    use object_store::memory::InMemory;

    use super::*;
    use crate::{
        domain::{
            storage::{
                DatasetState, IndexPolicy, IndexTypeId, PartitionPolicy, PhysicalDataset,
                PhysicalDatasetId, StoragePolicy, primary_dataset_type,
            },
            stream::{FieldDef, Schema, StreamType},
        },
        infra::storage::parquet::writer::ParquetWriter,
        shared::{ids::Id, time::TimestampMicros},
    };

    fn stream() -> StreamDefinition {
        let field = |name: &str, exact: bool| FieldDef {
            name: name.into(),
            data_type: FieldType::Utf8,
            nullable: false,
            index_type: None,
            indexed: true,
            encrypted: false,
            exact,
        };

        let mut request_id = field("request_id", false);
        request_id.configure_index(true, StreamIndexType::Bloom);

        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![field("trace_id", true), field("message", false), request_id],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn dataset(stream: &StreamDefinition) -> PhysicalDataset {
        PhysicalDataset {
            id: PhysicalDatasetId::generate(),
            organization_id: stream.org_id.clone(),
            logical_stream_id: stream.id.clone(),
            dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
            dataset_type_version: 1,
            partition_policy: PartitionPolicy::default(),
            storage_policy: StoragePolicy::default(),
            index_policy: IndexPolicy {
                indexers: vec![IndexTypeId::builtin(type_id::builtin::INDEX_TANTIVY)],
            },
            catalog_version: 0,
            state: DatasetState::Active,
            created_at_micros: 0,
            updated_at_micros: 0,
        }
    }

    fn batch(trace_ids: &[&str]) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("message", DataType::Utf8, false),
            Field::new("request_id", DataType::Utf8, false),
        ]));

        let times =
            TimestampMicrosecondArray::from((1..=trace_ids.len() as i64).collect::<Vec<_>>())
                .with_timezone("UTC");

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(times),
                Arc::new(StringArray::from(trace_ids.to_vec())),
                Arc::new(StringArray::from(vec!["disk full"; trace_ids.len()])),
                Arc::new(StringArray::from(
                    trace_ids
                        .iter()
                        .map(|trace_id| format!("request-{trace_id}"))
                        .collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap()
    }

    /// 每个段的整值字段从它的索引词典建出过滤器，`bloom` 字段的过滤器从段自己的过滤器 Artifact
    /// 拷来；下一代清单沿用已有段的过滤器，不再读索引；什么都读不到的段没有过滤器。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn filters_come_from_the_archives_and_are_kept_across_generations() {
        let stream = stream();
        let dataset = dataset(&stream);
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = ParquetWriter::new(store.clone());
        let mut segments = Vec::new();

        for trace_ids in [&["t-1", "t-2"][..], &["t-3"]] {
            segments.extend(
                writer
                    .write_compaction_catalog(&stream, &dataset, batch(trace_ids))
                    .await
                    .unwrap(),
            );
        }

        let blooms = build(store, &stream, &segments, None, &HashMap::new()).await;
        let (a, b) = (segments[0].id.as_str(), segments[1].id.as_str());

        assert_eq!(blooms.may_contain("trace_id", a, "t-1"), Some(true));
        assert_eq!(blooms.may_contain("trace_id", a, "t-2"), Some(true));
        assert_eq!(blooms.may_contain("trace_id", b, "t-3"), Some(true));
        assert_eq!(blooms.may_contain("trace_id", a, "t-3"), Some(false));
        assert_eq!(blooms.may_contain("trace_id", b, "t-1"), Some(false));
        assert_eq!(blooms.may_contain("message", a, "disk full"), None);
        assert_eq!(
            blooms.may_contain("request_id", a, "request-t-2"),
            Some(true)
        );
        assert_eq!(
            blooms.may_contain("request_id", b, "request-t-2"),
            Some(false)
        );

        let empty: Arc<dyn ObjectStore> = Arc::new(InMemory::new());

        assert_eq!(
            build(
                empty.clone(),
                &stream,
                &segments,
                Some(&blooms),
                &HashMap::new()
            )
            .await,
            blooms
        );
        assert!(
            build(empty, &stream, &segments, None, &HashMap::new())
                .await
                .is_empty()
        );
    }
}
