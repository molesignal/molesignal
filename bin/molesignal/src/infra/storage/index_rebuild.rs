// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use arrow::compute::concat_batches;
use bytes::Bytes;
use futures::{StreamExt, stream};
use object_store::{ObjectStore, PutMode, PutOptions, PutPayload, path::Path};
use prometheus::{Histogram, IntCounter};

use crate::{
    config::{GarbageCollectionSettings, IndexMaintenanceSettings},
    domain::{
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ArtifactUpdate,
            FileCatalog, IndexRebuildTask, ObjectChecksum, OrganizationScope, StoredObject,
            UpdateArtifact, type_id,
        },
        stream::StreamRepository,
    },
    infra::storage::{
        layout::StorageLayout,
        object_reader::ObjectReader,
        parquet::{reader::ParquetReader, writer::build_tantivy_bytes_for_batch},
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub struct IndexRebuildWorker {
    catalog: Arc<dyn FileCatalog>,
    streams: Arc<dyn StreamRepository>,
    objects: Arc<ObjectReader>,
    origin: Arc<dyn ObjectStore>,
    settings: IndexMaintenanceSettings,
    gc_grace_micros: i64,
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
        let tasks = self
            .catalog
            .index_rebuild_tasks(&scope, retry_before, self.settings.batch_size)
            .await?;
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

    async fn rebuild(&self, scope: &OrganizationScope, task: IndexRebuildTask) -> Result<()> {
        let started = Instant::now();
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
        let batches = ParquetReader::new(self.objects.store())
            .read_all(task.segment.primary.object.key.as_str())
            .await?;
        let schema = batches
            .first()
            .map(|batch| batch.schema())
            .ok_or_else(|| Error::internal("primary artifact contained no record batches"))?;
        let batch = concat_batches(&schema, &batches)
            .map_err(|error| Error::internal(format!("concat index source batches: {error}")))?;
        let bytes = build_tantivy_bytes_for_batch(&stream, &batch)?
            .ok_or_else(|| Error::invalid("stream has no supported indexed columns"))?;
        let artifact_id = ArtifactId::generate();
        let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);
        let key = StorageLayout::artifact_key(
            &scope.organization_id,
            &task.dataset.id,
            &task.segment.partition,
            &task.segment.id,
            &artifact_id,
            &artifact_type,
        );
        let bytes = Bytes::from(bytes);
        let put = self
            .origin
            .put_opts(
                &Path::from(key.as_str()),
                PutPayload::from(bytes.clone()),
                PutOptions {
                    mode: PutMode::Create,
                    ..PutOptions::default()
                },
            )
            .await
            .map_err(|error| Error::internal(format!("upload rebuilt index: {error}")))?;
        Ok(Artifact {
            id: artifact_id,
            role: ArtifactRole::Index,
            artifact_type,
            format_version: task.artifact.format_version,
            object: StoredObject {
                key,
                size_bytes: bytes.len() as u64,
                checksum: ObjectChecksum::from_string(format!(
                    "b3:{}",
                    blake3::hash(&bytes).to_hex()
                )),
                etag: put.e_tag,
            },
            source_artifact_id: Some(task.segment.primary.id.clone()),
            source_checksum: Some(task.segment.primary.object.checksum.clone()),
            schema_fingerprint: task.segment.primary.schema_fingerprint,
            state: ArtifactState::Ready,
            failure_reason: None,
        })
    }
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
