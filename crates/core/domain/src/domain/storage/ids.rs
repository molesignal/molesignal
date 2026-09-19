// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 存储上下文的强类型标识。
//!
//! 底层统一复用仓库通用的 [`Id`]（KSUID 字符串，字典序近似时间序）；这里只做
//! 类型区分，防止 dataset / segment / artifact 的 ID 在参数表里互相串位。
//! WAL 侧的 `WriterEpoch` / `WalSequence` 是纯数值序号，不是实体 ID。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::shared::ids::Id;

macro_rules! entity_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Id);

        impl $name {
            /// 生成新 KSUID。
            pub fn generate() -> Self {
                Self(Id::new())
            }

            pub fn from_string(value: impl Into<String>) -> Self {
                Self(Id::from_string(value))
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.0.as_str())
            }
        }

        impl From<Id> for $name {
            fn from(id: Id) -> Self {
                Self(id)
            }
        }
    };
}

entity_id! {
    /// 物理数据集 ID。FileCatalog、WAL、Buffer、对象路径只认它，不理解信号语义。
    PhysicalDatasetId
}

entity_id! {
    /// 一次 flush / compaction 产出的不可变数据单元 ID。
    SegmentId
}

entity_id! {
    /// Segment 下单个物理文件（Parquet、索引、统计等）的 ID。
    ArtifactId
}

/// 写入方节点标识（配置里的 node id）。进 WAL 目录、flush 幂等键与 checkpoint 主键；
/// 同一 dataset 允许多个节点并发各写各的 WAL，序号空间按 (node, epoch) 隔离。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WriterNodeId(pub String);

impl WriterNodeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WriterNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 单个 (dataset, node) 写入所有权的代际号。节点每次重新取得所有权时递增；
/// Catalog 提交校验 epoch，旧 writer 的迟到提交会被拒绝。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WriterEpoch(pub u64);

impl fmt::Display for WriterEpoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// (dataset, node, epoch) 内单调递增的 WAL 记录序号。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WalSequence(pub u64);

impl fmt::Display for WalSequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 闭区间 WAL 序号范围（一次 flush 覆盖的记录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceRange {
    pub start: WalSequence,
    pub end: WalSequence,
}

impl SequenceRange {
    pub fn new(start: WalSequence, end: WalSequence) -> Self {
        Self { start, end }
    }
}

/// flush 幂等键。同一段 WAL 无论重试多少次只能提交一次；键必须含 writer node，
/// 否则多写入节点的 (epoch, seq) 序号空间会互相撞车导致丢 flush。
///
/// 采用规范字符串而非哈希：可读、可反查、天然确定性。org / dataset 由所在表列
/// 提供作用域，不重复编码进键。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FlushId(String);

impl FlushId {
    pub fn derive(node: &WriterNodeId, epoch: WriterEpoch, sequence: SequenceRange) -> Self {
        Self(format!(
            "f-{}-{}-{}-{}",
            node.0, epoch.0, sequence.start.0, sequence.end.0
        ))
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FlushId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Arrow schema 的稳定指纹（字段名 + 类型的顺序敏感哈希，具体算法在 infra 层）。
/// 段与索引记录指纹用于一致性校验；不建 schema registry，schema 本体以
/// Parquet footer 为准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaFingerprint(pub i64);

/// 对象存储 key。只能由统一 StorageLayout 生成，业务代码不得自行拼接。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectKey(pub String);

impl ObjectKey {
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 对象内容校验和，带算法前缀（如 `b3:<hex>`、`crc32c:<hex>`），由上传方计算。
/// Catalog、块缓存 key、GC 删除前比对都用它。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectChecksum(pub String);

impl ObjectChecksum {
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectChecksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_id_is_deterministic_and_node_scoped() {
        let range = SequenceRange::new(WalSequence(10), WalSequence(42));
        let a = FlushId::derive(&WriterNodeId::new("ingester-0"), WriterEpoch(3), range);
        let b = FlushId::derive(&WriterNodeId::new("ingester-0"), WriterEpoch(3), range);
        let other_node = FlushId::derive(&WriterNodeId::new("ingester-1"), WriterEpoch(3), range);
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "f-ingester-0-3-10-42");
        assert_ne!(a, other_node);
    }

    #[test]
    fn entity_ids_roundtrip_serde_as_plain_strings() {
        let id = SegmentId::from_string("abc123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc123\"");
        let back: SegmentId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
