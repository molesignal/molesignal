// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A [`WalWriterRegistry`] inside one process, for tests and tools that run without Postgres. It
//! follows the same rules as the Postgres one, including the publication fence
//! ([`MemoryWalWriters::may_publish`]) that an in-memory catalog applies in `commit_flush`.

use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use domain::storage::{
    OrphanedStreams, PhysicalDatasetId, WalStreamRow, WalWriterRegistry, WriterEpoch, WriterNodeId,
};
use kernel::{Result, ids::Id};

struct Writer {
    lease_expires_at: Instant,
    fenced: bool,
}

type StreamKey = (String, String, u64);

#[derive(Default)]
struct State {
    next_epoch: u64,
    writers: BTreeMap<u64, Writer>,
    streams: BTreeMap<StreamKey, WalStreamRow>,
}

#[derive(Default)]
pub struct MemoryWalWriters {
    state: Mutex<State>,
}

fn stream_key(
    organization_id: &Id,
    dataset_id: &PhysicalDatasetId,
    epoch: WriterEpoch,
) -> StreamKey {
    (
        organization_id.as_str().to_owned(),
        dataset_id.as_str().to_owned(),
        epoch.0,
    )
}

impl MemoryWalWriters {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("wal writers lock")
    }

    /// Ends a writer's lease now, as if its process had died a lease ago.
    pub fn expire(&self, epoch: WriterEpoch) {
        if let Some(writer) = self.state().writers.get_mut(&epoch.0) {
            writer.lease_expires_at = Instant::now() - Duration::from_millis(1);
        }
    }

    pub fn is_fenced(&self, epoch: WriterEpoch) -> bool {
        self.state()
            .writers
            .get(&epoch.0)
            .is_some_and(|writer| writer.fenced)
    }

    pub fn streams(&self) -> Vec<WalStreamRow> {
        self.state().streams.values().cloned().collect()
    }

    pub fn writer_count(&self) -> usize {
        self.state().writers.len()
    }

    /// The fence `commit_flush` applies: `Err` names why `publisher` may not publish the stream.
    pub fn may_publish(
        &self,
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        publisher: WriterEpoch,
    ) -> std::result::Result<(), String> {
        let state = self.state();
        let key = stream_key(organization_id, dataset_id, writer_epoch);

        let Some(stream) = state.streams.get(&key) else {
            return Err("no open WAL stream".into());
        };

        if stream.publisher_epoch != publisher {
            return Err(format!("publisher {}", stream.publisher_epoch));
        }

        match state.writers.get(&publisher.0) {
            Some(writer) if !writer.fenced => Ok(()),
            _ => Err(format!("fenced publisher {publisher}")),
        }
    }
}

#[async_trait]
impl WalWriterRegistry for MemoryWalWriters {
    async fn register(&self, _node_id: &WriterNodeId, lease: Duration) -> Result<WriterEpoch> {
        let mut state = self.state();
        state.next_epoch += 1;
        let epoch = state.next_epoch;

        state.writers.insert(
            epoch,
            Writer {
                lease_expires_at: Instant::now() + lease,
                fenced: false,
            },
        );

        Ok(WriterEpoch(epoch))
    }

    async fn renew(&self, epoch: WriterEpoch, lease: Duration) -> Result<bool> {
        let mut state = self.state();

        match state.writers.get_mut(&epoch.0) {
            Some(writer) if !writer.fenced => {
                writer.lease_expires_at = Instant::now() + lease;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn release(&self, epoch: WriterEpoch) -> Result<()> {
        if let Some(writer) = self.state().writers.get_mut(&epoch.0) {
            writer.fenced = true;
            writer.lease_expires_at = writer.lease_expires_at.min(Instant::now());
        }

        Ok(())
    }

    async fn open_stream(&self, stream: &WalStreamRow) -> Result<()> {
        let key = stream_key(
            &stream.organization_id,
            &stream.dataset_id,
            stream.writer_epoch,
        );
        self.state()
            .streams
            .entry(key)
            .or_insert_with(|| stream.clone());
        Ok(())
    }

    async fn close_stream(
        &self,
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        publisher_epoch: WriterEpoch,
    ) -> Result<bool> {
        let mut state = self.state();
        let key = stream_key(organization_id, dataset_id, writer_epoch);

        if state
            .streams
            .get(&key)
            .is_some_and(|stream| stream.publisher_epoch == publisher_epoch)
        {
            state.streams.remove(&key);
            return Ok(true);
        }

        Ok(false)
    }

    async fn fence_expired(&self) -> Result<u64> {
        let now = Instant::now();
        let mut fenced = 0;

        for writer in self.state().writers.values_mut() {
            if !writer.fenced && writer.lease_expires_at < now {
                writer.fenced = true;
                fenced += 1;
            }
        }

        Ok(fenced)
    }

    async fn orphaned_streams(
        &self,
        claimant: WriterEpoch,
        kind: &OrphanedStreams,
        limit: u32,
    ) -> Result<Vec<WalStreamRow>> {
        let state = self.state();

        let mut rows: Vec<WalStreamRow> = state
            .streams
            .values()
            .filter(|stream| kind.admits(&stream.dataset_type))
            .filter(|stream| {
                let publisher_fenced = state
                    .writers
                    .get(&stream.publisher_epoch.0)
                    .is_none_or(|writer| writer.fenced);

                publisher_fenced
                    || (stream.publisher_epoch == claimant && stream.writer_epoch != claimant)
            })
            .cloned()
            .collect();

        rows.sort_by_key(|stream| stream.writer_epoch);
        rows.truncate(limit as usize);
        Ok(rows)
    }

    async fn claim_stream(&self, stream: &WalStreamRow, claimant: WriterEpoch) -> Result<bool> {
        let mut state = self.state();

        let claimant_live = state
            .writers
            .get(&claimant.0)
            .is_some_and(|writer| !writer.fenced);

        let key = stream_key(
            &stream.organization_id,
            &stream.dataset_id,
            stream.writer_epoch,
        );

        let Some(current) = state.streams.get(&key).map(|row| row.publisher_epoch) else {
            return Ok(false);
        };

        let current_fenced = state
            .writers
            .get(&current.0)
            .is_none_or(|writer| writer.fenced);

        if !claimant_live || (!current_fenced && current != claimant) {
            return Ok(false);
        }

        if let Some(row) = state.streams.get_mut(&key) {
            row.publisher_epoch = claimant;
        }

        Ok(true)
    }

    async fn forget_finished_writers(&self) -> Result<u64> {
        let mut state = self.state();

        let busy: Vec<u64> = state
            .streams
            .values()
            .flat_map(|stream| [stream.writer_epoch.0, stream.publisher_epoch.0])
            .collect();

        let before = state.writers.len();
        state
            .writers
            .retain(|epoch, writer| !writer.fenced || busy.contains(epoch));

        Ok((before - state.writers.len()) as u64)
    }
}
