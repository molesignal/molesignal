// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DataSegment：一次 flush 或 compaction 形成的不可变数据单元。
//!
//! 一个 Segment 由一个主数据 Artifact 和若干辅助 Artifact（索引、统计）组成；
//! Parquet 与索引之间的关系只由 Catalog 行描述，不做路径推导。

use serde::{Deserialize, Serialize};

use super::{
    artifact::{Artifact, ArtifactRole},
    ids::{FlushId, PhysicalDatasetId, SchemaFingerprint, SegmentId, SequenceRange},
};
use crate::shared::{ids::Id, time::TimeRange};

/// Segment 可见性状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    /// 对查询可见。
    Active,
    /// 已被 compaction 输出替换；对象等待延迟 GC。
    Replaced,
    /// 被 retention / 显式删除标记；对象等待延迟 GC。
    Tombstoned,
    /// 元数据已进入不可变 Partition Manifest，不再作为 hot overlay 返回。
    Sealed,
}

impl SegmentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Replaced => "replaced",
            Self::Tombstoned => "tombstoned",
            Self::Sealed => "sealed",
        }
    }
}

impl std::str::FromStr for SegmentState {
    type Err = crate::shared::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "replaced" => Ok(Self::Replaced),
            "tombstoned" => Ok(Self::Tombstoned),
            "sealed" => Ok(Self::Sealed),
            other => Err(crate::shared::Error::invalid(format!(
                "unknown segment state `{other}`"
            ))),
        }
    }
}

/// 时间分区槽位：`[start, end)` 的 UTC 桶 + 水平分片号。
/// 粒度由 dataset 的 PartitionPolicy 决定（当前为小时桶，shard 恒为 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Partition {
    pub start_micros: i64,
    pub end_micros: i64,
    pub shard: u16,
}

/// 列级 min/max 统计（按字段名 → JSON 标量），供查询侧保守跳段。
/// 语义：缺失字段不参与裁剪；统计必须覆盖该 Segment 全部行。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    #[serde(default)]
    pub min_values: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub max_values: serde_json::Map<String, serde_json::Value>,
}

impl ColumnStats {
    pub fn is_empty(&self) -> bool {
        self.min_values.is_empty() && self.max_values.is_empty()
    }
}

/// 一次 flush / compaction 产出的不可变数据单元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSegment {
    pub id: SegmentId,
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,

    pub partition: Partition,
    /// 实际数据的事件时间覆盖范围（可能窄于分区槽位）。
    pub time_range: TimeRange,
    /// flush 产物记录其覆盖的 WAL 序号（写入节点 + epoch 见 flush 溯源）；
    /// compaction 产物为 None。
    pub sequence_range: Option<SequenceRange>,

    pub row_count: u64,
    pub schema_fingerprint: Option<SchemaFingerprint>,
    pub column_stats: ColumnStats,

    /// 产生本 Segment 的 flush；compaction 产物为 None。
    pub flush_id: Option<FlushId>,
    /// 同一次 flush 按分区切出多个 Segment 时的序号，保证幂等重试产出稳定。
    pub output_ordinal: u32,

    pub primary: Artifact,
    pub auxiliaries: Vec<Artifact>,

    pub state: SegmentState,
    /// 从哪个 catalog version 起可见 / 何时退役；调试与快照一致性核对用。
    pub visible_from_version: u64,
    pub retired_at_version: Option<u64>,
    pub created_at_micros: i64,
}

impl DataSegment {
    /// 主数据 + 辅助 Artifact 的统一遍历。
    pub fn artifacts(&self) -> impl Iterator<Item = &Artifact> {
        std::iter::once(&self.primary).chain(self.auxiliaries.iter())
    }

    /// 校验结构不变量：主 Artifact 角色必须是 PrimaryData，辅助不允许再有主数据。
    pub fn validate(&self) -> crate::shared::Result<()> {
        if self.primary.role != ArtifactRole::PrimaryData {
            return Err(crate::shared::Error::invalid(format!(
                "segment {} primary artifact must have role primary_data, got {}",
                self.id,
                self.primary.role.as_str()
            )));
        }
        if self.primary.source_artifact_id.is_some() || self.primary.source_checksum.is_some() {
            return Err(crate::shared::Error::invalid(format!(
                "segment {} primary artifact must not declare a source",
                self.id
            )));
        }
        if let Some(aux) = self
            .auxiliaries
            .iter()
            .find(|a| a.role == ArtifactRole::PrimaryData)
        {
            return Err(crate::shared::Error::invalid(format!(
                "segment {} auxiliary artifact {} must not have role primary_data",
                self.id, aux.id
            )));
        }
        if let Some(index) = self.auxiliaries.iter().find(|artifact| {
            artifact.role == ArtifactRole::Index
                && (artifact.source_artifact_id.as_ref() != Some(&self.primary.id)
                    || artifact.source_checksum.as_ref() != Some(&self.primary.object.checksum))
        }) {
            return Err(crate::shared::Error::invalid(format!(
                "segment {} index artifact {} is not bound to the primary artifact identity",
                self.id, index.id
            )));
        }
        if self.time_range.start.0 > self.time_range.end.0 {
            return Err(crate::shared::Error::invalid(format!(
                "segment {} has inverted time range",
                self.id
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::{
            artifact::{ArtifactState, StoredObject},
            ids::{ArtifactId, ObjectChecksum, ObjectKey},
            type_id::{ArtifactTypeId, builtin},
        },
        shared::time::TimestampMicros,
    };

    fn artifact(role: ArtifactRole) -> Artifact {
        Artifact {
            id: ArtifactId::generate(),
            role,
            artifact_type: ArtifactTypeId::builtin(builtin::ARTIFACT_PARQUET),
            format_version: 1,
            object: StoredObject {
                key: ObjectKey::from_string("v1/artifacts/o/d/p/s/a.parquet"),
                size_bytes: 128,
                checksum: ObjectChecksum::from_string("b3:00"),
                etag: None,
            },
            source_artifact_id: None,
            source_checksum: None,
            schema_fingerprint: None,
            state: ArtifactState::Ready,
            failure_reason: None,
        }
    }

    fn segment(primary: Artifact, auxiliaries: Vec<Artifact>) -> DataSegment {
        DataSegment {
            id: SegmentId::generate(),
            organization_id: Id::from_string("org"),
            dataset_id: PhysicalDatasetId::generate(),
            partition: Partition {
                start_micros: 0,
                end_micros: 3_600_000_000,
                shard: 0,
            },
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            sequence_range: None,
            row_count: 1,
            schema_fingerprint: None,
            column_stats: ColumnStats::default(),
            flush_id: None,
            output_ordinal: 0,
            primary,
            auxiliaries,
            state: SegmentState::Active,
            visible_from_version: 1,
            retired_at_version: None,
            created_at_micros: 0,
        }
    }

    fn index_for(primary: &Artifact) -> Artifact {
        let mut index = artifact(ArtifactRole::Index);
        index.source_artifact_id = Some(primary.id.clone());
        index.source_checksum = Some(primary.object.checksum.clone());
        index
    }

    #[test]
    fn validate_rejects_misplaced_primary_role() {
        let primary = artifact(ArtifactRole::PrimaryData);
        assert!(
            segment(primary.clone(), vec![index_for(&primary)])
                .validate()
                .is_ok()
        );
        assert!(
            segment(artifact(ArtifactRole::Index), vec![])
                .validate()
                .is_err()
        );
        assert!(
            segment(
                artifact(ArtifactRole::PrimaryData),
                vec![artifact(ArtifactRole::PrimaryData)]
            )
            .validate()
            .is_err()
        );
        assert!(
            segment(
                artifact(ArtifactRole::PrimaryData),
                vec![artifact(ArtifactRole::Index)]
            )
            .validate()
            .is_err(),
            "index identity must bind to the primary object"
        );
    }

    #[test]
    fn artifacts_iterates_primary_first() {
        let primary = artifact(ArtifactRole::PrimaryData);
        let seg = segment(primary.clone(), vec![index_for(&primary)]);
        let roles: Vec<_> = seg.artifacts().map(|a| a.role).collect();
        assert_eq!(roles, vec![ArtifactRole::PrimaryData, ArtifactRole::Index]);
    }
}
