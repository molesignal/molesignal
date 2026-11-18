// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The minute counts of a sealed partition's segments, all in one object beside its manifest (see
//! [`PartitionCounts`]). It is written before the manifest is published, from each segment's own
//! object, and the Catalog collects it with the manifest; a segment it does not hold, or a
//! partition without it, is answered from the segment's own object.

use std::sync::Arc;

use domain::storage::{ArtifactRole, ArtifactState, DataSegment, ObjectKey, type_id};
use futures::{StreamExt, stream};
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, path::Path};

use crate::infra::storage::minute_counts::{FORMAT_VERSION, MinuteCounts, PartitionCounts};

/// Segment objects read at once.
const CONCURRENT_READS: usize = 16;

/// The key of the minute counts of a manifest: beside it, like its filters.
pub(crate) fn minute_counts_key(manifest_key: &ObjectKey) -> String {
    format!("{}.minute_counts", manifest_key.as_str())
}

/// Writes the minute counts of `segments` beside the manifest at `manifest_key`. Nothing is
/// written when no segment has counts, and a failure is only logged.
pub(crate) async fn write(
    store: &dyn ObjectStore,
    manifest_key: &ObjectKey,
    segments: &[DataSegment],
) {
    let counts = gather(store, segments).await;

    if counts.is_empty() {
        return;
    }

    let key = minute_counts_key(manifest_key);

    let written = match counts.encode() {
        Ok(bytes) => store
            .put_opts(
                &Path::from(key.as_str()),
                PutPayload::from(bytes),
                PutOptions {
                    mode: PutMode::Create,
                    ..PutOptions::default()
                },
            )
            .await
            .map(|_| ())
            .or_else(|error| match error {
                object_store::Error::AlreadyExists { .. } => Ok(()),
                other => Err(other.to_string()),
            }),
        Err(error) => Err(error.to_string()),
    };

    if let Err(error) = written {
        tracing::warn!(object_key = key, %error, "partition minute counts not written");
    }
}

/// The minute counts of `segments`, read from each one's own object; a segment without them, or
/// whose object cannot be read, is left out.
async fn gather(store: &dyn ObjectStore, segments: &[DataSegment]) -> PartitionCounts {
    let read = stream::iter(segments.iter().filter_map(own_counts))
        .map(|(segment, key)| async move {
            let bytes = store
                .get(&Path::from(key.as_str()))
                .await
                .ok()?
                .bytes()
                .await
                .ok()?;

            match MinuteCounts::decode(&bytes) {
                Ok(counts) => Some((segment, counts)),
                Err(error) => {
                    tracing::warn!(object_key = key, %error, "segment minute counts unreadable");
                    None
                }
            }
        })
        .buffer_unordered(CONCURRENT_READS)
        .collect::<Vec<_>>()
        .await;

    let mut partition = PartitionCounts::default();

    for (segment, counts) in read.into_iter().flatten() {
        partition.insert(segment, Arc::new(counts));
    }

    partition
}

/// The segment's id and the key of its own ready minute counts, built from its primary file.
fn own_counts(segment: &DataSegment) -> Option<(String, String)> {
    segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            artifact.role == ArtifactRole::Index
                && artifact.state == ArtifactState::Ready
                && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_MINUTE_COUNTS
                && artifact.format_version <= FORMAT_VERSION
                && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
        })
        .map(|artifact| {
            (
                segment.id.as_str().to_owned(),
                artifact.object.key.as_str().to_owned(),
            )
        })
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{RecordBatch, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
    };
    use domain::{
        storage::{
            DatasetState, FlushProvenance, IndexPolicy, IndexTypeId, PartitionPolicy,
            PhysicalDataset, PhysicalDatasetId, SequenceRange, StoragePolicy, WalSequence,
            WriterEpoch, WriterNodeId, primary_dataset_type,
        },
        stream::{
            Clustering, FieldDef, FieldType, Schema, StreamDefinition, StreamIndexType, StreamType,
        },
    };
    use kernel::{ids::Id, time::TimestampMicros};
    use object_store::memory::InMemory;

    use super::*;
    use crate::infra::storage::parquet::writer::ParquetWriter;

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    index_type: StreamIndexType::None,
                    encrypted: false,
                }],
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

    fn batch(levels: &[&str]) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("level", DataType::Utf8, true),
        ]));

        let times = TimestampMicrosecondArray::from((1..=levels.len() as i64).collect::<Vec<_>>())
            .with_timezone("UTC");

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(times),
                Arc::new(StringArray::from(levels.to_vec())),
            ],
        )
        .unwrap()
    }

    /// The partition's object holds the counts of each clustered segment, as the segment's own
    /// object does; a segment written in time order has none to give.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_partition_holds_the_counts_of_every_clustered_segment() {
        let stream = stream();
        let dataset = dataset(&stream);
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = ParquetWriter::new(store.clone());
        let clustering = Clustering::new(vec!["level".into()], 60_000_000);
        let mut segments = Vec::new();

        for (ordinal, (levels, clustered)) in [
            (&["INFO", "ERROR", "INFO"][..], true),
            (&["WARN"][..], true),
            (&["INFO"][..], false),
        ]
        .into_iter()
        .enumerate()
        {
            let sequence = ordinal as u64 * 2 + 1;

            let provenance = FlushProvenance::derive(
                WriterNodeId::new("node-a"),
                WriterEpoch(1),
                SequenceRange::new(WalSequence(sequence), WalSequence(sequence + 1)),
            );

            segments.extend(
                writer
                    .flush_catalog_clustered(
                        &stream,
                        &dataset,
                        &provenance,
                        batch(levels),
                        None,
                        clustered.then_some(&clustering),
                    )
                    .await
                    .unwrap(),
            );
        }

        let manifest = ObjectKey::from_string("manifests/v1/org/p/1.parquet");

        write(store.as_ref(), &manifest, &segments).await;

        let bytes = store
            .get(&Path::from(minute_counts_key(&manifest)))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        let partition = PartitionCounts::decode(&bytes).unwrap();
        let totals = segments
            .iter()
            .map(|segment| {
                partition
                    .get(segment.id.as_str())
                    .map(|counts| counts.total())
            })
            .collect::<Vec<_>>();

        assert_eq!(totals, [Some(3), Some(1), None]);
    }

    #[tokio::test]
    async fn nothing_is_written_without_counts() {
        let store = InMemory::new();
        let manifest = ObjectKey::from_string("manifests/v1/org/p/2.parquet");

        write(&store, &manifest, &[]).await;

        assert!(
            store
                .get(&Path::from(minute_counts_key(&manifest)))
                .await
                .is_err()
        );
    }
}
