// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! FileCatalog-native immutable Segment/Artifact publication.

use arrow::array::RecordBatch;
use bytes::Bytes;
use domain::{
    storage::{
        Artifact, ArtifactRole, ArtifactState, ArtifactTypeId, DataSegment, FlushProvenance,
        ObjectChecksum, ObjectKey, PhysicalDataset, SchemaFingerprint, SegmentState, StoredObject,
        type_id,
    },
    stream::{Clustering, FieldPartitions, StreamDefinition},
};
use kernel::{Error, Result, time::TimestampMicros};
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, path::Path};

use super::{ParquetWriter, index_objects::SegmentObjects};
use crate::infra::{
    search::tantivy_index,
    storage::{
        parquet::partition::{split_by_partition, split_by_shard},
        write_metrics::{WriteStage, record_written},
    },
};

mod encode;

use encode::{EncodedPiece, IndexBuild, encode_piece};

/// The rows of one segment: a time partition, or one shard of it.
struct Piece {
    batch: RecordBatch,
    shard: u16,
    /// The partition fields every row's shard follows; `None` when the rows were not sharded.
    sharding: Option<FieldPartitions>,
    /// The order of the rows inside the segment's file, see [`Clustering`].
    clustering: Option<Clustering>,
}

impl ParquetWriter {
    /// Encode and upload every partition of one immutable buffer generation, each partition split
    /// further into the shards of `sharding` when the stream has one. The returned Segment/Artifact
    /// graph is not visible until the caller atomically commits it through
    /// `FileCatalog::commit_flush`.
    pub async fn flush_catalog(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: &FlushProvenance,
        batch: RecordBatch,
        sharding: Option<&FieldPartitions>,
    ) -> Result<Vec<DataSegment>> {
        self.flush_catalog_to_store(
            self.object_store.as_ref(),
            stream,
            dataset,
            provenance,
            batch,
            sharding,
        )
        .await
    }

    pub async fn flush_catalog_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: &FlushProvenance,
        batch: RecordBatch,
        sharding: Option<&FieldPartitions>,
    ) -> Result<Vec<DataSegment>> {
        self.write_catalog_to_store(
            store,
            stream,
            dataset,
            Some(provenance),
            batch,
            sharding,
            None,
        )
        .await
    }

    /// [`Self::flush_catalog`], with the rows of each file in the order `clustering` gives them
    /// where it applies (see [`Clustering`]).
    pub async fn flush_catalog_clustered(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: &FlushProvenance,
        batch: RecordBatch,
        sharding: Option<&FieldPartitions>,
        clustering: Option<&Clustering>,
    ) -> Result<Vec<DataSegment>> {
        self.write_catalog_to_store(
            self.object_store.as_ref(),
            stream,
            dataset,
            Some(provenance),
            batch,
            sharding,
            clustering,
        )
        .await
    }

    /// Encode immutable compaction replacements. These segments carry no WAL sequence or flush
    /// identity and become visible only through `FileCatalog::replace_segments`.
    pub async fn write_compaction_catalog(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        batch: RecordBatch,
    ) -> Result<Vec<DataSegment>> {
        self.write_catalog_to_store(
            self.object_store.as_ref(),
            stream,
            dataset,
            None,
            batch,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_catalog_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: Option<&FlushProvenance>,
        batch: RecordBatch,
        sharding: Option<&FieldPartitions>,
        clustering: Option<&Clustering>,
    ) -> Result<Vec<DataSegment>> {
        if dataset.organization_id != stream.org_id || dataset.logical_stream_id != stream.id {
            return Err(Error::invalid(format!(
                "dataset {} does not belong to stream {}",
                dataset.id, stream.id
            )));
        }

        let partitions = split_by_partition(&batch, &dataset.partition_policy)?;

        if partitions.is_empty() {
            return Err(Error::invalid("catalog write called with empty batch"));
        }

        let mut pieces = Vec::with_capacity(partitions.len());

        for batch in partitions {
            let shards = match sharding {
                Some(partitions) => {
                    split_by_shard(&batch, partitions)?.map(|shards| (partitions, shards))
                }
                None => None,
            };

            match shards {
                Some((partitions, shards)) => {
                    pieces.extend(shards.into_iter().map(|(shard, batch)| Piece {
                        batch,
                        shard,
                        sharding: Some(partitions.clone()),
                        clustering: clustering.cloned(),
                    }));
                }
                None => pieces.push(Piece {
                    batch,
                    shard: 0,
                    sharding: None,
                    clustering: clustering.cloned(),
                }),
            }
        }

        let mut segments = Vec::with_capacity(pieces.len());

        for (ordinal, piece) in pieces.into_iter().enumerate() {
            let result = self
                .write_catalog_partition(store, stream, dataset, provenance, ordinal as u32, piece)
                .await;

            match result {
                Ok(segment) => segments.push(segment),
                Err(error) => {
                    self.delete_catalog_segments_from_store(store, &segments)
                        .await;
                    return Err(error);
                }
            }
        }

        let stage = match provenance {
            Some(_) => WriteStage::Flush,
            None => WriteStage::Compaction,
        };

        record_written(&stream.stream_type, stage, &segments);
        Ok(segments)
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_catalog_partition(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: Option<&FlushProvenance>,
        output_ordinal: u32,
        piece: Piece,
    ) -> Result<DataSegment> {
        let EncodedPiece {
            partition,
            time_range,
            segment_id,
            primary_id,
            primary_key,
            parquet,
            parquet_checksum,
            schema_fingerprint,
            index,
            filters,
            minute_counts,
            column_stats,
            row_count,
        } = encode_piece(stream, dataset, piece).await?;

        let parquet_size_bytes = parquet.len() as u64;
        let primary_put = put_immutable(store, &primary_key, parquet).await?;

        let primary = Artifact {
            id: primary_id.clone(),
            role: ArtifactRole::PrimaryData,
            artifact_type: ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
            format_version: 1,
            object: StoredObject {
                key: primary_key,
                size_bytes: parquet_size_bytes,
                checksum: parquet_checksum,
                etag: primary_put.e_tag,
            },
            source_artifact_id: None,
            source_checksum: None,
            schema_fingerprint: Some(schema_fingerprint),
            state: ArtifactState::Ready,
            failure_reason: None,
        };

        let mut auxiliaries = Vec::with_capacity(usize::from(index.is_some()));

        if let Some(index) = index {
            let (artifact_id, artifact_type, object, state, failure_reason) = match index {
                IndexBuild::Ready {
                    artifact_id,
                    artifact_type,
                    key,
                    bytes,
                    checksum: object_checksum,
                } => {
                    let size_bytes = bytes.len() as u64;

                    match put_immutable(store, &key, bytes).await {
                        Ok(put) => (
                            artifact_id,
                            artifact_type,
                            StoredObject {
                                key,
                                size_bytes,
                                checksum: object_checksum,
                                etag: put.e_tag,
                            },
                            ArtifactState::Ready,
                            None,
                        ),
                        Err(error) => (
                            artifact_id,
                            artifact_type,
                            pending_object(key),
                            ArtifactState::Failed,
                            Some(truncate_failure(&error.to_string())),
                        ),
                    }
                }
                IndexBuild::Failed {
                    artifact_id,
                    artifact_type,
                    key,
                    reason,
                } => (
                    artifact_id,
                    artifact_type,
                    pending_object(key),
                    ArtifactState::Failed,
                    Some(reason),
                ),
            };

            auxiliaries.push(Artifact {
                id: artifact_id,
                role: ArtifactRole::Index,
                artifact_type,
                format_version: tantivy_index::FORMAT_VERSION,
                object,
                source_artifact_id: Some(primary_id),
                source_checksum: Some(primary.object.checksum.clone()),
                schema_fingerprint: Some(schema_fingerprint),
                state,
                failure_reason,
            });
        }

        let objects = SegmentObjects {
            store,
            stream,
            dataset,
            partition: &partition,
            segment_id: &segment_id,
            primary: &primary,
        };

        if let Some(filters) = filters {
            auxiliaries.extend(objects.publish_filters(filters).await);
        }

        if let Some(counts) = minute_counts {
            auxiliaries.extend(objects.publish_minute_counts(counts).await);
        }

        Ok(DataSegment {
            id: segment_id,
            organization_id: dataset.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            time_range,
            sequence_range: provenance.map(|value| value.sequence),
            row_count,
            schema_fingerprint: Some(schema_fingerprint),
            column_stats,
            flush_id: provenance.map(|value| value.flush_id.clone()),
            output_ordinal,
            primary,
            auxiliaries,
            state: SegmentState::Active,
            visible_from_version: 0,
            retired_at_version: None,
            created_at_micros: TimestampMicros::now().0,
        })
    }

    /// Best-effort cleanup for uploaded replacements that failed Catalog publication.
    pub async fn delete_catalog_outputs(&self, segments: &[DataSegment]) {
        self.delete_catalog_segments_from_store(self.object_store.as_ref(), segments)
            .await;
    }

    async fn delete_catalog_segments_from_store(
        &self,
        store: &dyn ObjectStore,
        segments: &[DataSegment],
    ) {
        for artifact in segments.iter().flat_map(|segment| segment.artifacts()) {
            if let Err(error) = store
                .delete(&Path::from(artifact.object.key.as_str()))
                .await
            {
                tracing::warn!(
                    object_key = %artifact.object.key,
                    %error,
                    "failed to clean partial catalog flush output"
                );
            }
        }
    }
}

pub(super) async fn put_immutable(
    store: &dyn ObjectStore,
    key: &ObjectKey,
    bytes: Bytes,
) -> Result<object_store::PutResult> {
    store
        .put_opts(
            &Path::from(key.as_str()),
            PutPayload::from(bytes),
            PutOptions {
                mode: PutMode::Create,
                ..PutOptions::default()
            },
        )
        .await
        .map_err(|error| Error::internal(format!("object_store put immutable {key}: {error}")))
}

pub(super) fn checksum(bytes: &[u8]) -> ObjectChecksum {
    ObjectChecksum::from_string(format!("b3:{}", blake3::hash(bytes).to_hex()))
}

pub(super) fn pending_object(key: ObjectKey) -> StoredObject {
    StoredObject {
        key,
        size_bytes: 0,
        checksum: checksum(&[]),
        etag: None,
    }
}

pub(super) fn truncate_failure(error: &str) -> String {
    error.chars().take(1024).collect()
}

pub(super) fn schema_fingerprint(schema: &arrow::datatypes::Schema) -> SchemaFingerprint {
    let mut hasher = blake3::Hasher::new();

    for field in schema.fields() {
        hasher.update(field.name().as_bytes());
        hasher.update(&[0]);
        hasher.update(format!("{:?}", field.data_type()).as_bytes());
        hasher.update(&[u8::from(field.is_nullable())]);
    }

    let digest = hasher.finalize();
    let mut value = [0_u8; 8];
    value.copy_from_slice(&digest.as_bytes()[..8]);
    SchemaFingerprint(i64::from_le_bytes(value))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::{
        intake::RawEvent,
        storage::{WalSequence, WriterEpoch, WriterNodeId, primary_dataset_type},
        stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType},
    };
    use futures::StreamExt;
    use kernel::{ids::Id, time::TimestampMicros};
    use object_store::{ObjectStore, memory::InMemory};
    use serde_json::json;

    use super::*;
    use crate::infra::intake::{
        BufferWriter, RecordBuilder, dataset_resolver::test_support::test_catalog_and_resolver,
    };

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "customer-visible-name".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "message".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    index_type: StreamIndexType::FullText,
                    encrypted: false,
                }],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn event(timestamp: i64, message: &str) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(timestamp),
            fields: [("message".to_string(), json!(message))]
                .into_iter()
                .collect(),
        }
    }

    #[tokio::test]
    async fn catalog_flush_publishes_explicit_artifacts_in_stable_layout() {
        let stream = stream();
        let (_, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let writer = BufferWriter::new(WriterNodeId::new("node-a"), WriterEpoch(4));
        let mut records = RecordBuilder::new(&stream);
        records
            .push_with_position(&event(1, "first"), &writer, WalSequence(10))
            .unwrap();
        records
            .push_with_position(&event(3_600_000_001, "second"), &writer, WalSequence(11))
            .unwrap();
        let generation = records.begin_flush().unwrap().unwrap();
        let provenance = generation.provenance();
        let store = Arc::new(InMemory::new());

        let segments = ParquetWriter::new(store.clone())
            .flush_catalog(
                &stream,
                &dataset.dataset,
                &provenance,
                generation.batch,
                None,
            )
            .await
            .unwrap();

        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.output_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        for segment in &segments {
            assert_eq!(segment.flush_id.as_ref(), Some(&provenance.flush_id));
            assert_eq!(segment.sequence_range, Some(provenance.sequence));
            assert_eq!(segment.primary.role, ArtifactRole::PrimaryData);
            assert!(segment.primary.object.checksum.as_str().starts_with("b3:"));
            assert!(segment.primary.object.key.as_str().starts_with(&format!(
                "artifacts/v1/{}/logs/{}/",
                stream.org_id, dataset.dataset.id
            )));
            assert!(!segment.primary.object.key.as_str().contains(&stream.name));
            assert_eq!(segment.auxiliaries.len(), 1);
            assert_eq!(
                segment.auxiliaries[0].source_artifact_id.as_ref(),
                Some(&segment.primary.id)
            );
            assert_eq!(
                segment.auxiliaries[0].source_checksum.as_ref(),
                Some(&segment.primary.object.checksum)
            );
        }
        assert_eq!(store.list(None).collect::<Vec<_>>().await.len(), 4);
    }
}
