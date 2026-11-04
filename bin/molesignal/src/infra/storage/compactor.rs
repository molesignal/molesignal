// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Catalog-native compaction and retention orchestration.
//!
//! Inputs are fixed FileCatalog snapshots. Replacement publication is atomic, and retired
//! Artifact objects are queued for delayed GC rather than deleted while older queries may still
//! reference them. No object key is generated or inferred in this module.

use std::sync::{Arc, OnceLock};

use arrow::array::RecordBatch;
use futures::stream::BoxStream;
use object_store::ObjectStore;
use prometheus::{IntCounter, IntCounterVec};

use super::parquet::{partition::storage_ordering, reader::ParquetReader, writer::ParquetWriter};
use crate::{
    config::CompactorSettings,
    domain::{
        storage::{
            ArtifactRole, ArtifactState, DataSegment, DatasetSelection, DatasetState, FileCatalog,
            OrganizationScope, Partition, PhysicalDataset, ReplaceSegments, SegmentId,
            TombstoneSegments, type_id,
        },
        stream::{Downsample, StreamDefinition, StreamType},
    },
    infra::storage::{manifest::PartitionManifestReader, object_reader::ObjectReader},
    shared::{
        Error, Result,
        metrics::{register_int_counter, register_int_counter_vec},
        time::{TimeRange, TimestampMicros},
    },
};

mod dedup;
mod downsample;
mod partition;
mod sorted;

use partition::{build_groups, common_partitions, daily_groups, validate_day, validate_group};

static FAILURES: OnceLock<IntCounterVec> = OnceLock::new();
static MERGED: OnceLock<IntCounter> = OnceLock::new();
static RETENTION_DELETED: OnceLock<IntCounter> = OnceLock::new();
const DAY_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
/// Compressed bytes of one merge input read at a time.
const INPUT_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

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
    /// Merges the small files of `stream` in `time_range`; `daily_merge` is the stream's own
    /// setting to merge each small finished day into one file as well.
    pub async fn sweep_one(
        &self,
        stream: &StreamDefinition,
        time_range: TimeRange,
        daily_merge: bool,
    ) -> Result<usize> {
        let datasets = self.datasets_for_stream(stream).await?;
        let mut total = 0;

        for dataset in &datasets {
            total += self
                .sweep_dataset(stream, dataset, time_range, daily_merge)
                .await?;
        }

        Ok(total)
    }

    async fn sweep_dataset(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        time_range: TimeRange,
        daily_merge: bool,
    ) -> Result<usize> {
        let target_bytes = self.settings.target_bytes();
        let segments = self.snapshot_dataset(stream, dataset, time_range).await?;

        let merges_days =
            daily_merge && matches!(stream.stream_type, StreamType::LOGS | StreamType::TRACES);

        let days = merges_days.then(|| segments.clone());
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
        let hours_changed = !groups.is_empty();

        for group in groups {
            let merging = validate_group(&group, &dataset.id);

            let outcome = match merging {
                Ok(partition) => self.merge_into(stream, dataset, &group, partition).await,
                Err(error) => Err(error),
            };

            merged_groups += self.count_merge(stream, dataset, group.len(), outcome);
        }

        if let Some(segments) = days {
            // Merging the hours replaced or tombstoned some of the segments read above.
            let segments = if hours_changed {
                self.snapshot_dataset(stream, dataset, time_range).await?
            } else {
                segments
            };

            merged_groups += self.merge_days(stream, dataset, segments).await;
        }

        Ok(merged_groups)
    }

    /// Merges each small finished day of `segments` into one Segment, after the hours are merged.
    async fn merge_days(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        segments: Vec<DataSegment>,
    ) -> usize {
        let segments = segments.into_iter().filter(valid_primary);
        let days = daily_groups(
            segments,
            self.settings.target_bytes(),
            TimestampMicros::now().0,
        );
        let mut merged_days = 0;

        for (day, group) in days {
            let outcome = match validate_day(&group, &dataset.id, day) {
                Ok(()) => self.merge_into(stream, dataset, &group, day).await,
                Err(error) => Err(error),
            };

            merged_days += self.count_merge(stream, dataset, group.len(), outcome);
        }

        merged_days
    }

    /// 1 when a merge happened; a failed merge is logged and retried by the next sweep.
    fn count_merge(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        group_size: usize,
        outcome: Result<bool>,
    ) -> usize {
        match outcome {
            Ok(true) => {
                merged().inc();
                1
            }
            Ok(false) => 0,
            Err(error) => {
                tracing::warn!(
                    stream = %stream.name,
                    dataset_id = %dataset.id,
                    group_size,
                    %error,
                    "compactor merge group failed; will retry next sweep"
                );
                0
            }
        }
    }

    /// Merges `group` into one Segment of `partition`. Each input is read a window of rows at a
    /// time and the output is written as it is merged, so a merge holds about
    /// [`INPUT_WINDOW_BYTES`] per input, one output row group and a few upload parts, however
    /// large `target_size` is.
    #[tracing::instrument(
        name = "compactor.merge",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.compactor.file_count = group.len()
        )
    )]
    async fn merge_into(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        group: &[DataSegment],
        partition: Partition,
    ) -> Result<bool> {
        let physical_stream =
            crate::infra::intake::physical_schema::project(stream, &dataset.dataset_type);
        let schema = crate::infra::storage::arrow_schema::to_arrow(&physical_stream.schema);

        let ordering = storage_ordering(schema.as_ref(), Some(&dataset.dataset_type))
            .ok_or_else(|| Error::internal("a physical dataset has a storage order"))?;

        let opened =
            futures::future::join_all(group.iter().map(|segment| self.open_input(stream, segment)))
                .await;

        let mut inputs = Vec::with_capacity(group.len());
        let mut missing = Vec::new();

        for (segment, opened) in group.iter().zip(opened) {
            match opened {
                Ok(input) => inputs.push(input),
                Err(Error::NotFound(_)) => {
                    failures().with_label_values(&["ghost_file"]).inc();
                    tracing::warn!(
                        dataset_id = %dataset.id,
                        segment_id = %segment.id,
                        object_key = segment.primary.object.key.as_str(),
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

        let rows = sorted::merge(
            inputs,
            schema.clone(),
            &ordering,
            dedup::entity_columns(&schema),
        )?;

        let mut replacement = self
            .writer
            .write_sorted_segment(&physical_stream, dataset, partition, schema, rows)
            .await?;

        replacement.column_stats.partitions = common_partitions(group);

        let scope = OrganizationScope::new(stream.org_id.clone());

        let command = ReplaceSegments {
            dataset_id: dataset.id.clone(),
            replaced: group.iter().map(|segment| segment.id.clone()).collect(),
            replacements: vec![replacement.clone()],
            gc_not_before_micros: self.gc_not_before(),
        };

        if let Err(error) = self.catalog.replace_segments(&scope, command).await {
            failures().with_label_values(&["replace"]).inc();
            self.writer
                .delete_catalog_outputs(std::slice::from_ref(&replacement))
                .await;
            return Err(error);
        }

        Ok(true)
    }

    async fn open_input(
        &self,
        stream: &StreamDefinition,
        segment: &DataSegment,
    ) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        self.object_reader
            .register_segment(&stream.org_id, segment)?;

        self.reader
            .stream_windows_from_store(
                self.object_reader.store(),
                segment.primary.object.key.as_str(),
                INPUT_WINDOW_BYTES,
            )
            .await
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

    /// 删掉过了保留期的数据。`default_retention_days` 是数据流没有设置保留天数时的天数（平台
    /// 设置）。
    pub async fn retention_sweep(
        &self,
        stream: &StreamDefinition,
        default_retention_days: u32,
    ) -> Result<usize> {
        let now = TimestampMicros::now().0;
        let stream_retention = stream.effective_retention_days(default_retention_days);
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

    pub async fn downsample_sweep(
        &self,
        stream: &StreamDefinition,
        policy: Downsample,
    ) -> Result<usize> {
        downsample::sweep(self, stream, policy).await
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
    use std::collections::BTreeMap;

    use arrow::array::{Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray};
    use futures::StreamExt;
    use object_store::{ObjectStoreExt, memory::InMemory, path::Path};

    use super::*;
    use crate::{
        domain::{
            storage::{
                ArtifactRole, ArtifactState, CommitFlush, FlushProvenance, SequenceRange,
                WalSequence, WriterEpoch, WriterNodeId, primary_dataset_type, type_id,
            },
            stream::{FieldDef, FieldType, Schema, StreamIndexType},
        },
        infra::{
            intake::{
                ResolvedDataset,
                dataset_resolver::test_support::{StubFileCatalog, test_catalog_and_resolver},
            },
            search::tantivy_index::{BarQuery, BarTally, TantivyArchiveOpener},
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
                .flush_catalog(
                    stream,
                    &dataset.dataset,
                    &provenance,
                    batch(stream, start),
                    None,
                )
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
                target_size: 1,
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
                    true,
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
                    true,
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
                    true,
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

    fn level_batch(stream: &StreamDefinition, times: &[i64]) -> RecordBatch {
        let levels = times
            .iter()
            .map(|time| if time % 3 == 0 { "error" } else { "info" })
            .collect::<Vec<_>>();

        RecordBatch::try_new(
            arrow_schema::to_arrow(&stream.schema),
            vec![
                Arc::new(TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC")),
                Arc::new(Int64Array::from(times.to_vec())),
                Arc::new(StringArray::from(levels)),
            ],
        )
        .unwrap()
    }

    /// The merged Segment holds every input row once, in the storage order, and its index counts
    /// the rows by time and level as the rows themselves would be counted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn merges_interleaved_inputs_in_the_storage_order_with_an_index() {
        let mut stream = stream();

        stream.schema.fields.push(FieldDef {
            name: "level".into(),
            data_type: FieldType::Utf8,
            nullable: true,
            index_type: Some(StreamIndexType::Exact),
            indexed: true,
            encrypted: false,
            exact: true,
        });

        let (catalog, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let scope = OrganizationScope::new(stream.org_id.clone());

        // Three inputs whose times interleave: 1, 4, 7, … and 2, 5, 8, … and 3, 6, 9, …
        for input in 0..3_u64 {
            let times = (0..20)
                .map(|step| 1 + input as i64 + 3 * step)
                .collect::<Vec<_>>();

            let provenance = FlushProvenance::derive(
                WriterNodeId::new("node-a"),
                WriterEpoch(1),
                SequenceRange::new(WalSequence(input * 2 + 1), WalSequence(input * 2 + 2)),
            );

            let segments = writer
                .flush_catalog(
                    &stream,
                    &dataset.dataset,
                    &provenance,
                    level_batch(&stream, &times),
                    None,
                )
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

        let compactor = compactor(catalog.clone(), writer, store.clone());

        assert_eq!(
            compactor
                .sweep_one(
                    &stream,
                    TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
                    true,
                )
                .await
                .unwrap(),
            1
        );

        let segments = active_segments(&stream, &catalog, &dataset).await;
        let merged = &segments[0];
        assert_eq!(segments.len(), 1);
        assert_eq!(merged.row_count, 60);
        assert_eq!(
            (merged.time_range.start.0, merged.time_range.end.0),
            (1, 60)
        );

        let batches = ParquetReader::new(store.clone())
            .read_all(merged.primary.object.key.as_str())
            .await
            .unwrap();

        let times = batches
            .iter()
            .flat_map(|batch| {
                let column = batch.column(0).as_any();
                let times = column.downcast_ref::<TimestampMicrosecondArray>().unwrap();
                times.values().to_vec()
            })
            .collect::<Vec<_>>();

        assert_eq!(times, (1..=60).rev().collect::<Vec<i64>>());

        let index = merged
            .auxiliaries
            .iter()
            .find(|artifact| artifact.role == ArtifactRole::Index)
            .expect("the merged Segment is indexed");

        assert_eq!(index.state, ArtifactState::Ready);

        let handle = TantivyArchiveOpener::open_from_object_store(
            store.clone(),
            Path::from(index.object.key.as_str()),
            index.object.size_bytes,
        )
        .await
        .unwrap();

        let query = BarQuery {
            terms: Vec::new(),
            contains: Vec::new(),
            window: 0..100,
            origin: 0,
            width: 10,
            errors: Some("level".into()),
        };

        let bars = tokio::task::spawn_blocking(move || handle.count_bars(&query).unwrap())
            .await
            .unwrap();

        let mut expected: BTreeMap<i64, BarTally> = BTreeMap::new();

        for time in 1..=60_i64 {
            let bar = expected.entry(time / 10).or_default();
            bar.total += 1;
            bar.errors += u64::from(time % 3 == 0);
        }

        assert_eq!(bars, Some(expected));
    }

    /// A finished day whose hours hold one small file each becomes one file for the whole day.
    #[tokio::test]
    async fn merges_the_hours_of_a_small_finished_day() {
        let stream = stream();
        let (catalog, resolver) = test_catalog_and_resolver();
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let scope = OrganizationScope::new(stream.org_id.clone());
        let hour = 3_600_000_000_i64;

        for (ordinal, start) in [0, hour, 5 * hour].into_iter().enumerate() {
            let sequence_start = WalSequence(ordinal as u64 * 2 + 1);

            let provenance = FlushProvenance::derive(
                WriterNodeId::new("node-a"),
                WriterEpoch(1),
                SequenceRange::new(sequence_start, WalSequence(sequence_start.0 + 1)),
            );

            let segments = writer
                .flush_catalog(
                    &stream,
                    &dataset.dataset,
                    &provenance,
                    batch(&stream, start + 1),
                    None,
                )
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

        let day = TimeRange::new(TimestampMicros(0), TimestampMicros(24 * hour));
        let compactor = compactor(catalog.clone(), writer, store);

        let segments = || async {
            catalog
                .snapshot(
                    &scope,
                    DatasetSelection {
                        dataset_ids: vec![dataset.dataset.id.clone()],
                        time_range: day,
                        partition_shard: None,
                    },
                )
                .await
                .unwrap()
                .datasets
                .remove(0)
                .segments
        };

        // A stream that turned the daily merge off keeps its hours.
        assert_eq!(compactor.sweep_one(&stream, day, false).await.unwrap(), 0);
        assert_eq!(segments().await.len(), 3);

        assert_eq!(compactor.sweep_one(&stream, day, true).await.unwrap(), 1);

        let segments = segments().await;

        assert_eq!(segments.len(), 1);
        assert_eq!(
            (
                segments[0].partition.start_micros,
                segments[0].partition.end_micros
            ),
            (0, 24 * hour)
        );
        assert_eq!(segments[0].row_count, 6);
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

        let compactor = compactor(catalog.clone(), writer, store.clone());

        let policy = Downsample {
            after_days: 1,
            interval_secs: 3_600,
        };

        assert_eq!(
            compactor
                .downsample_sweep(&stream, Downsample::default())
                .await
                .unwrap(),
            0,
            "a stream that does not downsample keeps its samples"
        );
        assert_eq!(
            compactor.downsample_sweep(&stream, policy).await.unwrap(),
            1
        );

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
