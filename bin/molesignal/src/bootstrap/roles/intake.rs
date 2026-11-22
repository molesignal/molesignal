// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Intake role composition: appends acknowledged once in object storage, ordered catalog
//! publication, takeover of lapsed writers, and release on drain.

mod columnar;
mod discard;
mod flush;
mod release;
mod takeover;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use dashmap::DashMap;
use domain::{
    intake::{IntakeBatch, IntakeResult, IntakeSink},
    storage::{DatasetTypeId, FileCatalog, PhysicalDatasetId, WriterEpoch},
    stream::{Clustering, FieldPartitions, StreamRepository},
};
use kernel::{Result, drain::DrainController, health::Probe, ids::Id};
use postgres::cipher::FieldKeyService;
use settings::IntakeSettings;
use tokio::sync::{Mutex, Notify};

use crate::infra::{
    intake::{
        AdaptiveRotation, BufferKey, BufferPool, DatasetResolver, DurableIntakeSink, ObjectWal,
    },
    storage::parquet::writer::ParquetWriter,
};

/// WAL objects a committed flush covers whose deletion still has to be retried.
#[derive(Debug, Clone)]
struct PendingWalCleanup {
    dataset_id: PhysicalDatasetId,
    writer_epoch: WriterEpoch,
    high_watermark_seq: u64,
}

const DEFAULT_TAKEOVER_BYTE_CAP: usize = 512 * 1024 * 1024;

/// How long a stream's file layout read from its settings is used before it is read again.
const LAYOUT_TTL: Duration = Duration::from_secs(30);

/// How a flush lays out a stream's rows in files: the shards it splits them into and the order of
/// the rows inside each file.
#[derive(Debug, Clone, Default)]
struct FileLayout {
    sharding: Option<FieldPartitions>,
    clustering: Option<Clustering>,
}

pub struct IntakeWorker {
    sink: Arc<DurableIntakeSink>,
    wal: Arc<ObjectWal>,
    buffer: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    datasets: Arc<DatasetResolver>,
    file_catalog: Arc<dyn FileCatalog>,
    parquet_writer: Arc<ParquetWriter>,
    settings: IntakeSettings,
    flush_notify: Arc<Notify>,
    /// One publication at a time per physical dataset; distinct datasets may publish in parallel.
    flush_locks: DashMap<BufferKey, Arc<Mutex<()>>>,
    /// Catalog committed, while deleting the WAL objects it covers still needs retrying.
    pending_wal_cleanup: DashMap<BufferKey, PendingWalCleanup>,
    adaptive_rotation: AdaptiveRotation,
    drain: Option<Arc<DrainController>>,
    /// The most a takeover holds in memory before it publishes a segment.
    takeover_byte_cap: usize,
    /// Stream id → when its settings were read, and the file layout they named.
    layouts: DashMap<Id, (Instant, FileLayout)>,
}

impl IntakeWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wal: Arc<ObjectWal>,
        buffer: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
        datasets: Arc<DatasetResolver>,
        file_catalog: Arc<dyn FileCatalog>,
        parquet_writer: Arc<ParquetWriter>,
        probe: Arc<Probe>,
        settings: IntakeSettings,
    ) -> Self {
        let buffer_max_bytes = (settings.buffer_max_size as usize).saturating_mul(1024 * 1024);
        let adaptive_rotation = AdaptiveRotation::new(&settings.rotation, buffer_max_bytes);

        let sink = Arc::new(DurableIntakeSink::new(
            wal.clone(),
            buffer.clone(),
            streams.clone(),
            datasets.clone(),
        ));

        // Nothing to replay: unflushed data of earlier processes is in object storage and taken
        // over in the background, so this node can take writes at once.
        probe.set_replay_done(true);

        Self {
            sink,
            wal,
            buffer,
            streams,
            datasets,
            file_catalog,
            parquet_writer,
            settings,
            flush_notify: Arc::new(Notify::new()),
            flush_locks: DashMap::new(),
            pending_wal_cleanup: DashMap::new(),
            adaptive_rotation,
            drain: None,
            takeover_byte_cap: DEFAULT_TAKEOVER_BYTE_CAP,
            layouts: DashMap::new(),
        }
    }

    pub fn with_drain(mut self, drain: Arc<DrainController>) -> Self {
        self.drain = Some(drain);
        self
    }

    pub fn with_takeover_byte_cap(mut self, cap_bytes: usize) -> Self {
        self.takeover_byte_cap = cap_bytes.max(1);
        self
    }

    pub fn wal(&self) -> &Arc<ObjectWal> {
        &self.wal
    }

    pub fn with_field_keys(mut self, service: Arc<FieldKeyService>) -> Self {
        self.sink = Arc::new(
            DurableIntakeSink::new(
                self.wal.clone(),
                self.buffer.clone(),
                self.streams.clone(),
                self.datasets.clone(),
            )
            .with_field_keys(service),
        );

        self
    }

    fn rotation_threshold(&self, key: &BufferKey) -> usize {
        self.adaptive_rotation.threshold_for(key)
    }

    fn flush_max_age(&self) -> Duration {
        Duration::from_secs(self.settings.flush_interval_secs.max(1) as u64)
    }

    fn flush_lock(&self, key: &BufferKey) -> Arc<Mutex<()>> {
        self.flush_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

#[async_trait]
impl IntakeSink for IntakeWorker {
    fn supports_derived_datasets(&self) -> bool {
        true
    }

    fn primary_dataset_type(
        &self,
        stream_type: domain::stream::StreamType,
    ) -> Result<DatasetTypeId> {
        self.datasets.primary_dataset_type(stream_type)
    }

    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        let dataset_type = self.primary_dataset_type(batch.stream_type)?;
        self.write_dataset(dataset_type, batch).await
    }

    async fn write_dataset(
        &self,
        dataset_type: DatasetTypeId,
        batch: IntakeBatch,
    ) -> Result<IntakeResult> {
        self.write_metered_dataset(dataset_type, batch, None).await
    }

    async fn write_metered_dataset(
        &self,
        dataset_type: DatasetTypeId,
        batch: IntakeBatch,
        collected_bytes: Option<u64>,
    ) -> Result<IntakeResult> {
        let stream = self
            .streams
            .get(&batch.org_id, &batch.stream, batch.stream_type)
            .await?;

        let key = self
            .datasets
            .resolve(&stream, dataset_type.clone())
            .await?
            .dataset
            .id
            .clone();

        let result = self
            .sink
            .write_metered_dataset(dataset_type, batch, collected_bytes)
            .await?;

        self.notify_when_rotation_due(&key).await;
        Ok(result)
    }
}

impl IntakeWorker {
    /// Wakes the flush scheduler when the dataset's buffer is due to rotate after a write.
    async fn notify_when_rotation_due(&self, key: &BufferKey) {
        if let Some(buffer) = self.buffer.get(key)
            && buffer
                .records()
                .lock()
                .await
                .rotation_due(
                    self.rotation_threshold(key),
                    self.flush_max_age(),
                    Instant::now(),
                )
                .is_some()
        {
            self.flush_notify.notify_one();
        }
    }
}

#[cfg(test)]
mod tests;
