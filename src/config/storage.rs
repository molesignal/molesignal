// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[storage]` —— 与 `[store]` 解耦的上层存储 capability（目前为 ParquetFileMeta 冷分区下沉）。

use serde::{Deserialize, Serialize};

use super::yes;

/// `[storage]` —— 存储层子能力配置（与 `[store]` 解耦：`store` 负责底层元/对象
/// 存储凭据，`storage` 负责上层 capability 行为）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageSettings {
    /// `[storage.parquet_file_meta_dump]`：ParquetFileMeta 冷分区下沉到 object_store（spec
    /// `storage/ParquetFileMeta Dump Spillover`）。
    #[serde(default)]
    pub parquet_file_meta_dump: ParquetFileMetaDumpSettings,
}

/// `[storage.parquet_file_meta_dump]` —— ParquetFileMeta dump worker 行为开关与速率。
///
/// `enabled = false` 时 worker 不启动、查询路径回退到只读主表；
/// 已 dump 的对象与索引行保留不动（重启 enabled=true 时无需任何手工迁移）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetFileMetaDumpSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_parquet_file_meta_dump_cold_after_days")]
    pub cold_after_days: u32,
    #[serde(default = "default_parquet_file_meta_dump_interval_secs")]
    pub interval_secs: u32,
    #[serde(default = "default_parquet_file_meta_dump_max_partitions_per_tick")]
    pub max_partitions_per_tick: u32,
    /// Dump partition 粒度。`daily` 默认，hourly 高频小窗口场景下减少跨 partition 扫。
    /// 同 stream 允许混合粒度共存（change `parquet-file-meta-dump-columnar`）。
    #[serde(default)]
    pub partition_level: PartitionLevel,
}

/// Dump partition 粒度。镜像 `crate::domain::storage::PartitionLevel`，
/// config 不依赖 domain，留独立 enum 解耦；运行时由 infra 做 From 转换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionLevel {
    #[default]
    Daily,
    Hourly,
}

impl PartitionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PartitionLevel::Daily => "daily",
            PartitionLevel::Hourly => "hourly",
        }
    }
}

fn default_parquet_file_meta_dump_cold_after_days() -> u32 {
    30
}
fn default_parquet_file_meta_dump_interval_secs() -> u32 {
    3600
}
fn default_parquet_file_meta_dump_max_partitions_per_tick() -> u32 {
    100
}

impl Default for ParquetFileMetaDumpSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cold_after_days: default_parquet_file_meta_dump_cold_after_days(),
            interval_secs: default_parquet_file_meta_dump_interval_secs(),
            max_partitions_per_tick: default_parquet_file_meta_dump_max_partitions_per_tick(),
            partition_level: PartitionLevel::default(),
        }
    }
}
