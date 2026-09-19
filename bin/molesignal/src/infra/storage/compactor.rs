// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Catalog-native compaction and retention orchestration.
//!
//! Inputs are fixed FileCatalog snapshots. Replacement publication is atomic, and retired
//! Artifact objects are queued for delayed GC rather than deleted while older queries may still
//! reference them. No object key is generated or inferred in this module.

use std::sync::{Arc, OnceLock};

use arrow::compute::concat_batches;
use object_store::ObjectStore;
use prometheus::{IntCounter, IntCounterVec};

use super::parquet::{reader::ParquetReader, writer::ParquetWriter};
use crate::{
    config::CompactorSettings,
    domain::{
        storage::{
            ArtifactRole, ArtifactState, DataSegment, DatasetSelection, DatasetState, FileCatalog,
            OrganizationScope, PhysicalDataset, ReplaceSegments, SegmentId, TombstoneSegments,
            type_id,
        },
        stream::{StreamDefinition, StreamType},
    },
    infra::storage::{manifest::PartitionManifestReader, object_reader::ObjectReader},
    shared::{
        Error, Result,
        metrics::{register_int_counter, register_int_counter_vec},
        time::{TimeRange, TimestampMicros},
    },
};

mod downsample;
mod partition;

use partition::{build_groups, validate_group};

static FAILURES: OnceLock<IntCounterVec> = OnceLock::new();
static MERGED: OnceLock<IntCounter> = OnceLock::new();
static RETENTION_DELETED: OnceLock<IntCounter> = OnceLock::new();
const DAY_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

fn failures() -> &'static IntCounterVec {
    FAILURES.get_or_init(|| {
        register_int_counter_vec(
            "compactor_failures_total",
            "compactor failures by reason",
            &["reason"],
        )
    })
}

fn merged() -> &'static IntCounter {
    MERGED.get_or_init(|| {
        register_int_counter(
            "compactor_merged_groups_total",
            "compactor groups merged successfully",
        )
    })
}

fn retention_deleted() -> &'static IntCounter {
    RETENTION_DELETED.get_or_init(|| {
        register_int_counter(
            "compactor_retention_deleted_total",
            "segments tombstoned by retention sweep",
        )
    })
}

pub struct Compactor {
    catalog: Arc<dyn FileCatalog>,
    object_reader: Arc<ObjectReader>,
    manifest_reader: Arc<PartitionManifestReader>,
    reader: Arc<ParquetReader>,
    writer: Arc<ParquetWriter>,
    object_store: Arc<dyn ObjectStore>,
    settings: CompactorSettings,
    gc_grace_micros: i64,
}

impl Compactor {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        object_reader: Arc<ObjectReader>,
        manifest_reader: Arc<PartitionManifestReader>,
        writer: Arc<ParquetWriter>,
        object_store: Arc<dyn ObjectStore>,
        settings: CompactorSettings,
        gc_grace_period_secs: u32,
    ) -> Self {
        Self {
            catalog,
            reader: Arc::new(ParquetReader::new(object_reader.store())),
            object_reader,
            manifest_reader,
            writer,
            object_store,
            settings,
            gc_grace_micros: i64::from(gc_grace_period_secs) * 1_000_000,
        }
    }

    pub fn settings(&self) -> &CompactorSettings {
        &self.settings
    }

    #[tracing::instrument(
        name = "worker.compactor",
        parent = None,
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.worker.name = "compactor",
            molesignal.stream.type = ?stream.stream_type
        )
    )]
    pub async fn sweep_one(
        &self,
        stream: &StreamDefinition,
        time_range: TimeRange,
    ) -> Result<usize> {
        let datasets = self.datasets_for_stream(stream).await?;
        let mut total = 0;
        for dataset in &datasets {
            total += self.sweep_dataset(stream, dataset, time_range).await?;
        }
        Ok(total)
    }

    async fn sweep_dataset(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        time_range: TimeRange,
    ) -> Result<usize> {
        let target_bytes = (self.settings.target_mb as u64).saturating_mul(1024 * 1024);
        let segments = self.snapshot_dataset(stream, dataset, time_range).await?;
        let mut candidates = Vec::new();
        for segment in segments {
            if valid_primary(&segment) && segment.primary.object.size_bytes < target_bytes {
                candidates.push(segment);
            } else if !valid_primary(&segment) {
                failures()
                    .with_label_values(&["invalid_primary_artifact"])
                    .inc();
                tracing::error!(
                    dataset_id = %dataset.id,
                    segment_id = %segment.id,
                    "compactor skipped active segment without a supported ready Parquet primary Artifact"
                );
            }
        }
        let (groups, invalid) = build_groups(candidates, target_bytes);
        for segment in invalid {
            failures().with_label_values(&["invalid_partition"]).inc();
            tracing::error!(
                dataset_id = %dataset.id,
                segment_id = %segment.id,
                start = segment.time_range.start.0,
                end = segment.time_range.end.0,
                "compactor refused segment outside its Catalog partition"
            );
        }

        let mut merged_groups = 0;
        for group in groups {
            match self.merge_group(stream, dataset, &group).await {
                Ok(true) => {
                    merged_groups += 1;
                    merged().inc();
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    stream = %stream.name,
                    dataset_id = %dataset.id,
                    group_size = group.len(),
                    %error,
                    "compactor merge group failed; will retry next sweep"
                ),
            }
        }
        Ok(merged_groups)
    }

    #[tracing::instrument(
        name = "compactor.merge",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.compactor.file_count = group.len()
        )
    )]
    async fn merge_group(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        group: &[DataSegment],
    ) -> Result<bool> {
        validate_group(group, &dataset.id)?;
        let mut batches = Vec::new();
        let mut missing = Vec::new();
        for segment in group {
            self.object_reader
                .register_segment(&stream.org_id, segment)?;
            let key = segment.primary.object.key.as_str();
            match self.reader.read_all(key).await {
                Ok(read) => batches.extend(read),
                Err(Error::NotFound(_)) => {
                    failures().with_label_values(&["ghost_file"]).inc();
                    tracing::warn!(
                        dataset_id = %dataset.id,
                        segment_id = %segment.id,
                        object_key = key,
                        "Catalog primary Artifact is missing; tombstoning its segment"
                    );
                    missing.push(segment.id.clone());
                }
                Err(error) => {
                    self.tombstone_missing(stream, dataset, &missing).await?;
                    return Err(error);
                }
            }
        }
        if !missing.is_empty() {
            self.tombstone_missing(stream, dataset, &missing).await?;
            return Ok(false);
        }
        if batches.is_empty() {
            return Err(Error::internal(
                "compactor group produced no record batches",
            ));
        }

        let physical_stream =
            crate::infra::intake::physical_schema::project(stream, &dataset.dataset_type);
        let schema = crate::infra::storage::arrow_schema::to_arrow(&physical_stream.schema);
        let aligned = batches
            .iter()
            .map(|batch| {
                crate::infra::storage::arrow_schema::align_batch_to_schema(batch, &schema)
                    .map_err(|error| Error::internal(format!("compactor align schema: {error}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let merged_batch = concat_batches(&schema, &aligned)
            .map_err(|error| Error::internal(format!("concat_batches: {error}")))?;
        let replacements = self
            .writer
            .write_compaction_catalog(&physical_stream, dataset, merged_batch)
            .await?;
        if replacements.len() != 1 || replacements[0].partition != group[0].partition {
            self.writer.delete_catalog_outputs(&replacements).await;
            return Err(Error::internal(
                "one compaction group must produce exactly one replacement in the same partition",
            ));
        }

        let scope = OrganizationScope::new(stream.org_id.clone());
        let command = ReplaceSegments {
            dataset_id: dataset.id.clone(),
            replaced: group.iter().map(|segment| segment.id.clone()).collect(),
            replacements: replacements.clone(),
            gc_not_before_micros: self.gc_not_before(),
        };
        if let Err(error) = self.catalog.replace_segments(&scope, command).await {
            failures().with_label_values(&["replace"]).inc();
            self.writer.delete_catalog_outputs(&replacements).await;
            return Err(error);
        }
        Ok(true)
    }

    async fn tombstone_missing(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        segment_ids: &[SegmentId],
    ) -> Result<()> {
        if segment_ids.is_empty() {
            return Ok(());
        }
        self.catalog
            .tombstone_segments(
                &OrganizationScope::new(stream.org_id.clone()),
                TombstoneSegments {
                    dataset_id: dataset.id.clone(),
                    segment_ids: segment_ids.to_vec(),
                    gc_not_before_micros: self.gc_not_before(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn retention_sweep(&self, stream: &StreamDefinition) -> Result<usize> {
        let now = TimestampMicros::now().0;
        let stream_retention = stream.effective_retention_days(self.settings.retention_days);
        let profile_cutoff =
            now.saturating_sub(i64::from(stream_retention).saturating_mul(DAY_MICROS));
        if stream.stream_type == StreamType::PROFILES {
            match crate::infra::profiles::sweep_expired_archives(
                &self.object_store,
                &stream.org_id,
                profile_cutoff,
            )
            .await
            {
                Ok(removed) if removed > 0 => tracing::debug!(
                    org_id = %stream.org_id,
                    removed,
                    "profiles archive retention sweep"
                ),
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    org_id = %stream.org_id,
                    %error,
                    "profiles archive retention sweep failed"
                ),
            }
        }

        let datasets = self.datasets_for_stream(stream).await?;
        let scope = OrganizationScope::new(stream.org_id.clone());
        let mut total = 0;
        for dataset in datasets {
            let retention_days = dataset
                .storage_policy
                .retention_days
                .unwrap_or(stream_retention);
            let cutoff = now.saturating_sub(i64::from(retention_days).saturating_mul(DAY_MICROS));
            let segments = self
                .snapshot_dataset(
                    stream,
                    &dataset,
                    TimeRange::new(TimestampMicros(i64::MIN), TimestampMicros(cutoff)),
                )
                .await?;
            let segment_ids = segments
                .into_iter()
                .filter(|segment| segment.time_range.end.0 <= cutoff)
                .map(|segment| segment.id)
                .collect::<Vec<_>>();
            if segment_ids.is_empty() {
                continue;
            }
            self.catalog
                .tombstone_segments(
                    &scope,
                    TombstoneSegments {
                        dataset_id: dataset.id.clone(),
                        segment_ids: segment_ids.clone(),
                        gc_not_before_micros: self.gc_not_before(),
                    },
                )
                .await?;
            total += segment_ids.len();
        }
        retention_deleted().inc_by(total as u64);
        Ok(total)
    }

    pub async fn downsample_sweep(&self, stream: &StreamDefinition) -> Result<usize> {
        downsample::sweep(self, stream).await
    }

    async fn datasets_for_stream(&self, stream: &StreamDefinition) -> Result<Vec<PhysicalDataset>> {
        let scope = OrganizationScope::new(stream.org_id.clone());
        Ok(self
            .catalog
            .list_datasets(&scope, &stream.id)
            .await?
            .into_iter()
            .filter(|dataset| dataset.state == DatasetState::Active)
            .collect())
    }

    async fn snapshot_dataset(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        time_range: TimeRange,
    ) -> Result<Vec<DataSegment>> {
        let snapshot = self
            .catalog
            .snapshot(
                &OrganizationScope::new(stream.org_id.clone()),
                DatasetSelection {
                    dataset_ids: vec![dataset.id.clone()],
                    time_range,
                    partition_shard: None,
                },
            )
            .await?;
        snapshot
            .dataset(&dataset.id)
            .map(|dataset| dataset.segments.clone())
            .ok_or_else(|| Error::internal(format!("catalog omitted dataset {}", dataset.id)))
    }

    fn gc_not_before(&self) -> i64 {
        TimestampMicros::now()
            .0
            .saturating_add(self.gc_grace_micros)
    }
}

fn valid_primary(segment: &DataSegment) -> bool {
    segment.primary.role == ArtifactRole::PrimaryData
        && segment.primary.state == ArtifactState::Ready
        && segment.primary.artifact_type.as_str() == type_id::builtin::ARTIFACT_PARQUET
        && segment.primary.format_version == 1
}

#[cfg(test)]
mod tests {
    use arrow::array::{Int64Array, RecordBatch, TimestampMicrosecondArray};
    use futures::StreamExt;
    use object_store::{ObjectStoreExt, memory::InMemory, path::Path};

    use super::*;
    use crate::{
        domain::{
            storage::{
                CommitFlush, FlushProvenance, SequenceRange, WalSequence, WriterEpoch,
                WriterNodeId, primary_dataset_type, type_id,
            },
            stream::{FieldDef, FieldType, Schema},
        },
        infra::{
            intake::{
                ResolvedDataset,
                dataset_resolver::test_support::{StubFileCatalog, test_catalog_and_resolver},
            },
            storage::arrow_schema,
        },
        shared::ids::Id,
    };

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "customer-name".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "value".into(),
                    data_type: FieldType::Int64,
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

    fn batch(stream: &StreamDefinition, start: i64) -> RecordBatch {
        let schema = arrow_schema::to_arrow(&stream.schema);
        let timestamps =
            TimestampMicrosecondArray::from(vec![start, start + 1]).with_timezone("UTC");
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(timestamps),
                Arc::new(Int64Array::from(vec![start, start + 1])),
            ],
        )
        .unwrap()
    }

    async fn publish_two(
        stream: &StreamDefinition,
        catalog: &Arc<StubFileCatalog>,
        dataset: &ResolvedDataset,
        writer: &Arc<ParquetWriter>,
    ) {
        let scope = OrganizationScope::new(stream.org_id.clone());
        for (ordinal, start) in [1_i64, 10].into_iter().enumerate() {
            let sequence_start = WalSequence((ordinal * 2 + 1) as u64);
            let provenance = FlushProvenance::derive(
                WriterNodeId::new("node-a"),
                WriterEpoch(1),
                SequenceRange::new(sequence_start, WalSequence(sequence_start.0 + 1)),
            );
            let segments = writer
                .flush_catalog(stream, &dataset.dataset, &provenance, batch(stream, start))
                .await
                .unwrap();
            catalog
                .commit_flush(
                    &scope,
                    CommitFlush {
                        dataset_id: dataset.dataset.id.clone(),
                        provenance,
                        segments,
                    },
                )
                .await
                .unwrap();
        }
    }

    async fn active_segments(
        stream: &StreamDefinition,
        catalog: &Arc<StubFileCatalog>,
        dataset: &ResolvedDataset,
    ) -> Vec<DataSegment> {
        catalog
            .snapshot(
                &OrganizationScope::new(stream.org_id.clone()),
                DatasetSelection {
                    dataset_ids: vec![dataset.dataset.id.clone()],
                    time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
                    partition_shard: None,
                },
            )
            .await
            .unwrap()
            .datasets
            .remove(0)
            .segments
    }

    fn compactor(
        catalog: Arc<StubFileCatalog>,
        writer: Arc<ParquetWriter>,
        store: Arc<dyn ObjectStore>,
    ) -> Compactor {
        let object_reader = ObjectReader::build(
            store.clone(),
            "local",
            &crate::config::ObjectCacheSettings::default(),
        )
        .unwrap();
        Compactor::new(
            catalog,
            object_reader.clone(),
            Arc::new(PartitionManifestReader::new(
                object_reader.clone(),
                8 * 1024 * 1024,
            )),
            writer,
            store,
            CompactorSettings {
                target_mb: 1,
                ..CompactorSettings::default()
            },
            3600,
        )
    }

    #[tokio::test]
    async fn replaces_catalog_segments_without_deleting_snapshot_objects() {
        let stream = stream();
        let (catalog, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        publish_two(&stream, &catalog, &dataset, &writer).await;
        assert_eq!(store.list(None).collect::<Vec<_>>().await.len(), 2);

        let compactor = compactor(catalog.clone(), writer, store.clone());
        assert_eq!(
            compactor
                .sweep_one(
                    &stream,
                    TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
                )
                .await
                .unwrap(),
            1
        );

        let segments = active_segments(&stream, &catalog, &dataset).await;
        let replacement = &segments[0];
        assert_eq!(segments.len(), 1);
        assert_eq!(replacement.row_count, 4);
        assert!(replacement.sequence_range.is_none());
        assert!(replacement.flush_id.is_none());
        assert!(
            !replacement
                .primary
                .object
                .key
                .as_str()
                .contains(&stream.name)
        );
        assert_eq!(store.list(None).collect::<Vec<_>>().await.len(), 3);
    }

    #[tokio::test]
    async fn replacement_conflict_cleans_only_unpublished_outputs() {
        let stream = stream();
        let (catalog, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        publish_two(&stream, &catalog, &dataset, &writer).await;
        catalog.fail_next_replace();

        let compactor = compactor(catalog.clone(), writer, store.clone());
        assert_eq!(
            compactor
                .sweep_one(
                    &stream,
                    TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
                )
                .await
                .unwrap(),
            0
        );
        assert_eq!(active_segments(&stream, &catalog, &dataset).await.len(), 2);
        assert_eq!(
            store.list(None).collect::<Vec<_>>().await.len(),
            2,
            "the failed replacement output is removed but published inputs remain"
        );
    }

    #[tokio::test]
    async fn missing_primary_artifacts_are_tombstoned_through_catalog() {
        let stream = stream();
        let (catalog, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        publish_two(&stream, &catalog, &dataset, &writer).await;
        for segment in active_segments(&stream, &catalog, &dataset).await {
            store
                .delete(&Path::from(segment.primary.object.key.as_str()))
                .await
                .unwrap();
        }

        let compactor = compactor(catalog.clone(), writer, store);
        assert_eq!(
            compactor
                .sweep_one(
                    &stream,
                    TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
                )
                .await
                .unwrap(),
            0
        );
        assert!(
            active_segments(&stream, &catalog, &dataset)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn downsample_atomically_moves_a_metrics_partition_to_rollup() {
        let mut stream = stream();
        stream.stream_type = StreamType::METRICS;
        stream.name = "metrics".into();
        let (catalog, resolver) = test_catalog_and_resolver();
        let raw = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        publish_two(&stream, &catalog, &raw, &writer).await;

        let mut compactor = compactor(catalog.clone(), writer, store.clone());
        compactor.settings.downsample_after_days = 1;
        compactor.settings.downsample_interval_secs = 3_600;
        assert_eq!(compactor.downsample_sweep(&stream).await.unwrap(), 1);

        let datasets = catalog
            .list_datasets(&OrganizationScope::new(stream.org_id.clone()), &stream.id)
            .await
            .unwrap();
        let rollup = datasets
            .iter()
            .find(|dataset| {
                dataset.dataset_type.as_str() == type_id::builtin::DATASET_METRIC_ROLLUP
            })
            .unwrap();
        let snapshot = catalog
            .snapshot(
                &OrganizationScope::new(stream.org_id.clone()),
                DatasetSelection {
                    dataset_ids: vec![raw.dataset.id.clone(), rollup.id.clone()],
                    time_range: TimeRange::new(
                        TimestampMicros(i64::MIN),
                        TimestampMicros(i64::MAX),
                    ),
                    partition_shard: None,
                },
            )
            .await
            .unwrap();
        assert!(
            snapshot
                .dataset(&raw.dataset.id)
                .unwrap()
                .segments
                .is_empty()
        );
        let rollup_segments = &snapshot.dataset(&rollup.id).unwrap().segments;
        assert_eq!(rollup_segments.len(), 1);
        assert_eq!(rollup_segments[0].row_count, 1);
        assert_eq!(store.list(None).collect::<Vec<_>>().await.len(), 3);
    }
}
