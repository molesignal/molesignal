// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Intake role composition: durable append, bounded replay, and ordered catalog publication.

mod flush;
mod replay;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{Mutex, Notify};

use crate::{
    config::IntakeSettings,
    domain::{
        intake::{IntakeBatch, IntakeResult, IntakeSink},
        storage::{DatasetTypeId, FileCatalog, PhysicalDatasetId},
        stream::StreamRepository,
    },
    infra::{
        cipher::FieldKeyService,
        intake::{
            AdaptiveRotation, BufferKey, BufferPool, DatasetResolver, DurableIntakeSink, WalPool,
        },
        storage::parquet::writer::ParquetWriter,
    },
    shared::{Result, drain::DrainController, health::Probe},
};

#[derive(Debug, Clone)]
struct PendingWalCleanup {
    dataset_id: PhysicalDatasetId,
    high_watermark_seq: u64,
}

const DEFAULT_REPLAY_BYTE_CAP: usize = 512 * 1024 * 1024;

pub struct IntakeWorker {
    sink: Arc<DurableIntakeSink>,
    wal: Arc<WalPool>,
    buffer: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    datasets: Arc<DatasetResolver>,
    file_catalog: Arc<dyn FileCatalog>,
    parquet_writer: Arc<ParquetWriter>,
    probe: Arc<Probe>,
    settings: IntakeSettings,
    flush_notify: Arc<Notify>,
    /// One publication at a time per physical dataset; distinct datasets may publish in parallel.
    flush_locks: DashMap<BufferKey, Arc<Mutex<()>>>,
    /// Catalog committed, while local WAL cleanup still needs retrying.
    pending_wal_cleanup: DashMap<BufferKey, PendingWalCleanup>,
    adaptive_rotation: AdaptiveRotation,
    field_keys: Option<Arc<FieldKeyService>>,
    drain: Option<Arc<DrainController>>,
    replay_byte_cap: usize,
}

impl IntakeWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wal: Arc<WalPool>,
        buffer: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
        datasets: Arc<DatasetResolver>,
        file_catalog: Arc<dyn FileCatalog>,
        parquet_writer: Arc<ParquetWriter>,
        probe: Arc<Probe>,
        settings: IntakeSettings,
    ) -> Self {
        let buffer_max_bytes = (settings.buffer_max_mb as usize).saturating_mul(1024 * 1024);
        let adaptive_rotation = AdaptiveRotation::new(&settings.rotation, buffer_max_bytes);
        let sink = Arc::new(DurableIntakeSink::new(
            wal.clone(),
            buffer.clone(),
            streams.clone(),
            datasets.clone(),
        ));
        Self {
            sink,
            wal,
            buffer,
            streams,
            datasets,
            file_catalog,
            parquet_writer,
            probe,
            settings,
            flush_notify: Arc::new(Notify::new()),
            flush_locks: DashMap::new(),
            pending_wal_cleanup: DashMap::new(),
            adaptive_rotation,
            field_keys: None,
            drain: None,
            replay_byte_cap: DEFAULT_REPLAY_BYTE_CAP,
        }
    }

    pub fn with_drain(mut self, drain: Arc<DrainController>) -> Self {
        self.drain = Some(drain);
        self
    }

    pub fn with_replay_byte_cap(mut self, cap_bytes: usize) -> Self {
        self.replay_byte_cap = cap_bytes.max(1);
        self
    }

    pub fn with_field_keys(mut self, service: Arc<FieldKeyService>) -> Self {
        self.sink = Arc::new(
            DurableIntakeSink::new(
                self.wal.clone(),
                self.buffer.clone(),
                self.streams.clone(),
                self.datasets.clone(),
            )
            .with_field_keys(service.clone()),
        );
        self.field_keys = Some(service);
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
        stream_type: crate::domain::stream::StreamType,
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
        let result = self.sink.write_dataset(dataset_type, batch).await?;
        if let Some(buffer) = self.buffer.get(&key)
            && buffer
                .records()
                .lock()
                .await
                .rotation_due(
                    self.rotation_threshold(&key),
                    self.flush_max_age(),
                    Instant::now(),
                )
                .is_some()
        {
            self.flush_notify.notify_one();
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
