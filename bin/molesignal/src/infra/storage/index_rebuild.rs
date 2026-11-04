// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use futures::{StreamExt, stream};
use object_store::ObjectStore;
use parking_lot::Mutex;
use prometheus::{Histogram, IntCounter};

mod reindex;

pub(crate) use reindex::reindex_segment;

use crate::{
    config::{GarbageCollectionSettings, IndexMaintenanceSettings},
    domain::{
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ArtifactUpdate,
            DataSegment, FileCatalog, IndexRebuildTask, OrganizationScope, PhysicalDatasetId,
            UpdateArtifact, type_id,
        },
        stream::{StreamDefinition, StreamRepository},
    },
    infra::{
        search::tantivy_index::{self, TIMESTAMP_FIELD, TantivyArchiveBuilder},
        storage::{
            layout::StorageLayout,
            object_reader::ObjectReader,
            parquet::{
                reader::{ParquetReader, ReadOptions},
                writer::{add_batch_docs, upload_tantivy_archive},
            },
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// Stale indexes whose rebuild failed are remembered at most this many at once.
const MAX_REMEMBERED_FAILURES: usize = 1_000;

pub struct IndexRebuildWorker {
    catalog: Arc<dyn FileCatalog>,
    streams: Arc<dyn StreamRepository>,
    objects: Arc<ObjectReader>,
    origin: Arc<dyn ObjectStore>,
    settings: IndexMaintenanceSettings,
    gc_grace_micros: i64,
    /// Ready indexes of an older format whose rebuild failed, and when: they stay as they are and
    /// are skipped until `retry_after_secs` pass.
    failed_upgrades: Mutex<HashMap<ArtifactId, Instant>>,
}

impl IndexRebuildWorker {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        streams: Arc<dyn StreamRepository>,
        objects: Arc<ObjectReader>,
        origin: Arc<dyn ObjectStore>,
        settings: IndexMaintenanceSettings,
        gc: &GarbageCollectionSettings,
    ) -> Self {
        Self {
            catalog,
            streams,
            objects,
            origin,
            settings,
            gc_grace_micros: i64::from(gc.grace_period_secs) * 1_000_000,
            failed_upgrades: Mutex::new(HashMap::new()),
        }
    }

    pub fn settings(&self) -> &IndexMaintenanceSettings {
        &self.settings
    }

    pub async fn run_scope(&self, scope: OrganizationScope) -> Result<usize> {
        if !self.settings.enabled {
            return Ok(0);
        }

        let retry_before = TimestampMicros::now()
            .0
            .saturating_sub(i64::from(self.settings.retry_after_secs) * 1_000_000);

        let mut tasks = self
            .catalog
            .index_rebuild_tasks(&scope, retry_before, self.settings.batch_size)
            .await?;

        // With room left after the missing indexes, the ones in an older format are rebuilt in
        // the current one.
        let room = (self.settings.batch_size as usize).saturating_sub(tasks.len());

        if room > 0 {
            let skip = self.recent_failures();

            tasks.extend(
                self.catalog
                    .stale_index_tasks(
                        &scope,
                        type_id::builtin::ARTIFACT_TANTIVY,
                        tantivy_index::FORMAT_VERSION,
                        &skip,
                        room as u32,
                    )
                    .await?,
            );
        }

        let completed = stream::iter(tasks)
            .map(|task| self.rebuild(&scope, task))
            .buffer_unordered(self.settings.max_concurrency.max(1))
            .fold(0_usize, |count, result| async move {
                match result {
                    Ok(()) => count + 1,
                    Err(error) => {
                        tracing::warn!(error = %error, "index rebuild task failed");
                        count
                    }
                }
            })
            .await;

        Ok(completed)
    }

    /// Stale indexes whose rebuild failed within `retry_after_secs`, forgetting older failures.
    fn recent_failures(&self) -> Vec<ArtifactId> {
        let wait = Duration::from_secs(u64::from(self.settings.retry_after_secs));
        let mut failed = self.failed_upgrades.lock();

        failed.retain(|_, at| at.elapsed() < wait);
        failed.keys().cloned().collect()
    }

    fn remember_failure(&self, artifact_id: &ArtifactId) {
        let mut failed = self.failed_upgrades.lock();

        if failed.len() < MAX_REMEMBERED_FAILURES {
            failed.insert(artifact_id.clone(), Instant::now());
        }
    }

    async fn rebuild(&self, scope: &OrganizationScope, task: IndexRebuildTask) -> Result<()> {
        let started = Instant::now();
        // A ready index of an older format: it keeps serving until its replacement is in.
        let stale = task.artifact.state == ArtifactState::Ready;
        let artifact_id = task.artifact.id.clone();

        if task.artifact.state == ArtifactState::Failed {
            self.catalog
                .update_artifact(
                    scope,
                    UpdateArtifact {
                        dataset_id: task.dataset.id.clone(),
                        segment_id: task.segment.id.clone(),
                        artifact_id: task.artifact.id.clone(),
                        update: ArtifactUpdate::Retry,
                    },
                )
                .await?;
        }

        let result = self.build_uploaded(scope, &task).await;

        let replacement = match result {
            Ok(replacement) => replacement,
            Err(error) => {
                failed_total().inc();

                if stale {
                    self.remember_failure(&artifact_id);
                    return Err(error);
                }

                let reason = truncate_error(&error.to_string());

                let _ = self
                    .catalog
                    .update_artifact(
                        scope,
                        UpdateArtifact {
                            dataset_id: task.dataset.id.clone(),
                            segment_id: task.segment.id.clone(),
                            artifact_id: task.artifact.id.clone(),
                            update: ArtifactUpdate::MarkFailed { reason },
                        },
                    )
                    .await;

                return Err(error);
            }
        };

        let replacement_key = replacement.object.key.clone();

        let update = self
            .catalog
            .update_artifact(
                scope,
                UpdateArtifact {
                    dataset_id: task.dataset.id,
                    segment_id: task.segment.id,
                    artifact_id: task.artifact.id,
                    update: ArtifactUpdate::Replace {
                        replacement,
                        gc_not_before_micros: TimestampMicros::now()
                            .0
                            .saturating_add(self.gc_grace_micros),
                    },
                },
            )
            .await;

        if let Err(error) = update {
            if stale {
                self.remember_failure(&artifact_id);
            }

            let _ = self
                .catalog
                .enqueue_orphan(
                    scope,
                    &replacement_key,
                    TimestampMicros::now()
                        .0
                        .saturating_add(self.gc_grace_micros),
                )
                .await;

            return Err(error);
        }

        built_total().inc();
        build_duration().observe(started.elapsed().as_secs_f64());
        Ok(())
    }

    async fn build_uploaded(
        &self,
        scope: &OrganizationScope,
        task: &IndexRebuildTask,
    ) -> Result<Artifact> {
        if task.artifact.role != ArtifactRole::Index
            || task.artifact.artifact_type.as_str() != type_id::builtin::ARTIFACT_TANTIVY
            || task.artifact.source_artifact_id.as_ref() != Some(&task.segment.primary.id)
            || task.artifact.source_checksum.as_ref() != Some(&task.segment.primary.object.checksum)
        {
            return Err(Error::invalid("unsupported or stale index rebuild task"));
        }

        let stream = self
            .streams
            .get_by_id(&task.dataset.logical_stream_id)
            .await?;

        if stream.org_id != scope.organization_id {
            return Err(Error::invalid(
                "index rebuild stream crossed organization scope",
            ));
        }

        self.objects
            .register_segment(&scope.organization_id, &task.segment)?;

        build_index_artifact(
            self.objects.store(),
            self.origin.clone(),
            &stream,
            &scope.organization_id,
            &task.dataset.id,
            &task.segment,
        )
        .await
    }
}

/// Builds the Tantivy archive of `segment` in the current format from its primary Parquet file and
/// uploads it under a new artifact id. The file is read from `source` a row group at a time, only
/// the columns the index is built from, and the archive goes through a local file: neither the
/// file nor its index is ever held in memory whole.
pub(crate) async fn build_index_artifact(
    source: Arc<dyn ObjectStore>,
    origin: Arc<dyn ObjectStore>,
    stream: &StreamDefinition,
    organization_id: &Id,
    dataset_id: &PhysicalDatasetId,
    segment: &DataSegment,
) -> Result<Artifact> {
    let mut builder = TantivyArchiveBuilder::try_new(stream)
        .map_err(|error| Error::internal(format!("tantivy builder: {error}")))?
        .ok_or_else(|| Error::invalid("stream has no supported indexed columns"))?;

    let columns = builder
        .fields()
        .keys()
        .cloned()
        .chain([TIMESTAMP_FIELD.to_owned()])
        .collect::<Vec<_>>();

    let columns = columns.iter().map(String::as_str).collect::<Vec<_>>();

    let mut batches = ParquetReader::new(source.clone())
        .stream_from_store(
            source,
            segment.primary.object.key.as_str(),
            ReadOptions::new().with_columns(&columns),
        )
        .await?;

    let mut rows = 0_usize;

    while let Some(batch) = batches.next().await {
        let batch = batch?;
        rows += batch.num_rows();

        if !add_batch_docs(&mut builder, stream, &batch)? {
            return Err(Error::invalid("stream has no supported indexed columns"));
        }
    }

    if rows == 0 {
        return Err(Error::internal(
            "primary artifact contained no record batches",
        ));
    }

    upload_archive(
        origin,
        builder,
        stream,
        organization_id,
        dataset_id,
        segment,
    )
    .await
}

/// Uploads the archive `builder` holds as the Tantivy index of `segment`, under a new artifact id.
pub(crate) async fn upload_archive(
    origin: Arc<dyn ObjectStore>,
    builder: TantivyArchiveBuilder,
    stream: &StreamDefinition,
    organization_id: &Id,
    dataset_id: &PhysicalDatasetId,
    segment: &DataSegment,
) -> Result<Artifact> {
    let artifact_id = ArtifactId::generate();
    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);

    let key = StorageLayout::artifact_key(
        organization_id,
        stream.stream_type,
        dataset_id,
        &segment.partition,
        &segment.id,
        &artifact_id,
        &artifact_type,
    );

    let object = upload_tantivy_archive(origin, builder, key)
        .await
        .map_err(|error| Error::internal(format!("upload rebuilt index: {error}")))?;

    Ok(Artifact {
        id: artifact_id,
        role: ArtifactRole::Index,
        artifact_type,
        format_version: tantivy_index::FORMAT_VERSION,
        object,
        source_artifact_id: Some(segment.primary.id.clone()),
        source_checksum: Some(segment.primary.object.checksum.clone()),
        schema_fingerprint: segment.primary.schema_fingerprint,
        state: ArtifactState::Ready,
        failure_reason: None,
    })
}

fn truncate_error(error: &str) -> String {
    error.chars().take(1024).collect()
}

fn build_duration() -> &'static Histogram {
    static METRIC: OnceLock<Histogram> = OnceLock::new();

    METRIC.get_or_init(|| {
        crate::shared::metrics::register_histogram(
            "index_build_duration_seconds",
            "Background index rebuild duration",
            vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 15.0, 60.0],
        )
    })
}

fn built_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();

    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "index_rebuilt_total",
            "Optional index artifacts rebuilt successfully",
        )
    })
}

fn failed_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();

    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "index_rebuild_failed_total",
            "Optional index artifact rebuild failures",
        )
    })
}
