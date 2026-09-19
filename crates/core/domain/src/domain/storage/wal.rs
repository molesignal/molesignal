// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WAL 身份与 flush 溯源。
//!
//! WAL 是独立于 FileCatalog 的持久化子系统，只保证未 flush 数据的持久与恢复；
//! 正常查询不扫 WAL。身份统一落在 `(org, dataset, node, epoch)`，
//! 不再使用 stream name / stream type / dataset type 作为路径或键。

use serde::{Deserialize, Serialize};

use super::ids::{FlushId, PhysicalDatasetId, SequenceRange, WriterEpoch, WriterNodeId};
use crate::shared::ids::Id;

/// 一条 WAL 流的完整身份。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WalStreamId {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
}

impl WalStreamId {
    /// 本地目录相对路径：`{node}/{dataset}/{epoch}`。
    /// org 不进路径——单机 WAL 根目录属于一个进程，dataset ID 已全局唯一。
    pub fn local_dir(&self) -> String {
        format!(
            "{}/{}/{}",
            self.writer_node_id.0, self.dataset_id, self.writer_epoch.0
        )
    }
}

/// 一次 flush 的提交溯源；进 `storage_flush_commits` 表并推进 WAL checkpoint。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlushProvenance {
    pub flush_id: FlushId,
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
    pub sequence: SequenceRange,
}

impl FlushProvenance {
    /// 从 (node, epoch, seq range) 推导确定性 flush_id。
    pub fn derive(node: WriterNodeId, epoch: WriterEpoch, sequence: SequenceRange) -> Self {
        Self {
            flush_id: FlushId::derive(&node, epoch, sequence),
            writer_node_id: node,
            writer_epoch: epoch,
            sequence,
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

    #[test]
    fn local_dir_uses_only_stable_ids() {
        let id = WalStreamId {
            organization_id: Id::from_string("org-1"),
            dataset_id: PhysicalDatasetId::from_string("ds-1"),
            writer_node_id: WriterNodeId::new("node-a"),
            writer_epoch: WriterEpoch(3),
        };
        assert_eq!(id.local_dir(), "node-a/ds-1/3");
    }
}
