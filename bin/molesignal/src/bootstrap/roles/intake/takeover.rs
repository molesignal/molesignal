// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Taking over the WAL of writers whose lease lapsed: a node that crashed, lost its disk, was
//! partitioned away, or drained without flushing everything.
//!
//! Every intake node runs a round each `takeover_interval_secs`:
//!
//! 1. Fence every writer whose lease lapsed. A fenced writer never publishes again, and it had
//!    stopped acknowledging before the lease could lapse.
//! 2. List the streams whose publisher is fenced. A node takes the datasets it owns by rendezvous
//!    hashing among the live intake nodes, plus any stream it claimed earlier and did not finish.
//! 3. Claim the stream (its publisher becomes this node's epoch), read the committed checkpoint,
//!    and publish every record past it from the objects, in sequence order, under the original
//!    `(node, epoch)`. Flush ids are deterministic, so a retried publication is a no-op, and the
//!    catalog refuses a fenced or superseded publisher, so two publishers never both commit.
//! 4. Delete the objects and close the stream; forget writers with no stream left.
//!
//! A takeover that fails half-way keeps its claim and resumes from the checkpoint next round. A
//! takeover node that dies is itself fenced, and its claims move on to the next node.

use std::{sync::Arc, time::Duration};

use domain::{
    storage::{
        OrganizationScope, OrphanedStreams, PhysicalDataset, WalSequence, WalStreamRow, WriterEpoch,
    },
    stream::StreamDefinition,
};
use futures::TryStreamExt;
use kernel::{Error, Result};
use object_store::{ObjectStoreExt, path::Path};
use tokio::task::JoinHandle;

use super::{IntakeWorker, flush::PublishError};
use crate::{
    app::cluster::WorkOwnership,
    infra::{
        intake::{
            BufferWriter, RecordBuilder,
            object_wal::{self, metrics as wal_metrics},
            physical_schema, wal_record,
        },
        storage::layout::StorageLayout,
    },
};

/// Streams looked at in one round.
const STREAMS_PER_ROUND: u32 = 256;

/// What one round did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TakeoverRound {
    /// Writers fenced because their lease lapsed.
    pub fenced: u64,
    /// Streams published and closed.
    pub streams: usize,
    /// Rows published for other writers.
    pub rows: usize,
}

/// Where a takeover publishes: the stream and dataset the records belong to, and the rows read.
struct Target {
    stream: StreamDefinition,
    dataset: PhysicalDataset,
    builder: RecordBuilder,
}

enum Outcome {
    Published(usize),
    /// The stream or its dataset no longer exists: the records have nowhere to go.
    Gone,
}

impl IntakeWorker {
    /// Runs a takeover round every `period` until the node drains.
    pub fn spawn_takeover_loop(
        self: &Arc<Self>,
        ownership: Arc<WorkOwnership>,
        period: Duration,
    ) -> JoinHandle<()> {
        let worker = self.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                if worker
                    .drain
                    .as_ref()
                    .is_some_and(|drain| drain.is_draining())
                {
                    return;
                }

                match worker.take_over_lapsed(&ownership).await {
                    Ok(round) if round.streams > 0 || round.fenced > 0 => tracing::info!(
                        fenced = round.fenced,
                        streams = round.streams,
                        rows = round.rows,
                        "WAL takeover round"
                    ),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "WAL takeover round failed"),
                }
            }
        })
    }

    /// One takeover round.
    #[tracing::instrument(
        name = "worker.wal_takeover",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "wal_takeover")
    )]
    pub async fn take_over_lapsed(&self, ownership: &WorkOwnership) -> Result<TakeoverRound> {
        let registry = self.wal.registry().clone();

        let mut round = TakeoverRound {
            fenced: registry.fence_expired().await?,
            ..TakeoverRound::default()
        };

        let lease = self.wal.lease();

        if !lease.is_valid() {
            return Ok(round);
        }

        let streams = registry
            .orphaned_streams(
                lease.epoch(),
                &OrphanedStreams::Except(crate::app::trace::durable::candidate_log_type()),
                STREAMS_PER_ROUND,
            )
            .await?;

        let view = ownership.view().await;

        // Trace candidate logs are no datasets: the trace takeover asks for those.
        for stream in streams {
            let resumed = stream.publisher_epoch == lease.epoch();

            if !resumed && !view.owns(stream.dataset_id.as_str()) {
                continue;
            }

            match self.take_over_stream(&stream, lease.epoch()).await {
                Ok(Some(rows)) => {
                    round.streams += 1;
                    round.rows += rows;
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    dataset_id = %stream.dataset_id,
                    writer_node_id = %stream.writer_node_id,
                    writer_epoch = %stream.writer_epoch,
                    %error,
                    "WAL takeover of a stream failed; it resumes from its checkpoint next round"
                ),
            }
        }

        registry.forget_finished_writers().await?;
        Ok(round)
    }

    /// Claims and publishes one stream; `None` when another node holds it.
    async fn take_over_stream(
        &self,
        stream: &WalStreamRow,
        claimant: WriterEpoch,
    ) -> Result<Option<usize>> {
        if !self.wal.registry().claim_stream(stream, claimant).await? {
            return Ok(None);
        }

        let objects = self.list_wal_objects(stream).await?;

        tracing::info!(
            dataset_id = %stream.dataset_id,
            writer_node_id = %stream.writer_node_id,
            writer_epoch = %stream.writer_epoch,
            objects = objects.len(),
            "claimed the WAL of a lapsed writer"
        );

        let rows = match self.publish_stream(stream, claimant, &objects).await? {
            Outcome::Published(rows) => rows,
            Outcome::Gone => {
                tracing::error!(
                    dataset_id = %stream.dataset_id,
                    objects = objects.len(),
                    "WAL of a stream or dataset that no longer exists; its records are dropped"
                );
                0
            }
        };

        let failed = object_wal::delete_objects(self.wal.store(), objects).await;

        if !failed.is_empty() {
            return Err(Error::unavailable(format!(
                "{} WAL objects could not be deleted",
                failed.len()
            )));
        }

        self.wal
            .registry()
            .close_stream(
                &stream.organization_id,
                &stream.dataset_id,
                stream.writer_epoch,
                claimant,
            )
            .await?;

        wal_metrics::inc_takeover_rows(rows);
        Ok(Some(rows))
    }

    /// The stream's objects, by sequence.
    async fn list_wal_objects(&self, stream: &WalStreamRow) -> Result<Vec<(u64, Path)>> {
        let prefix = Path::from(StorageLayout::wal_stream_prefix(
            &stream.organization_id,
            &stream.dataset_id,
            stream.writer_epoch.0,
        ));

        let listed: Vec<_> = self
            .wal
            .store()
            .list(Some(&prefix))
            .try_collect()
            .await
            .map_err(|error| Error::unavailable(format!("list WAL objects: {error}")))?;

        let mut objects: Vec<(u64, Path)> = listed
            .into_iter()
            .filter_map(|meta| {
                StorageLayout::wal_object_sequence(meta.location.as_ref())
                    .map(|sequence| (sequence, meta.location))
            })
            .collect();

        objects.sort_by_key(|(sequence, _)| *sequence);
        Ok(objects)
    }

    /// Publishes every record past the committed checkpoint, at most `takeover_byte_cap` per
    /// segment.
    async fn publish_stream(
        &self,
        stream: &WalStreamRow,
        publisher: WriterEpoch,
        objects: &[(u64, Path)],
    ) -> Result<Outcome> {
        let scope = OrganizationScope::new(stream.organization_id.clone());

        let committed = self
            .file_catalog
            .wal_checkpoints(&scope, &stream.dataset_id)
            .await?
            .into_iter()
            .find(|checkpoint| {
                checkpoint.writer_node_id == stream.writer_node_id
                    && checkpoint.writer_epoch == stream.writer_epoch
            })
            .map_or(0, |checkpoint| checkpoint.committed_sequence.0);

        let writer = BufferWriter::new(stream.writer_node_id.clone(), stream.writer_epoch);
        let mut target: Option<Target> = None;
        let mut pending_bytes = 0usize;
        let mut rows = 0usize;

        for (sequence, path) in objects {
            if *sequence <= committed {
                continue;
            }

            let bytes = self
                .wal
                .store()
                .get(path)
                .await
                .map_err(|error| Error::unavailable(format!("read WAL object {path}: {error}")))?
                .bytes()
                .await
                .map_err(|error| Error::unavailable(format!("read WAL object {path}: {error}")))?;

            let records = object_wal::decode_object(&bytes, self.wal.cipher())
                .map_err(|error| Error::internal(format!("WAL object {path}: {error}")))?;

            for record in records {
                let batch = wal_record::decode(&record)
                    .map_err(|error| Error::internal(format!("WAL record in {path}: {error}")))?;

                if batch.columns.num_rows() == 0 {
                    continue;
                }

                if batch.org_id != stream.organization_id {
                    return Err(Error::conflict(format!(
                        "WAL object {path} holds a record of another organization"
                    )));
                }

                if target.is_none() {
                    match self.takeover_target(stream, &batch).await? {
                        Some(found) => target = Some(found),
                        None => return Ok(Outcome::Gone),
                    }
                }

                let current = target.as_mut().expect("target set above");
                current.builder.add_accounted_bytes(record.len());
                current.builder.add_collected_bytes(batch.collected_bytes);
                current
                    .builder
                    .push_batch(&batch.columns, &writer, WalSequence(*sequence))
                    .map_err(|error| Error::internal(format!("takeover push: {error}")))?;

                rows += batch.columns.num_rows();
                pending_bytes += record.len();
            }

            // A segment holds whole objects: its range ends where an object ends.
            if pending_bytes >= self.takeover_byte_cap
                && let Some(current) = target.as_mut()
            {
                if !self.publish_takeover(current, publisher).await? {
                    return Ok(Outcome::Gone);
                }

                pending_bytes = 0;
            }
        }

        if let Some(current) = target.as_mut()
            && !self.publish_takeover(current, publisher).await?
        {
            return Ok(Outcome::Gone);
        }

        Ok(Outcome::Published(rows))
    }

    /// The stream and dataset of the records, or `None` when they are gone: the stream was
    /// deleted, or deleted and made again (its dataset is then another one).
    async fn takeover_target(
        &self,
        stream: &WalStreamRow,
        batch: &wal_record::WalBatch,
    ) -> Result<Option<Target>> {
        let definition = match self
            .streams
            .get(&batch.org_id, &batch.stream, batch.stream_type)
            .await
        {
            Ok(definition) => definition,
            Err(Error::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        };

        let resolved = self
            .datasets
            .resolve(&definition, stream.dataset_type.clone())
            .await?;

        if resolved.dataset.id != stream.dataset_id {
            return Ok(None);
        }

        let builder =
            RecordBuilder::new(&physical_schema::project(&definition, &stream.dataset_type));

        Ok(Some(Target {
            stream: definition,
            dataset: resolved.dataset.clone(),
            builder,
        }))
    }

    /// Publishes what the target holds; `false` when its dataset is gone from the catalog.
    async fn publish_takeover(&self, target: &mut Target, publisher: WriterEpoch) -> Result<bool> {
        let Some(generation) = target
            .builder
            .begin_flush()
            .map_err(|error| Error::internal(format!("takeover rotation: {error}")))?
        else {
            return Ok(true);
        };

        match self
            .publish_generation(&target.stream, &target.dataset, &generation, publisher)
            .await
        {
            Ok(_) => {
                target
                    .builder
                    .complete_flush(&generation.flush_id())
                    .map_err(|error| Error::internal(format!("takeover retirement: {error}")))?;

                Ok(true)
            }
            Err(PublishError::Commit(Error::NotFound(_))) => Ok(false),
            Err(PublishError::Write(error) | PublishError::Commit(error)) => Err(error),
        }
    }
}
