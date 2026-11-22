// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered buffer generation publication through the FileCatalog.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use domain::{
    storage::{
        CommitFlush, FlushId, OrganizationScope, PhysicalDataset, PhysicalDatasetId, WriterEpoch,
        is_stale_writer_epoch,
    },
    stream::{StreamDefinition, StreamType},
};
use futures::{StreamExt, stream};
use kernel::{Error, Result, drain::DrainController};
use tokio::task::JoinHandle;

use super::{FileLayout, IntakeWorker, LAYOUT_TTL, PendingWalCleanup};
use crate::infra::intake::{
    BufferKey, BufferedRecordBatch, DatasetBuffer, FlushInflightGuard, RotationReason,
    inc_flush_error, inc_rotation,
};

/// How long a draining node waits for WAL groups in flight once it stops acknowledging.
const DRAIN_GROUP_WAIT: Duration = Duration::from_secs(5);

/// Why a generation was not published.
pub(super) enum PublishError {
    /// Writing the segment files failed.
    Write(Error),
    /// The catalog refused or failed the commit.
    Commit(Error),
}

impl IntakeWorker {
    #[tracing::instrument(
        name = "worker.intake_flush",
        parent = None,
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.worker.name = "intake_flush",
            molesignal.dataset.id = %key
        )
    )]
    pub async fn flush_one(&self, key: &BufferKey) -> Result<()> {
        self.flush_one_inner(key, None).await
    }

    /// The partition fields and the clustering of `stream`, read from its settings at most once
    /// per [`LAYOUT_TTL`]: flushes every few seconds do not read the settings each time. Only logs
    /// and traces are partitioned, and only logs are clustered. A failed read keeps the last
    /// answer.
    async fn layout(&self, stream: &StreamDefinition) -> FileLayout {
        if !matches!(stream.stream_type, StreamType::LOGS | StreamType::TRACES) {
            return FileLayout::default();
        }

        let cached = self
            .layouts
            .get(&stream.id)
            .map(|entry| entry.value().clone());

        if let Some((read_at, layout)) = &cached
            && read_at.elapsed() < LAYOUT_TTL
        {
            return layout.clone();
        }

        // Saved settings were validated; ones edited around that are not followed, since the
        // shard numbers and the order of their files could not be read back.
        let layout = match self.streams.get_settings(&stream.id).await {
            Ok(settings) => FileLayout {
                sharding: Some(settings.partitions)
                    .filter(|partitions| !partitions.is_empty() && partitions.validate().is_ok()),
                clustering: Some(settings.clustering).filter(|clustering| {
                    stream.stream_type == StreamType::LOGS
                        && !clustering.is_empty()
                        && clustering.validate().is_ok()
                }),
            },
            Err(error) => {
                tracing::warn!(
                    stream_id = %stream.id,
                    %error,
                    "stream settings unreadable; flushing with the last known file layout"
                );

                return cached.map(|(_, layout)| layout).unwrap_or_default();
            }
        };

        self.layouts
            .insert(stream.id.clone(), (Instant::now(), layout.clone()));

        layout
    }

    async fn flush_one_if_due(&self, key: &BufferKey) -> Result<()> {
        self.flush_one_if_due_at(key, Instant::now()).await
    }

    pub(super) async fn flush_one_if_due_at(&self, key: &BufferKey, now: Instant) -> Result<()> {
        self.flush_one_inner(key, Some(now)).await
    }

    async fn flush_one_inner(&self, key: &BufferKey, due_at: Option<Instant>) -> Result<()> {
        let flush_lock = self.flush_lock(key);
        let _single_flight = flush_lock.lock().await;
        self.retry_pending_wal_cleanup(key).await;

        let Some(buffer) = self.buffer.get(key) else {
            return Ok(());
        };

        let stream_definition = match self
            .streams
            .get(
                &buffer.organization_id,
                &buffer.stream_name,
                buffer.stream_type,
            )
            .await
        {
            Ok(stream) if stream.id == buffer.logical_stream_id => stream,
            // 流已被删除，或同名重建成了另一条流：旧数据集里的数据没有去处。
            Ok(_) | Err(Error::NotFound(_)) => {
                self.discard_dataset_gone(&buffer, key, "stream_gone").await;
                return Ok(());
            }
            Err(error) => return Err(error),
        };

        let mut guard = buffer.records().lock().await;

        if guard.is_empty() {
            return Ok(());
        }

        let reason = match due_at {
            None => RotationReason::Forced,
            Some(now) => {
                match guard.rotation_due(self.rotation_threshold(key), self.flush_max_age(), now) {
                    Some(reason) => reason,
                    None => return Ok(()),
                }
            }
        };

        let generation = guard
            .begin_flush()
            .map_err(|error| Error::internal(format!("buffer rotation: {error}")))?
            .ok_or_else(|| Error::internal("non-empty buffer produced no flush generation"))?;

        drop(guard);

        let stream_type = buffer.stream_type.as_str();
        let dataset_type = buffer.dataset.dataset.dataset_type.as_str();
        inc_rotation(stream_type, reason);
        let _inflight = FlushInflightGuard::enter(stream_type);
        let flush_id = generation.flush_id();

        let estimated_raw_bytes = if generation.accounted_size_bytes() > 0 {
            generation.accounted_size_bytes()
        } else {
            generation.approximate_size_bytes()
        };

        // The writer publishes its own rows; rows of an epoch this process was fenced out of are
        // refused here and dropped, the node that claimed that epoch publishes them.
        let publisher = generation.writer.writer_epoch;

        let encoded_size_bytes = match self
            .publish_generation(
                &stream_definition,
                &buffer.dataset.dataset,
                &generation,
                publisher,
            )
            .await
        {
            Ok(bytes) => bytes,
            Err(PublishError::Write(error)) => {
                inc_flush_error("parquet_write");
                self.fail_generation(&buffer, &flush_id).await?;
                return Err(error);
            }
            Err(PublishError::Commit(error)) => {
                inc_flush_error("file_catalog_commit");

                // 物理数据集已从 catalog 删除（例如流被删除后又重建）：这批数据没有去处。
                if matches!(error, Error::NotFound(_)) {
                    self.discard_dataset_gone(&buffer, key, "dataset_gone")
                        .await;
                    return Ok(());
                }

                // The publisher was fenced: these rows are published from the WAL by the node
                // that claimed the stream, and retrying here could never succeed.
                if is_stale_writer_epoch(&error) {
                    self.discard_fenced_writer(&buffer, key, &generation.writer, &error)
                        .await;
                    return Err(error);
                }

                self.fail_generation(&buffer, &flush_id).await?;
                return Err(error);
            }
        };

        let accounted_size_bytes = buffer
            .records()
            .lock()
            .await
            .complete_flush(&flush_id)
            .map_err(|error| Error::internal(format!("buffer retirement: {error}")))?;

        self.buffer.release_memory(accounted_size_bytes);
        self.adaptive_rotation
            .observe(key, dataset_type, estimated_raw_bytes, encoded_size_bytes);

        let high_watermark_seq = generation.sequence_range().end.0;

        match self.cleanup_wal(key, publisher, high_watermark_seq).await {
            Ok(()) => {
                self.pending_wal_cleanup.remove(key);
            }
            Err(error) => {
                self.remember_pending_wal_cleanup(key, publisher, high_watermark_seq);
                tracing::warn!(
                    dataset_id = %key,
                    high_watermark_seq,
                    %error,
                    "catalog flush committed; WAL cleanup deferred"
                );
            }
        }

        Ok(())
    }

    /// Writes `generation`'s segment files and commits them to the catalog as `publisher`. Shared
    /// by the flush of this node's own rows and by takeovers. Returns the encoded bytes.
    pub(super) async fn publish_generation(
        &self,
        stream_definition: &StreamDefinition,
        dataset: &PhysicalDataset,
        generation: &BufferedRecordBatch,
        publisher: WriterEpoch,
    ) -> std::result::Result<u64, PublishError> {
        let provenance = generation.provenance();

        let physical_stream = crate::infra::intake::physical_schema::project(
            stream_definition,
            &dataset.dataset_type,
        );

        let layout = self.layout(stream_definition).await;

        let segments = self
            .parquet_writer
            .flush_catalog_clustered(
                &physical_stream,
                dataset,
                &provenance,
                generation.batch.clone(),
                layout.sharding.as_ref(),
                layout.clustering.as_ref(),
            )
            .await
            .map_err(PublishError::Write)?;

        let encoded_size_bytes = segments
            .iter()
            .flat_map(|segment| segment.artifacts())
            .map(|artifact| artifact.object.size_bytes)
            .sum();

        self.file_catalog
            .commit_flush(
                &OrganizationScope::new(dataset.organization_id.clone()),
                CommitFlush {
                    dataset_id: dataset.id.clone(),
                    provenance,
                    publisher_epoch: publisher,
                    segments,
                },
            )
            .await
            .map_err(PublishError::Commit)?;

        Ok(encoded_size_bytes)
    }

    pub(super) async fn flush_keys(&self, keys: Vec<BufferKey>, force: bool, phase: &'static str) {
        stream::iter(keys)
            .for_each_concurrent(
                self.settings.flush_parallelism.max(1) as usize,
                |key| async move {
                    let result = if force {
                        self.flush_one(&key).await
                    } else {
                        self.flush_one_if_due(&key).await
                    };

                    if let Err(error) = result {
                        tracing::error!(?key, %error, phase, "intake flush failed");
                    }
                },
            )
            .await;
    }

    async fn fail_generation(&self, buffer: &DatasetBuffer, flush_id: &FlushId) -> Result<()> {
        buffer
            .records()
            .lock()
            .await
            .fail_flush(flush_id)
            .map_err(|error| Error::internal(format!("restore failed flush generation: {error}")))
    }

    pub(super) fn remember_pending_wal_cleanup(
        &self,
        key: &BufferKey,
        writer_epoch: WriterEpoch,
        high_watermark_seq: u64,
    ) {
        self.pending_wal_cleanup
            .entry(key.clone())
            .and_modify(|pending| {
                if pending.writer_epoch == writer_epoch {
                    pending.high_watermark_seq = pending.high_watermark_seq.max(high_watermark_seq);
                } else if writer_epoch > pending.writer_epoch {
                    pending.writer_epoch = writer_epoch;
                    pending.high_watermark_seq = high_watermark_seq;
                }
            })
            .or_insert_with(|| PendingWalCleanup {
                dataset_id: key.clone(),
                writer_epoch,
                high_watermark_seq,
            });
    }

    async fn retry_pending_wal_cleanup(&self, key: &BufferKey) {
        let Some(pending) = self.pending_wal_cleanup.get(key).map(|entry| entry.clone()) else {
            return;
        };

        match self
            .cleanup_wal(
                &pending.dataset_id,
                pending.writer_epoch,
                pending.high_watermark_seq,
            )
            .await
        {
            Ok(()) => {
                self.pending_wal_cleanup.remove(key);
            }
            Err(error) => tracing::warn!(
                dataset_id = %pending.dataset_id,
                high_watermark_seq = pending.high_watermark_seq,
                %error,
                "deferred WAL cleanup still pending"
            ),
        }
    }

    /// Deletes the WAL objects of `writer_epoch`'s WAL of `dataset_id` up to `high_watermark_seq`,
    /// which a committed flush covers.
    pub(super) async fn cleanup_wal(
        &self,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        high_watermark_seq: u64,
    ) -> Result<()> {
        self.wal
            .cleanup(dataset_id, writer_epoch, high_watermark_seq)
            .await
            .inspect_err(|_| inc_flush_error("wal_cleanup"))
    }

    /// Interval and explicit wake-up driven flush loop.
    pub fn spawn_flush_loop(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(
                self.settings.flush_interval_secs.max(1) as u64,
            ));

            interval.tick().await;

            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = self.flush_notify.notified() => {}
                    _ = drain_tripwire(&self.drain) => {}
                }

                if let Some(drain) = &self.drain
                    && drain.is_draining()
                {
                    self.flush_keys(self.buffer.snapshot_keys(), true, "drain")
                        .await;
                    self.wal.stop_acknowledging(DRAIN_GROUP_WAIT).await;
                    self.flush_keys(self.buffer.snapshot_keys(), true, "drain_final")
                        .await;
                    self.release_wal().await;
                    drain.mark_drained();
                    tracing::info!("intake drained: pending buffers flushed; flush loop exiting");
                    break;
                }

                self.flush_keys(self.buffer.snapshot_keys(), false, "steady")
                    .await;
            }
        })
    }

    pub fn notify_flush(&self) {
        self.flush_notify.notify_one();
    }
}

async fn drain_tripwire(drain: &Option<Arc<DrainController>>) {
    match drain {
        Some(drain) => {
            while !drain.is_draining() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        None => std::future::pending().await,
    }
}
