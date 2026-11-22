// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WAL 身份与 flush 溯源。
//!
//! The WAL lives in object storage and only holds data not yet flushed; queries never read it.
//! A record is named by `(org, dataset, node, epoch, sequence)`; epochs come from
//! [`super::WalWriterRegistry`] and are unique across the cluster.

use serde::{Deserialize, Serialize};

use super::ids::{FlushId, PhysicalDatasetId, SequenceRange, WriterEpoch, WriterNodeId};

/// Cumulative canonical intake bytes committed for a physical dataset.
#[derive(Debug, Clone)]
pub struct DatasetIntakeUsage {
    pub dataset_id: PhysicalDatasetId,
    pub collected_bytes: Option<u64>,
    /// False when historical flushes predate byte metering.
    pub complete: bool,
}

/// 一次 flush 的提交溯源；进 `storage_flush_commits` 表并推进 WAL checkpoint。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlushProvenance {
    pub flush_id: FlushId,
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
    pub sequence: SequenceRange,
    /// Canonical pre-transformation bytes; None for legacy/unmetered generations.
    #[serde(default)]
    pub collected_bytes: Option<u64>,
}

impl FlushProvenance {
    /// 从 (node, epoch, seq range) 推导确定性 flush_id。
    pub fn derive(node: WriterNodeId, epoch: WriterEpoch, sequence: SequenceRange) -> Self {
        Self {
            flush_id: FlushId::derive(&node, epoch, sequence),
            writer_node_id: node,
            writer_epoch: epoch,
            sequence,
            collected_bytes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::storage::ids::WalSequence;

    #[test]
    fn provenance_derives_matching_flush_id() {
        let provenance = FlushProvenance::derive(
            WriterNodeId::new("node-a"),
            WriterEpoch(2),
            SequenceRange::new(WalSequence(1), WalSequence(9)),
        );
        assert_eq!(provenance.flush_id.as_str(), "f-node-a-2-1-9");
    }
}
