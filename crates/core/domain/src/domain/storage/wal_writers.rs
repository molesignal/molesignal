// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writers of the object-storage WAL and who may publish what they wrote.
//!
//! Every process takes a writer epoch from the registry when it starts. Epochs are unique across
//! the cluster and only grow, so a node that comes back without its disk never reuses one, and
//! `(node, epoch, sequence)` names one WAL record for good.
//!
//! A writer holds a lease. While the lease is alive only the writer itself publishes the streams
//! it wrote. Once it lapses, any node may fence the writer and claim its streams: the claim moves
//! a stream's `publisher` to the claimant, and `FileCatalog::commit_flush` refuses a publication
//! whose publisher is not the stream's current one, or is fenced. The writer itself stops
//! acknowledging writes before its lease can lapse, so nothing acknowledged is written after a
//! claim.

use std::time::Duration;

use async_trait::async_trait;
use kernel::{Result, ids::Id};

use super::{
    ids::{PhysicalDatasetId, WriterEpoch, WriterNodeId},
    type_id::DatasetTypeId,
};

/// One writer's WAL for one physical dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalStreamRow {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub dataset_type: DatasetTypeId,
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
    /// The epoch allowed to publish the stream: the writer's own, or a claimant's.
    pub publisher_epoch: WriterEpoch,
}

/// Which orphaned streams a takeover round asks for. Each kind of round asks only for its own
/// rows, so a pile of one kind cannot fill the other's page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanedStreams {
    /// Every stream except those of this dataset type.
    Except(DatasetTypeId),
    /// Only the streams of this dataset type.
    Only(DatasetTypeId),
}

impl OrphanedStreams {
    pub fn admits(&self, dataset_type: &DatasetTypeId) -> bool {
        match self {
            Self::Except(excluded) => dataset_type != excluded,
            Self::Only(only) => dataset_type == only,
        }
    }
}

#[async_trait]
pub trait WalWriterRegistry: Send + Sync {
    /// Starts a new writer for `node_id` holding a lease of `lease`; returns its epoch.
    async fn register(&self, node_id: &WriterNodeId, lease: Duration) -> Result<WriterEpoch>;

    /// Extends the lease. `false` when the writer was fenced: it may never publish again.
    async fn renew(&self, epoch: WriterEpoch, lease: Duration) -> Result<bool>;

    /// Fences the writer and ends its lease at once, so its remaining streams can be claimed
    /// without waiting: a draining node's last step.
    async fn release(&self, epoch: WriterEpoch) -> Result<()>;

    /// Records that `epoch` writes the WAL of a dataset. Idempotent.
    async fn open_stream(&self, stream: &WalStreamRow) -> Result<()>;

    /// Forgets a stream whose records are all published and whose objects are gone. Only the
    /// stream's current publisher may do so; returns whether the row was removed.
    async fn close_stream(
        &self,
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        publisher_epoch: WriterEpoch,
    ) -> Result<bool>;

    /// Fences every writer whose lease has lapsed; returns how many were fenced now.
    async fn fence_expired(&self) -> Result<u64>;

    /// Work for a takeover by `claimant`, oldest writer first: streams of the `kind` asked for
    /// whose publisher is fenced, and those `claimant` claimed earlier and has not finished.
    async fn orphaned_streams(
        &self,
        claimant: WriterEpoch,
        kind: &OrphanedStreams,
        limit: u32,
    ) -> Result<Vec<WalStreamRow>>;

    /// Makes `claimant` the publisher of a stream, if its current publisher is fenced (or is the
    /// claimant already) and the claimant is not fenced. Returns whether `claimant` now holds it.
    async fn claim_stream(&self, stream: &WalStreamRow, claimant: WriterEpoch) -> Result<bool>;

    /// Deletes fenced writers that have no stream left; returns how many.
    async fn forget_finished_writers(&self) -> Result<u64>;
}
