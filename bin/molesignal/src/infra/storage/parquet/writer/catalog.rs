// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! FileCatalog-native immutable Segment/Artifact publication.

use arrow::array::RecordBatch;
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, path::Path};

use super::{
    ParquetWriter, build_tantivy_bytes_for_batch, encode_parquet,
    metadata::{timestamp_range, zone_maps},
};
use crate::{
    domain::{
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ColumnStats,
            DataSegment, FlushProvenance, IndexTypeId, ObjectChecksum, ObjectKey, Partition,
            PhysicalDataset, SchemaFingerprint, SegmentId, SegmentState, StoredObject, type_id,
        },
        stream::StreamDefinition,
    },
    infra::storage::{
        layout::StorageLayout,
        parquet::partition::{sort_for_storage, split_by_partition},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

enum IndexBuild {
    Ready {
        artifact_id: ArtifactId,
        artifact_type: ArtifactTypeId,
        key: ObjectKey,
        bytes: Vec<u8>,
    },
    Failed {
        artifact_id: ArtifactId,
        artifact_type: ArtifactTypeId,
        key: ObjectKey,
        reason: String,
    },
}

impl ParquetWriter {
    /// Encode and upload every partition of one immutable buffer generation. The returned
    /// Segment/Artifact graph is not visible until the caller atomically commits it through
    /// `FileCatalog::commit_flush`.
    pub async fn flush_catalog(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: &FlushProvenance,
        batch: RecordBatch,
    ) -> Result<Vec<DataSegment>> {
        self.flush_catalog_to_store(
            self.object_store.as_ref(),
            stream,
            dataset,
            provenance,
            batch,
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
    ) -> Result<Vec<DataSegment>> {
        self.write_catalog_to_store(store, stream, dataset, Some(provenance), batch)
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
        self.write_catalog_to_store(self.object_store.as_ref(), stream, dataset, None, batch)
            .await
    }

    async fn write_catalog_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        provenance: Option<&FlushProvenance>,
        batch: RecordBatch,
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

        let mut segments = Vec::with_capacity(partitions.len());
        for (ordinal, partition_batch) in partitions.into_iter().enumerate() {
            let result = self
                .write_catalog_partition(
                    store,
                    stream,
                    dataset,
                    provenance,
                    ordinal as u32,
                    partition_batch,
                )
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
        batch: RecordBatch,
    ) -> Result<DataSegment> {
        let batch = sort_for_storage(batch, &dataset.dataset_type)?;
        let (start_micros, end_micros) = timestamp_range(&batch)?;
        let partition_start = dataset.partition_policy.bucket_start_micros(start_micros);
        let partition = Partition {
            start_micros: partition_start,
            end_micros: partition_start
                .checked_add(dataset.partition_policy.granularity.micros())
                .ok_or_else(|| Error::invalid("partition end overflows i64"))?,
            shard: 0,
        };

        let segment_id = SegmentId::generate();
        let primary_id = ArtifactId::generate();
        let parquet_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET);
        let primary_key = StorageLayout::artifact_key(
            &dataset.organization_id,
            &dataset.id,
            &partition,
            &segment_id,
            &primary_id,
            &parquet_type,
        );
        let parquet_bytes = encode_parquet(stream, &batch)?;
        let parquet_size_bytes = parquet_bytes.len() as u64;
        let parquet_checksum = checksum(&parquet_bytes);
        let schema_fingerprint = schema_fingerprint(batch.schema().as_ref());

        let wants_tantivy = dataset
            .index_policy
            .indexers
            .iter()
            .any(|index| index == &IndexTypeId::builtin(type_id::builtin::INDEX_TANTIVY));
        let index_output = if wants_tantivy {
            match build_tantivy_bytes_for_batch(stream, &batch) {
                Ok(Some(bytes)) => {
                    let artifact_id = ArtifactId::generate();
                    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);
                    let key = StorageLayout::artifact_key(
                        &dataset.organization_id,
                        &dataset.id,
                        &partition,
                        &segment_id,
                        &artifact_id,
                        &artifact_type,
                    );
                    Some(IndexBuild::Ready {
                        artifact_id,
                        artifact_type,
                        key,
                        bytes,
                    })
                }
                Ok(None) => None,
                Err(error) => {
                    let artifact_id = ArtifactId::generate();
                    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);
                    let key = StorageLayout::artifact_key(
                        &dataset.organization_id,
                        &dataset.id,
                        &partition,
                        &segment_id,
                        &artifact_id,
                        &artifact_type,
                    );
                    Some(IndexBuild::Failed {
                        artifact_id,
                        artifact_type,
                        key,
                        reason: truncate_failure(&error.to_string()),
                    })
                }
            }
        } else {
            None
        };

        let primary_put = put_immutable(store, &primary_key, parquet_bytes).await?;
        let primary = Artifact {
            id: primary_id.clone(),
            role: ArtifactRole::PrimaryData,
            artifact_type: parquet_type,
            format_version: 1,
            object: StoredObject {
                key: primary_key.clone(),
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

        let mut auxiliaries = Vec::with_capacity(usize::from(index_output.is_some()));
        if let Some(index_output) = index_output {
            let (artifact_id, artifact_type, object, state, failure_reason) = match index_output {
                IndexBuild::Ready {
                    artifact_id,
                    artifact_type,
                    key,
                    bytes,
                } => {
                    let size_bytes = bytes.len() as u64;
                    let object_checksum = checksum(&bytes);
                    match put_immutable(store, &key, Bytes::from(bytes)).await {
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
                format_version: 1,
                object,
                source_artifact_id: Some(primary_id),
                source_checksum: Some(primary.object.checksum.clone()),
                schema_fingerprint: Some(schema_fingerprint),
                state,
                failure_reason,
            });
        }

        let (min_values, max_values) = zone_maps(stream, &batch);
        Ok(DataSegment {
            id: segment_id,
            organization_id: dataset.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            time_range: TimeRange::new(TimestampMicros(start_micros), TimestampMicros(end_micros)),
            sequence_range: provenance.map(|value| value.sequence),
            row_count: batch.num_rows() as u64,
            schema_fingerprint: Some(schema_fingerprint),
            column_stats: ColumnStats {
                min_values,
                max_values,
            },
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

async fn put_immutable(
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

fn checksum(bytes: &[u8]) -> ObjectChecksum {
    ObjectChecksum::from_string(format!("b3:{}", blake3::hash(bytes).to_hex()))
}

fn pending_object(key: ObjectKey) -> StoredObject {
    StoredObject {
        key,
        size_bytes: 0,
        checksum: checksum(&[]),
        etag: None,
    }
}

fn truncate_failure(error: &str) -> String {
    error.chars().take(1024).collect()
}

fn schema_fingerprint(schema: &arrow::datatypes::Schema) -> SchemaFingerprint {
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

    use futures::StreamExt;
    use object_store::{ObjectStore, memory::InMemory};
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            intake::RawEvent,
            storage::{WalSequence, WriterEpoch, WriterNodeId, primary_dataset_type},
            stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType},
        },
        infra::intake::{
            BufferWriter, RecordBuilder, dataset_resolver::test_support::test_catalog_and_resolver,
        },
        shared::{ids::Id, time::TimestampMicros},
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
                    index_type: Some(StreamIndexType::FullText),
                    indexed: true,
                    encrypted: false,
                    exact: false,
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
            .flush_catalog(&stream, &dataset.dataset, &provenance, generation.batch)
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
                "v1/artifacts/{}/{}/",
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
