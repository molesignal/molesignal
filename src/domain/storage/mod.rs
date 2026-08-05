// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 存储上下文：parquet 文件元信息、分区、清单。
//!
//! 实际字节存放在对象存储中；元数据库里只保留 [`ParquetFileMeta`]，
//! querier 查询时先扫 ParquetFileMeta 做分区裁剪，再去对象存储拉数据。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::stream::StreamType,
    shared::{Result, ids::Id, time::TimeRange},
};

mod physical;

pub use physical::{
    HOUR_MICROS, PhysicalDatasetKind, hour_partition_path, hour_start_micros,
    logical_query_datasets,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetFileMeta {
    pub id: Id,
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    /// 同一逻辑 stream 内部的物理数据用途。默认查询只读取 `raw`。
    #[serde(default)]
    pub dataset_kind: PhysicalDatasetKind,
    /// 对象存储中的 key，例如
    /// `{org}/{stream_type}/{dataset_kind}/{stream}/{YYYY}/{MM}/{DD}/{HH}/{id}.parquet`。
    pub object_key: String,
    pub time_range: TimeRange,
    pub rows: u64,
    pub size_bytes: u64,
    /// 该文件用到的最小/最大值索引（按字段名 → JSON）
    pub min_values: serde_json::Map<String, serde_json::Value>,
    pub max_values: serde_json::Map<String, serde_json::Value>,
    /// 是否已被 compactor 合并掉
    pub deleted: bool,
}

#[async_trait]
pub trait ParquetFileMetaRepository: Send + Sync {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()>;

    /// 原子插入同一批 flush 生成的多个小时文件。默认实现仅用于内存仓库；生产仓库必须
    /// 覆盖为单事务，避免 WAL 已提交但 ParquetFileMeta 只落了一部分。
    async fn insert_many(&self, files: Vec<ParquetFileMeta>) -> Result<()> {
        for file in files {
            self.insert(file).await?;
        }
        Ok(())
    }

    /// 查询原始物理数据集。派生摘要必须显式调用 [`Self::find_dataset`]。
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>>;

    async fn find_dataset(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .find(org_id, stream, stream_type, time_range)
            .await?
            .into_iter()
            .filter(|file| file.dataset_kind == dataset_kind)
            .collect())
    }

    /// compactor 合并落库：仅当 `merged_ids` 全部仍存活时，才把它们标删并写入
    /// `new_files`；只要有一条已被标删就整体回滚并返回 [`crate::shared::Error::Conflict`]。
    ///
    /// 这个全有全无的语义是多副本部署下的正确性前提：两个实例同时合并同一组源文件时，
    /// 若双方都能提交，就会留下两个含相同行的存活文件，查询侧行数翻倍。冲突方必须删掉
    /// 自己刚写出的新对象。幂等的清理场景（幽灵文件、retention）用 [`Self::mark_deleted`]。
    async fn replace(&self, merged_ids: &[Id], new_files: Vec<ParquetFileMeta>) -> Result<()>;

    /// 幂等标删：把 `ids` 里仍存活的行标 `deleted`，返回实际标删数。
    /// 部分命中不是错误——并发写者已经标删过其中一部分是预期情况。
    async fn mark_deleted(&self, ids: &[Id]) -> Result<usize>;
}

/// Partition 粒度：daily 或 hourly。dump 写出时按此粒度决定 `partition_key` 形态：
/// daily → `YYYY-MM-DD`，hourly → `YYYY-MM-DD-HH`。同一 `(org, stream, stream_type)`
/// 允许混合粒度共存（change `parquet-file-meta-dump-columnar`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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

/// 一行 `parquet_file_meta_dump` 索引：把 `(org, stream, stream_type, partition_level, partition_key)`
/// 分区已被序列化的 ParquetFileMeta 列表指向 object_store 上一份 columnar parquet。
/// **不存储任何原始 ParquetFileMeta 字段** —— 索引行只是定位 dump.parquet 的指针 +
/// PG 侧裁剪所需的最小元信息（`min_ts_micros / max_ts_micros / deleted`）；
/// 真正的 ParquetFileMeta 行在 `object_key` 指向的 parquet 文件里。
///
/// `date` 字段仅在 `partition_level = Daily` 时与 `partition_key` 重合；为了
/// 渐进迁移保留为人类可读冗余列，正式裁剪走 `partition_key + partition_level`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetFileMetaDumpRow {
    pub id: Id,
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    /// dump 仍按物理数据集隔离，避免 raw 与派生摘要共享对象或缓存键。
    pub dataset_kind: PhysicalDatasetKind,
    /// 分区日期（UTC，YYYY-MM-DD），与 parquet 写入分区一致。
    pub date: String,
    /// 指向
    /// `{org}/_parquet_file_meta_dump/{stream_type}/{dataset_kind}/{stream}/{partition_key}.parquet`。
    pub object_key: String,
    pub rows_in_dump: u32,
    pub created_at: crate::shared::time::TimestampMicros,
    /// 分区粒度（change `parquet-file-meta-dump-columnar`）。
    #[serde(default)]
    pub partition_level: PartitionLevel,
    /// 分区 key：daily → `YYYY-MM-DD`，hourly → `YYYY-MM-DD-HH`。
    #[serde(default)]
    pub partition_key: String,
    /// `true` 表示这条索引行的 dump 文件已被 `delete_by_time_range` 重写/作废，
    /// 查询路径不应再 GET 对应 object。
    #[serde(default)]
    pub deleted: bool,
    /// dump 文件覆盖的 ParquetFileMeta 行中 `time_range.start` 的最小值（micros）。
    #[serde(default)]
    pub min_ts_micros: i64,
    /// dump 文件覆盖的 ParquetFileMeta 行中 `time_range.end` 的最大值（micros）。
    #[serde(default)]
    pub max_ts_micros: i64,
    /// dump parquet 上传字节数；compactor / 运营观测用。
    #[serde(default)]
    pub size_bytes: i64,
    /// dump 行最后一次更新时间（micros）；重写时 bump。
    #[serde(default)]
    pub updated_at_micros: i64,
}

/// `parquet_file_meta_dump_stats`：per dump file 的聚合统计（change `parquet-file-meta-dump-columnar`）。
/// 用于 stream stats 服务在不打开 dump parquet 的前提下回答冷分区聚合查询。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParquetFileMetaDumpStats {
    pub object_key: String,
    pub rows_total: i64,
    pub files_total: i64,
    pub time_start_micros: i64,
    pub time_end_micros: i64,
    pub storage_size_bytes: i64,
    pub updated_at_micros: i64,
}

/// `parquet_file_meta_dump` 索引表的访问入口（spec `storage/ParquetFileMeta Dump Spillover`）。
///
/// 实装由 infra crate 提供（PG）；domain 层只暴露 trait 让 dumper worker 与查询
/// 路径透明地拿到 cold-tier 指针。所有方法是 best-effort 失败不破坏调用方语义。
///
/// 新增方法（change `parquet-file-meta-dump-columnar`）暂以 `unimplemented!` 默认实现，
/// 让中间态 build 通过；task 6.1-6.4 落地具体 PG 实装时移除 default。
#[async_trait]
pub trait ParquetFileMetaDumpRepository: Send + Sync {
    /// dumper worker 写完 parquet 后插入一条索引行；与主表 `parquet_file_meta` 的 DELETE
    /// 必须在同一事务中执行才算 atomic dump（具体由 caller 通过 `tx` 自己拼）。
    async fn insert(&self, row: ParquetFileMetaDumpRow) -> Result<()>;

    /// 查询路径用：列出 `(org, stream, stream_type, dataset_kind)` 落在
    /// `time_range` 内且
    /// `deleted = false` 的索引行；caller 拿到 `object_key` 后再去 object_store
    /// 取真正的 dump.parquet。
    async fn find_by_time_range(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMetaDumpRow>>;

    /// GC 入口：dumper 二次扫描 / retention 协同时删一条索引行。
    async fn delete(&self, id: &Id) -> Result<()>;

    /// 把指定 `object_key` 的 dump 行标记 `deleted = true`。
    /// `delete_by_time_range` 重写时调用；查询路径据此跳过老对象。
    async fn mark_deleted(&self, _object_key: &str) -> Result<()> {
        unimplemented!("mark_deleted: pending change parquet-file-meta-dump-columnar task 6.2")
    }

    /// 一笔 tx 内写入 dump 行 + 对应 stats 行。取代旧 `insert`，未来 task 6.3 切换 caller。
    async fn upsert_dump(
        &self,
        _row: ParquetFileMetaDumpRow,
        _stats: ParquetFileMetaDumpStats,
    ) -> Result<()> {
        unimplemented!("upsert_dump: pending change parquet-file-meta-dump-columnar task 6.3")
    }

    /// `delete_by_time_range` 用：一笔 tx 内插新 dump + stats、标老 dump deleted、
    /// 删老 stats（三步合一）。
    async fn insert_rewrite(
        &self,
        _old_object_key: &str,
        _new_row: ParquetFileMetaDumpRow,
        _new_stats: ParquetFileMetaDumpStats,
    ) -> Result<()> {
        unimplemented!("insert_rewrite: pending change parquet-file-meta-dump-columnar task 6.4")
    }

    /// 列出 `deleted = true` 但 object_store 上对应对象尚未删除的索引行；
    /// 由异步 cleanup sweep 消费（实装 follow-up）。
    async fn pending_object_deletes(&self, _limit: u32) -> Result<Vec<ParquetFileMetaDumpRow>> {
        unimplemented!(
            "pending_object_deletes: pending change parquet-file-meta-dump-columnar task 8.x"
        )
    }
}
