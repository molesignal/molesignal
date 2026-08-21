// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded WAL recovery into PhysicalDataset-keyed buffer generations.

use std::sync::Arc;

use super::IntakeWorker;
use crate::{
    domain::{
        intake::IntakeBatch,
        storage::{OrganizationScope, WalSequence, WriterEpoch, WriterNodeId},
        stream::StreamDefinition,
    },
    infra::intake::{BufferWriter, DatasetBuffer, WalRecoverySource, physical_schema},
    shared::{Error, Result},
};

impl IntakeWorker {
    /// Replay every retained epoch, publishing bounded generations before the process becomes
    /// ready. A failed dataset remains on disk for the next boot and does not affect other
    /// datasets.
    #[tracing::instrument(
        name = "worker.wal_replay",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "wal_replay")
    )]
    pub async fn recover_and_replay(&self) -> Result<()> {
        tracing::info!("intake wal replay starting");
        let sources = self
            .wal
            .recovery_sources()
            .map_err(|error| Error::internal(format!("wal recover: {error}")))?;
        let mut total_rows = 0usize;
        for source in sources {
            match self.replay_dataset(&source).await {
                Ok(rows) => total_rows += rows,
                Err(error) => tracing::error!(
                    dataset = %source.dataset_id,
                    %error,
                    "wal replay failed; epoch dirs retained for next boot"
                ),
            }
        }
        tracing::info!(rows = total_rows, "intake replay finished");
        self.probe.set_replay_done(true);
        tracing::info!("intake ready");
        Ok(())
    }

    async fn replay_dataset(&self, source: &WalRecoverySource) -> Result<usize> {
        let scope = OrganizationScope::new(source.organization_id.clone());
        let checkpoints = self
            .file_catalog
            .wal_checkpoints(&scope, &source.dataset_id)
            .await?;
        let writer_node_id = WriterNodeId::new(self.wal.node_id());
        let mut rows = 0usize;
        let mut buffer_entry: Option<Arc<DatasetBuffer>> = None;
        let mut stream_definition: Option<StreamDefinition> = None;
        let mut stream_missing = false;

        'epochs: for epoch in &source.epoch_dirs {
            let committed_sequence = checkpoints
                .iter()
                .find(|checkpoint| {
                    checkpoint.writer_node_id == writer_node_id
                        && checkpoint.writer_epoch == WriterEpoch(epoch.epoch)
                })
                .map(|checkpoint| checkpoint.committed_sequence)
                .unwrap_or_default();
            let writer = BufferWriter::new(writer_node_id.clone(), WriterEpoch(epoch.epoch));
            let mut pending_bytes = 0usize;

            for (position, segment) in epoch.segments.iter().enumerate() {
                let records = self
                    .wal
                    .read_segment_records(segment, position + 1 == epoch.segments.len())
                    .map_err(|error| Error::internal(format!("wal read segment: {error}")))?;
                for record in records {
                    if record.term != epoch.epoch {
                        return Err(Error::internal(format!(
                            "wal record epoch {} does not match directory epoch {}",
                            record.term, epoch.epoch
                        )));
                    }
                    if record.index <= committed_sequence.0 {
                        continue;
                    }
                    let batch: IntakeBatch = match serde_json::from_slice(&record.payload) {
                        Ok(batch) => batch,
                        Err(error) => {
                            tracing::warn!(
                                dataset = %source.dataset_id,
                                %error,
                                "wal payload decode failed; skip record"
                            );
                            continue;
                        }
                    };
                    if batch.events.is_empty() {
                        continue;
                    }
                    if batch.org_id != source.organization_id {
                        tracing::error!(
                            dataset = %source.dataset_id,
                            "wal record org mismatch with epoch IDENTITY; skip record"
                        );
                        continue;
                    }

                    if stream_definition.is_none() && !stream_missing {
                        match self
                            .streams
                            .get(&batch.org_id, &batch.stream, batch.stream_type)
                            .await
                        {
                            Ok(definition) => stream_definition = Some(definition),
                            Err(error) => {
                                tracing::debug!(
                                    dataset = %source.dataset_id,
                                    %error,
                                    "stream definition missing during replay; discarding wal"
                                );
                                stream_missing = true;
                            }
                        }
                    }
                    let Some(definition) = stream_definition.as_ref() else {
                        break 'epochs;
                    };

                    if buffer_entry.is_none() {
                        let resolved = self
                            .datasets
                            .resolve(definition, source.dataset_type.clone())
                            .await?;
                        if resolved.dataset.id != source.dataset_id {
                            return Err(Error::conflict(format!(
                                "wal dataset {} resolves to catalog dataset {}",
                                source.dataset_id, resolved.dataset.id
                            )));
                        }
                        buffer_entry =
                            Some(self.buffer.get_or_create_dataset(definition, resolved)?);
                    }

                    let field_key = if definition.schema.fields.iter().any(|field| field.encrypted)
                    {
                        match &self.field_keys {
                            Some(service) => {
                                Some(service.current(&batch.org_id).await.map_err(|error| {
                                    Error::internal(format!(
                                        "field DEK resolve during replay: {error}"
                                    ))
                                })?)
                            }
                            None => {
                                return Err(Error::internal(
                                    "stream has encrypted fields but no field key service",
                                ));
                            }
                        }
                    } else {
                        None
                    };

                    let entry = buffer_entry
                        .as_ref()
                        .expect("buffer entry initialized above");
                    let mut guard = entry.records().lock().await;
                    guard.sync_schema(&physical_schema::project(definition, &source.dataset_type));
                    if let Some(field_key) = field_key {
                        guard.set_field_key(field_key);
                    }
                    let reservation = self.buffer.force_reserve(record.payload.len())?;
                    guard.add_accounted_bytes(reservation.commit());
                    for event in &batch.events {
                        guard
                            .push_with_position(event, &writer, WalSequence(record.index))
                            .map_err(|error| Error::internal(format!("replay push: {error}")))?;
                        rows += 1;
                    }
                    drop(guard);

                    pending_bytes += record.payload.len();
                    if pending_bytes >= self.replay_byte_cap {
                        self.flush_one(&source.dataset_id).await?;
                        pending_bytes = 0;
                    }
                }
            }

            // One FileCatalog flush belongs to exactly one writer epoch.
            if pending_bytes > 0 {
                self.flush_one(&source.dataset_id).await?;
            }
        }

        let drained = match buffer_entry {
            Some(entry) => entry.records().lock().await.is_empty(),
            None => true,
        };
        if !drained {
            return Err(Error::internal(format!(
                "replayed dataset {} was not fully published",
                source.dataset_id
            )));
        }
        self.wal
            .purge_epoch_dirs(&source.epoch_dirs)
            .map_err(|error| Error::internal(format!("wal purge: {error}")))?;
        Ok(rows)
    }
}
