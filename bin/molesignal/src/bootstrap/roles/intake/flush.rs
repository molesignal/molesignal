// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered buffer generation publication through the FileCatalog.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{StreamExt, stream};
use tokio::task::JoinHandle;

use super::{IntakeWorker, PendingWalCleanup};
use crate::{
    domain::storage::{CommitFlush, FlushId, OrganizationScope, PhysicalDatasetId},
    infra::intake::{
        BufferKey, DatasetBuffer, FlushInflightGuard, RotationReason, inc_flush_error, inc_rotation,
    },
    shared::{Error, Result, drain::DrainController},
};

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
        let stream_definition = self
            .streams
            .get(
                &buffer.organization_id,
                &buffer.stream_name,
                buffer.stream_type,
            )
            .await?;

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
        let provenance = generation.provenance();
        let flush_id = provenance.flush_id.clone();
        let estimated_raw_bytes = if generation.accounted_size_bytes() > 0 {
            generation.accounted_size_bytes()
        } else {
            generation.approximate_size_bytes()
        };
        let physical_stream = crate::infra::intake::physical_schema::project(
            &stream_definition,
            &buffer.dataset.dataset.dataset_type,
        );
        let segments = match self
            .parquet_writer
            .flush_catalog(
                &physical_stream,
                &buffer.dataset.dataset,
                &provenance,
                generation.batch.clone(),
            )
            .await
        {
            Ok(segments) => segments,
            Err(error) => {
                inc_flush_error("parquet_write");
                self.fail_generation(&buffer, &flush_id).await?;
                return Err(error);
            }
        };
        let encoded_size_bytes = segments
            .iter()
            .flat_map(|segment| segment.artifacts())
            .map(|artifact| artifact.object.size_bytes)
            .sum();

        let scope = OrganizationScope::new(buffer.organization_id.clone());
        if let Err(error) = self
            .file_catalog
            .commit_flush(
                &scope,
                CommitFlush {
                    dataset_id: key.clone(),
                    provenance,
                    segments,
                },
            )
            .await
        {
            inc_flush_error("file_catalog_commit");
            self.fail_generation(&buffer, &flush_id).await?;
            return Err(error);
        }

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
        match self.cleanup_wal(key, high_watermark_seq).await {
            Ok(()) => {
                self.pending_wal_cleanup.remove(key);
            }
            Err(error) => {
                self.remember_pending_wal_cleanup(key, high_watermark_seq);
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

    fn remember_pending_wal_cleanup(&self, key: &BufferKey, high_watermark_seq: u64) {
        self.pending_wal_cleanup
            .entry(key.clone())
            .and_modify(|pending| {
                pending.high_watermark_seq = pending.high_watermark_seq.max(high_watermark_seq);
            })
            .or_insert_with(|| PendingWalCleanup {
                dataset_id: key.clone(),
                high_watermark_seq,
            });
    }

    async fn retry_pending_wal_cleanup(&self, key: &BufferKey) {
        let Some(pending) = self.pending_wal_cleanup.get(key).map(|entry| entry.clone()) else {
            return;
        };
        match self
            .cleanup_wal(&pending.dataset_id, pending.high_watermark_seq)
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

    async fn cleanup_wal(
        &self,
        dataset_id: &PhysicalDatasetId,
        high_watermark_seq: u64,
    ) -> Result<()> {
        if let Err(error) = self.wal.seal_active(dataset_id).await {
            inc_flush_error("wal_seal");
            return Err(Error::internal(format!("wal seal_active: {error}")));
        }
        if let Err(error) = self
            .wal
            .truncate_up_to(dataset_id, high_watermark_seq)
            .await
        {
            inc_flush_error("wal_truncate");
            return Err(Error::internal(format!("wal truncate: {error}")));
        }
        Ok(())
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
                    self.flush_keys(self.buffer.snapshot_keys(), true, "drain_final")
                        .await;
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
