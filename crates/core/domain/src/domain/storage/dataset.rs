// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PhysicalDataset：逻辑流下的物理数据集。
//!
//! 一个逻辑流可以对应多个物理数据集（如 metrics 的 samples / rollup /
//! catalog）；FileCatalog、WAL、Buffer 与对象布局只接收 [`PhysicalDatasetId`]，
//! 不理解 logs / metrics / traces 的业务语义。

use serde::{Deserialize, Serialize};

use super::{
    ids::PhysicalDatasetId,
    type_id::{DatasetTypeId, IndexTypeId, WalCodecId},
};
use crate::shared::ids::Id;

/// 时间分区粒度。对象布局与 Segment 槽位按它切桶。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionGranularity {
    #[default]
    Hourly,
    Daily,
}

impl PartitionGranularity {
    pub const fn micros(self) -> i64 {
        match self {
            Self::Hourly => 60 * 60 * 1_000_000,
            Self::Daily => 24 * 60 * 60 * 1_000_000,
        }
    }
}

/// 分区策略。`shards` 预留水平分片位（当前恒 1，即 shard 号恒 0）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionPolicy {
    #[serde(default)]
    pub granularity: PartitionGranularity,
    #[serde(default = "default_shards")]
    pub shards: u16,
}

fn default_shards() -> u16 {
    1
}

impl Default for PartitionPolicy {
    fn default() -> Self {
        Self {
            granularity: PartitionGranularity::default(),
            shards: default_shards(),
        }
    }
}

impl PartitionPolicy {
    /// 时间戳所属分区槽位的起点（向下取整，兼容 epoch 之前的值）。
    pub const fn bucket_start_micros(&self, timestamp_micros: i64) -> i64 {
        let unit = self.granularity.micros();
        timestamp_micros.div_euclid(unit) * unit
    }
}

/// 存储策略。留空的项回落到 stream 级配置（retention）或全局默认。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoragePolicy {
    /// 数据保留天数；None = 跟随 stream retention。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u32>,
    /// 分区停止写入多久后允许封存为 Manifest；None = 跟随全局配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal_after_hours: Option<u32>,
}

/// 索引策略：该数据集应构建哪些索引。字段级规则仍在 stream schema 上。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexPolicy {
    #[serde(default)]
    pub indexers: Vec<IndexTypeId>,
}

/// Dataset 生命周期状态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetState {
    #[default]
    Active,
    Disabled,
    Deleting,
}

impl DatasetState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Deleting => "deleting",
        }
    }
}

impl std::str::FromStr for DatasetState {
    type Err = crate::shared::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "deleting" => Ok(Self::Deleting),
            other => Err(crate::shared::Error::invalid(format!(
                "unknown dataset state `{other}`"
            ))),
        }
    }
}

/// 创建物理数据集的规格（registry 默认值或调用方定制）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalDatasetSpec {
    pub dataset_type: DatasetTypeId,
    pub dataset_type_version: u32,
    pub wal_codec: WalCodecId,
    pub partition_policy: PartitionPolicy,
    pub storage_policy: StoragePolicy,
    pub index_policy: IndexPolicy,
}

/// 逻辑流下的一个物理数据集。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalDataset {
    pub id: PhysicalDatasetId,
    pub organization_id: Id,
    pub logical_stream_id: Id,

    pub dataset_type: DatasetTypeId,
    pub dataset_type_version: u32,

    pub partition_policy: PartitionPolicy,
    pub storage_policy: StoragePolicy,
    pub index_policy: IndexPolicy,

    /// 每次可见文件集合变化（flush / compaction / retention）单调递增。
    pub catalog_version: u64,
    pub state: DatasetState,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_start_floors_toward_negative_infinity() {
        let policy = PartitionPolicy::default();
        assert_eq!(policy.bucket_start_micros(3_600_000_001), 3_600_000_000);
        assert_eq!(policy.bucket_start_micros(-1), -3_600_000_000);
        let daily = PartitionPolicy {
            granularity: PartitionGranularity::Daily,
            shards: 1,
        };
        assert_eq!(
            daily.bucket_start_micros(86_400_000_000 + 5),
            86_400_000_000
        );
    }

    #[test]
    fn policies_roundtrip_json_with_defaults() {
        let policy: PartitionPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(policy, PartitionPolicy::default());
        assert_eq!(policy.shards, 1);
        let storage: StoragePolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(storage, StoragePolicy::default());
    }
}
