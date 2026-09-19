// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Query snapshot bridge from logical streams to FileCatalog segments plus local buffers.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use async_trait::async_trait;

use crate::{
    domain::{
        storage::{
            ArtifactRole, ArtifactState, CatalogSnapshot, DatasetSelection, DatasetState,
            DatasetTypeId, FileCatalog, OrganizationScope, PhysicalDataset, PhysicalDatasetId,
            QueryFile, QueryFileSource, primary_dataset_type, type_id,
        },
        stream::{StreamDefinition, StreamRepository, StreamType},
    },
    infra::{
        intake::{BufferPool, buffer_pool::align_to_schema, physical_schema},
        storage::{arrow_schema, manifest::PartitionManifestReader, object_reader::ObjectReader},
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

#[derive(Debug, Clone)]
pub struct QueryDatasetSnapshot {
    pub dataset_id: PhysicalDatasetId,
    pub dataset_type: DatasetTypeId,
    pub catalog_version: u64,
    pub schema: SchemaRef,
    pub files: Vec<QueryFile>,
    /// Explicit Primary Artifact object key → Tantivy Artifact object key mapping.
    pub tantivy_indexes: HashMap<String, String>,
    pub buffered_batches: Vec<RecordBatch>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamStorageSnapshot {
    pub datasets: Vec<QueryDatasetSnapshot>,
}

#[derive(Debug, Clone)]
pub struct StreamSnapshotSelection {
    pub stream: StreamDefinition,
    pub dataset_types: Vec<DatasetTypeId>,
}

impl StreamSnapshotSelection {
    pub fn new(stream: StreamDefinition, dataset_types: impl Into<Vec<DatasetTypeId>>) -> Self {
        Self {
            stream,
            dataset_types: dataset_types.into(),
        }
    }
}

impl StreamStorageSnapshot {
    pub fn files(&self) -> Vec<QueryFile> {
        self.datasets
            .iter()
            .flat_map(|dataset| dataset.files.iter().cloned())
            .collect()
    }

    pub fn buffered_batches(&self) -> Vec<RecordBatch> {
        self.datasets
            .iter()
            .flat_map(|dataset| dataset.buffered_batches.iter().cloned())
            .collect()
    }

    pub fn tantivy_indexes(&self) -> HashMap<String, String> {
        self.datasets
            .iter()
            .flat_map(|dataset| dataset.tantivy_indexes.clone())
            .collect()
    }

    pub fn dataset(&self, dataset_type: &DatasetTypeId) -> Option<&QueryDatasetSnapshot> {
        self.datasets
            .iter()
            .find(|dataset| dataset.dataset_type == *dataset_type)
    }
}

/// The buffer snapshot is deliberately captured before the repeatable-read Catalog snapshot.
/// Checkpoints from the latter then remove any in-flight generation that became committed between
/// the two observations.
pub struct CatalogQuerySource {
    catalog: Arc<dyn FileCatalog>,
    buffers: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    object_reader: Arc<ObjectReader>,
    manifests: Arc<PartitionManifestReader>,
}

impl CatalogQuerySource {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        buffers: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
        object_reader: Arc<ObjectReader>,
        manifests: Arc<PartitionManifestReader>,
    ) -> Self {
        Self {
            catalog,
            buffers,
            streams,
            object_reader,
            manifests,
        }
    }

    pub async fn snapshot_by_name(
        &self,
        organization_id: &Id,
        stream_name: &str,
        stream_type: StreamType,
        dataset_types: &[DatasetTypeId],
        time_range: TimeRange,
    ) -> Result<StreamStorageSnapshot> {
        let stream = self
            .streams
            .get(organization_id, stream_name, stream_type)
            .await?;
        self.snapshot_stream(&stream, dataset_types, time_range)
            .await
    }

    /// Current logical schema for distributed readers that receive an explicit file list.
    /// Parquet schema adaptation fills columns absent from older files with nulls.
    pub async fn logical_arrow_schema(
        &self,
        organization_id: &Id,
        stream_name: &str,
        stream_type: StreamType,
    ) -> Result<SchemaRef> {
        let stream = self
            .streams
            .get(organization_id, stream_name, stream_type)
            .await?;
        Ok(arrow_schema::to_arrow(&stream.schema))
    }

    /// Register an explicit distributed-query projection with the shared immutable object reader.
    pub fn register_query_file(&self, file: &QueryFile) -> Result<()> {
        if let Some(object) = file.stored_object() {
            self.object_reader.register(&file.org_id, &object)?;
        }
        Ok(())
    }

    pub async fn snapshot_stream(
        &self,
        stream: &StreamDefinition,
        dataset_types: &[DatasetTypeId],
        time_range: TimeRange,
    ) -> Result<StreamStorageSnapshot> {
        self.snapshot_streams(
            &[StreamSnapshotSelection::new(
                stream.clone(),
                dataset_types.to_vec(),
            )],
            time_range,
        )
        .await?
        .pop()
        .ok_or_else(|| Error::internal("catalog stream snapshot omitted selection"))
    }

    /// Capture buffers for every selected logical stream first, then resolve all of their
    /// physical datasets through one repeatable-read FileCatalog snapshot.
    pub async fn snapshot_streams(
        &self,
        selections: &[StreamSnapshotSelection],
        time_range: TimeRange,
    ) -> Result<Vec<StreamStorageSnapshot>> {
        let Some(first) = selections.first() else {
            return Ok(Vec::new());
        };
        if selections
            .iter()
            .any(|selection| selection.stream.org_id != first.stream.org_id)
        {
            return Err(Error::invalid(
                "one catalog snapshot cannot span multiple organizations",
            ));
        }
        let scope = OrganizationScope::new(first.stream.org_id.clone());
        let mut selected_by_stream = Vec::with_capacity(selections.len());
        for selection in selections {
            if selection.dataset_types.is_empty() {
                selected_by_stream.push(Vec::new());
                continue;
            }
            let available = self
                .catalog
                .list_datasets(&scope, &selection.stream.id)
                .await?;
            selected_by_stream.push(select_datasets(&selection.dataset_types, available));
        }

        let dataset_count: usize = selected_by_stream.iter().map(Vec::len).sum();
        let mut buffer_snapshots = HashMap::with_capacity(dataset_count);
        let mut dataset_ids = Vec::with_capacity(dataset_count);
        let mut seen_dataset_ids = HashSet::with_capacity(dataset_count);
        for selected in &selected_by_stream {
            for (_, dataset) in selected {
                if seen_dataset_ids.insert(dataset.id.clone()) {
                    buffer_snapshots.insert(
                        dataset.id.clone(),
                        self.buffers.snapshot_dataset(&dataset.id).await?,
                    );
                    dataset_ids.push(dataset.id.clone());
                }
            }
        }

        let catalog = if dataset_ids.is_empty() {
            CatalogSnapshot::default()
        } else {
            self.catalog
                .snapshot(
                    &scope,
                    DatasetSelection {
                        dataset_ids,
                        time_range,
                        partition_shard: None,
                    },
                )
                .await?
        };

        let mut manifest_segments = HashMap::new();
        for dataset in &catalog.datasets {
            let mut segments = Vec::new();
            for pointer in &dataset.manifests {
                let manifest = self.manifests.load(pointer).await?;
                segments.extend(
                    manifest
                        .segments
                        .iter()
                        .filter(|segment| segment.time_range.overlaps(time_range))
                        .cloned(),
                );
            }
            manifest_segments.insert(dataset.dataset_id.clone(), segments);
        }

        let mut output = Vec::with_capacity(selections.len());
        for (selection, selected) in selections.iter().zip(selected_by_stream) {
            let mut datasets = Vec::with_capacity(selected.len());
            for (_, dataset) in selected {
                datasets.push(build_dataset_snapshot(
                    &selection.stream,
                    &dataset,
                    &catalog,
                    manifest_segments.remove(&dataset.id).unwrap_or_default(),
                    buffer_snapshots
                        .get(&dataset.id)
                        .cloned()
                        .unwrap_or_default(),
                    time_range,
                    &self.object_reader,
                )?);
            }
            output.push(StreamStorageSnapshot { datasets });
        }
        Ok(output)
    }
}

fn select_datasets(
    requested: &[DatasetTypeId],
    available: Vec<PhysicalDataset>,
) -> Vec<(DatasetTypeId, PhysicalDataset)> {
    requested
        .iter()
        .filter_map(|dataset_type| {
            available
                .iter()
                .find(|dataset| {
                    dataset.dataset_type == *dataset_type && dataset.state == DatasetState::Active
                })
                .cloned()
                .map(|dataset| (dataset_type.clone(), dataset))
        })
        .collect()
}

fn build_dataset_snapshot(
    stream: &StreamDefinition,
    dataset: &PhysicalDataset,
    catalog: &CatalogSnapshot,
    manifest_segments: Vec<crate::domain::storage::DataSegment>,
    buffer_batches: Vec<crate::infra::intake::BufferedRecordBatch>,
    time_range: TimeRange,
    object_reader: &ObjectReader,
) -> Result<QueryDatasetSnapshot> {
    let dataset_type = dataset.dataset_type.clone();
    let snapshot = catalog
        .dataset(&dataset.id)
        .ok_or_else(|| Error::internal(format!("catalog omitted dataset {}", dataset.id)))?;
    let segments = snapshot
        .segments
        .iter()
        .cloned()
        .chain(manifest_segments)
        .collect::<Vec<_>>();
    for segment in &segments {
        object_reader.register_segment(&stream.org_id, segment)?;
    }
    let files = segments
        .iter()
        .map(|segment| segment_to_query_file(stream, &dataset_type, segment))
        .collect::<Result<Vec<_>>>()?;
    let tantivy_indexes = segments.iter().filter_map(segment_tantivy_index).collect();

    let physical_stream = physical_schema::project(stream, &dataset_type);
    let target_schema = arrow_schema::to_arrow(&physical_stream.schema);
    let mut visible_buffers = Vec::new();
    for buffered in buffer_batches {
        let committed = snapshot.committed_sequence(
            &buffered.writer.writer_node_id,
            buffered.writer.writer_epoch,
        );
        if let Some(batch) = buffered
            .visible_after(committed, time_range)
            .map_err(|error| Error::internal(format!("filter buffer snapshot: {error}")))?
        {
            visible_buffers.push(
                align_to_schema(batch, &target_schema)
                    .map_err(|error| Error::internal(format!("align buffer schema: {error}")))?,
            );
        }
    }

    Ok(QueryDatasetSnapshot {
        dataset_id: dataset.id.clone(),
        dataset_type,
        catalog_version: snapshot.catalog_version,
        schema: target_schema,
        files,
        tantivy_indexes,
        buffered_batches: visible_buffers,
    })
}

fn segment_tantivy_index(
    segment: &crate::domain::storage::DataSegment,
) -> Option<(String, String)> {
    segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            segment.primary.role == ArtifactRole::PrimaryData
                && segment.primary.state == ArtifactState::Ready
                && segment.primary.artifact_type.as_str() == type_id::builtin::ARTIFACT_PARQUET
                && segment.primary.format_version == 1
                && segment.schema_fingerprint == segment.primary.schema_fingerprint
                && artifact.role == ArtifactRole::Index
                && artifact.state == ArtifactState::Ready
                && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
                && artifact.format_version == 1
                && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
                && artifact.source_checksum.as_ref() == Some(&segment.primary.object.checksum)
                && artifact.schema_fingerprint == segment.primary.schema_fingerprint
        })
        .map(|artifact| {
            (
                segment.primary.object.key.as_str().to_owned(),
                artifact.object.key.as_str().to_owned(),
            )
        })
}

fn segment_to_query_file(
    stream: &StreamDefinition,
    dataset_type: &DatasetTypeId,
    segment: &crate::domain::storage::DataSegment,
) -> Result<QueryFile> {
    let primary = &segment.primary;
    if primary.role != ArtifactRole::PrimaryData
        || primary.state != ArtifactState::Ready
        || primary.artifact_type.as_str() != type_id::builtin::ARTIFACT_PARQUET
        || primary.format_version != 1
    {
        return Err(Error::internal(format!(
            "active segment {} has no supported ready Parquet primary Artifact",
            segment.id
        )));
    }
    Ok(QueryFile {
        id: segment.id.0.clone(),
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        dataset_type: dataset_type.clone(),
        object_key: primary.object.key.as_str().to_owned(),
        checksum: Some(primary.object.checksum.clone()),
        etag: primary.object.etag.clone(),
        time_range: segment.time_range,
        rows: segment.row_count,
        size_bytes: primary.object.size_bytes,
        min_values: segment.column_stats.min_values.clone(),
        max_values: segment.column_stats.max_values.clone(),
    })
}

/// Read-only query projection backed by FileCatalog snapshots and immutable manifests.
#[async_trait]
impl QueryFileSource for CatalogQuerySource {
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        let dataset_type = primary_dataset_type(stream_type)?;
        self.find_dataset(org_id, stream, stream_type, dataset_type, time_range)
            .await
    }

    async fn find_dataset(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_type: DatasetTypeId,
        time_range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .snapshot_by_name(
                org_id,
                stream,
                stream_type,
                std::slice::from_ref(&dataset_type),
                time_range,
            )
            .await?
            .dataset(&dataset_type)
            .map(|dataset| dataset.files.clone())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use object_store::memory::InMemory;
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            intake::RawEvent,
            storage::{
                CommitFlush, OrganizationScope, WalSequence, WriterEpoch, WriterNodeId,
                type_id::builtin,
            },
            stream::{FieldDef, FieldType, Schema},
        },
        infra::{
            intake::{BufferWriter, dataset_resolver::test_support::test_catalog_and_resolver},
            storage::parquet::writer::ParquetWriter,
        },
        shared::time::TimestampMicros,
    };

    struct OneStream(StreamDefinition);

    #[async_trait]
    impl StreamRepository for OneStream {
        async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
            Ok(definition)
        }

        async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
            Ok(())
        }

        async fn get(
            &self,
            organization_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            if self.0.org_id == *organization_id
                && self.0.name == name
                && self.0.stream_type == stream_type
            {
                Ok(self.0.clone())
            } else {
                Err(Error::not_found(format!("stream {name}")))
            }
        }

        async fn list(&self, organization_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok((self.0.org_id == *organization_id)
                .then(|| self.0.clone())
                .into_iter()
                .collect())
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn metric_stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "cpu".into(),
            stream_type: StreamType::METRICS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "value".into(),
                    data_type: FieldType::Float64,
                    nullable: false,
                    index_type: None,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn event(timestamp: i64, value: f64) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(timestamp),
            fields: [("value".to_string(), json!(value))].into_iter().collect(),
        }
    }

    fn local_object_reader() -> Arc<ObjectReader> {
        let store: Arc<dyn object_store::ObjectStore> = Arc::new(InMemory::new());
        ObjectReader::build(
            store,
            "local",
            &crate::config::ObjectCacheSettings::default(),
        )
        .unwrap()
    }

    fn local_manifest_reader(objects: Arc<ObjectReader>) -> Arc<PartitionManifestReader> {
        Arc::new(PartitionManifestReader::new(objects, 1024 * 1024))
    }

    #[tokio::test]
    async fn one_catalog_snapshot_filters_inflight_rows_by_writer_checkpoint() {
        let stream = metric_stream();
        let buffers = Arc::new(BufferPool::new());
        let (catalog, resolver) = test_catalog_and_resolver();
        let raw_type = primary_dataset_type(stream.stream_type).unwrap();
        let rollup_type = DatasetTypeId::builtin(builtin::DATASET_METRIC_ROLLUP);
        let raw = resolver.resolve(&stream, raw_type.clone()).await.unwrap();
        let rollup = resolver
            .resolve(&stream, rollup_type.clone())
            .await
            .unwrap();
        let writer = BufferWriter::new(WriterNodeId::new("node-a"), WriterEpoch(7));

        let raw_buffer = buffers.get_or_create_dataset(&stream, raw.clone()).unwrap();
        let generation = {
            let mut records = raw_buffer.records().lock().await;
            for sequence in 1..=2 {
                records
                    .push_with_position(
                        &event(sequence * 10, sequence as f64),
                        &writer,
                        WalSequence(sequence as u64),
                    )
                    .unwrap();
            }
            let generation = records.begin_flush().unwrap().unwrap();
            records
                .push_with_position(&event(30, 3.0), &writer, WalSequence(3))
                .unwrap();
            generation
        };
        let rollup_buffer = buffers
            .get_or_create_dataset(&stream, rollup.clone())
            .unwrap();
        rollup_buffer
            .records()
            .lock()
            .await
            .push_with_position(&event(40, 4.0), &writer, WalSequence(1))
            .unwrap();

        let provenance = generation.provenance();
        let segments = ParquetWriter::new(Arc::new(InMemory::new()))
            .flush_catalog(&stream, &raw.dataset, &provenance, generation.batch.clone())
            .await
            .unwrap();
        catalog
            .commit_flush(
                &OrganizationScope::new(stream.org_id.clone()),
                CommitFlush {
                    dataset_id: raw.dataset.id.clone(),
                    provenance,
                    segments,
                },
            )
            .await
            .unwrap();
        let objects = local_object_reader();
        let source = CatalogQuerySource::new(
            catalog.clone(),
            buffers,
            Arc::new(OneStream(stream.clone())),
            objects.clone(),
            local_manifest_reader(objects),
        );
        let snapshot = source
            .snapshot_stream(
                &stream,
                &[raw_type.clone(), rollup_type.clone()],
                TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
            )
            .await
            .unwrap();

        assert_eq!(
            catalog.snapshot_calls(),
            1,
            "both datasets share one snapshot"
        );
        assert_eq!(snapshot.datasets.len(), 2);
        let raw_snapshot = snapshot.dataset(&raw_type).unwrap();
        assert_eq!(raw_snapshot.files.len(), 1);
        assert_eq!(raw_snapshot.files[0].rows, 2);
        assert_eq!(
            raw_snapshot
                .buffered_batches
                .iter()
                .map(RecordBatch::num_rows)
                .sum::<usize>(),
            1,
            "checkpoint removes the in-flight rows already covered by the catalog segment"
        );
        assert_eq!(
            snapshot
                .dataset(&rollup_type)
                .unwrap()
                .buffered_batches
                .iter()
                .map(RecordBatch::num_rows)
                .sum::<usize>(),
            1
        );
    }

    #[tokio::test]
    async fn multiple_logical_streams_share_one_catalog_snapshot() {
        let first = metric_stream();
        let mut second = metric_stream();
        second.id = Id::new();
        second.name = "memory".into();
        let buffers = Arc::new(BufferPool::new());
        let (catalog, resolver) = test_catalog_and_resolver();
        let raw_type = primary_dataset_type(first.stream_type).unwrap();
        resolver.resolve(&first, raw_type.clone()).await.unwrap();
        resolver.resolve(&second, raw_type.clone()).await.unwrap();
        let objects = local_object_reader();
        let source = CatalogQuerySource::new(
            catalog.clone(),
            buffers,
            Arc::new(OneStream(first.clone())),
            objects.clone(),
            local_manifest_reader(objects),
        );

        let snapshots = source
            .snapshot_streams(
                &[
                    StreamSnapshotSelection::new(first, vec![raw_type.clone()]),
                    StreamSnapshotSelection::new(second, vec![raw_type]),
                ],
                TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
            )
            .await
            .unwrap();

        assert_eq!(snapshots.len(), 2);
        assert_eq!(catalog.snapshot_calls(), 1);
        assert!(
            snapshots
                .iter()
                .all(|snapshot| snapshot.datasets.len() == 1)
        );
    }
}
