// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The trace candidate log: tenant spans held in object storage from the moment they are
//! acknowledged until their fate is durable.
//!
//! Tail sampling keeps a trace's spans in memory for its decision window, far longer than a
//! sender may wait, so spans are acknowledged before they are sampled. To make that
//! acknowledgement survive the node and its disk, the intake writes each request's spans to this
//! log, an object per group commit under the same writer epoch and lease as the dataset WAL, and
//! answers only once the object is stored:
//!
//! ```text
//! wal/v1/trace/{writer_epoch:020}/{sequence:020}.wal
//! ```
//!
//! - **Settled:** a span is settled once its fate is durable: its trace was sampled out, it was a
//!   duplicate, or the storage write of its kept trace was acknowledged (by the dataset WAL). A
//!   span handed to a remote sampler owner settles when that owner accepts it, which the owner
//!   does only once the span is in its own log.
//! - **Markers:** settled spans are recorded in the log in batches, so a replay skips them.
//! - **Cleanup:** every object below the lowest sequence with an unsettled span is deleted.
//! - **Takeover:** the log's epoch has a `wal_streams` row like a dataset's. When the writer is
//!   fenced (crash, lost disk, partition) or released (drain) with spans unsettled, another node
//!   claims the row, writes the unsettled spans into its own log, routes them again, deletes the
//!   old objects and closes the row.
//!
//! A replay can write a span twice only when its storage write was acknowledged but its marker
//! was not yet stored, or when a takeover node dies between handing a chunk of replayed spans
//! over and recording in the old log that it did.

mod batch;
mod log;
mod progress;
mod record;
mod takeover;

use domain::storage::{DatasetTypeId, PhysicalDatasetId, WalStreamRow};
use kernel::ids::Id;

pub use self::{
    log::{TraceCandidateLog, TraceLogConfig},
    takeover::{TraceTakeoverRound, spawn_takeover_loop, takeover_round},
};

/// The dataset type of a candidate log's `wal_streams` row.
pub const CANDIDATE_LOG_TYPE: &str = "builtin.traces.candidate_log";
/// The dataset id of a candidate log's `wal_streams` row; one row per writer epoch.
pub const CANDIDATE_LOG_ID: &str = "trace-candidates";

/// Where one acknowledged span sits in a candidate log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpanOrigin {
    pub epoch: u64,
    pub sequence: u64,
    pub record: u32,
    pub span: u32,
}

/// Told when spans are settled: their fate is durable and they need no replay.
pub trait SpanSettlement: Send + Sync {
    fn settle(&self, origins: &[SpanOrigin]);
}

/// Whether a `wal_streams` row is a trace candidate log rather than a dataset's WAL.
pub fn is_candidate_log(row: &WalStreamRow) -> bool {
    row.dataset_type == candidate_log_type()
}

pub fn candidate_log_type() -> DatasetTypeId {
    DatasetTypeId::builtin(CANDIDATE_LOG_TYPE)
}

pub(crate) fn candidate_log_id() -> PhysicalDatasetId {
    PhysicalDatasetId::from(Id(CANDIDATE_LOG_ID.into()))
}

pub(crate) fn epoch_prefix(epoch: u64) -> object_store::path::Path {
    object_store::path::Path::from(format!("wal/v1/trace/{epoch:020}"))
}

pub(crate) fn object_path(epoch: u64, sequence: u64) -> object_store::path::Path {
    object_store::path::Path::from(format!("wal/v1/trace/{epoch:020}/{sequence:020}.wal"))
}

/// The sequence an object path names, if it is a candidate log object.
pub(crate) fn sequence_of(path: &object_store::path::Path) -> Option<u64> {
    path.filename()?.strip_suffix(".wal")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths_round_trip_their_sequence() {
        let path = object_path(7, 42);

        assert_eq!(
            path.as_ref(),
            "wal/v1/trace/00000000000000000007/00000000000000000042.wal"
        );
        assert_eq!(sequence_of(&path), Some(42));
        assert!(path.as_ref().starts_with(epoch_prefix(7).as_ref()));
    }
}
