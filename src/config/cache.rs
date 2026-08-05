// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[cache]` —— 进程内多层缓存（ParquetFileMeta / parquet meta / query result /
//! tantivy result+footer / ParquetFileMeta dump）与 `[cache.disk_cache]` 本地磁盘二级缓存。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 单层缓存容量与 TTL。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheLayerSettings {
    pub capacity: u64,
    pub ttl_secs: u64,
}

impl CacheLayerSettings {
    pub const fn new(capacity: u64, ttl_secs: u64) -> Self {
        Self { capacity, ttl_secs }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettings {
    /// `(org, stream, stream_type, time_bucket_hour)` → `Vec<ParquetFileMeta>`
    #[serde(default = "default_parquet_file_meta_cache")]
    pub parquet_file_meta: CacheLayerSettings,
    /// `object_key` → `Arc<ParquetMetaData>`（含 Tantivy IndexHandle 复用）
    #[serde(default = "default_parquet_meta_cache")]
    pub parquet_meta: CacheLayerSettings,
    /// `blake3(stmt + org + time_range + role)` → `QueryResult`
    #[serde(default = "default_query_result_cache")]
    pub query_result: CacheLayerSettings,
    /// 本地 NVMe parquet 二级缓存（spec `caching/Parquet Disk Cache`）。
    /// 默认启用，占盘上限 10 GB，目录 `./data/cache/parquet`。
    #[serde(default)]
    pub disk_cache: DiskCacheSettings,
    /// `(index_object_key, field, term)` → `count: u64`，命中跳过 `IndexHandle::count_term`。
    /// `capacity = 0` 整层关闭，行为退化为无 cache。
    #[serde(default)]
    pub tantivy_result: TantivyResultCacheSettings,
    /// `index_object_key` → `Arc<TantivyFooter>` 缓存 tantivy 归档 bytes + 解析后的 schema，
    /// IndexHandle 过期后短路掉对象存储 GET。`capacity = 0` 整层关闭。
    #[serde(default)]
    pub tantivy_footer: TantivyFooterCacheSettings,
    /// `(org, stream, stream_type, partition_level, partition_key)` →
    /// `Arc<Vec<ParquetFileMeta>>` 缓存冷分区 dump parquet 解析结果。
    /// `capacity = 0` 整层关闭（change `parquet-file-meta-dump-columnar`）。
    #[serde(default)]
    pub parquet_file_meta_dump: ParquetFileMetaDumpCacheSettings,
}

fn default_parquet_file_meta_cache() -> CacheLayerSettings {
    CacheLayerSettings::new(100_000, 60)
}

fn default_parquet_meta_cache() -> CacheLayerSettings {
    CacheLayerSettings::new(10_000, 600)
}

fn default_query_result_cache() -> CacheLayerSettings {
    CacheLayerSettings::new(1_000, 60)
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            parquet_file_meta: default_parquet_file_meta_cache(),
            parquet_meta: default_parquet_meta_cache(),
            query_result: default_query_result_cache(),
            disk_cache: DiskCacheSettings::default(),
            tantivy_result: TantivyResultCacheSettings::default(),
            tantivy_footer: TantivyFooterCacheSettings::default(),
            parquet_file_meta_dump: ParquetFileMetaDumpCacheSettings::default(),
        }
    }
}

/// `[cache.parquet_file_meta_dump]` —— 冷分区 ParquetFileMeta dump 进程内缓存。
///
/// Key = `(org, stream, stream_type, partition_level, partition_key)`、
/// Value = `Arc<Vec<ParquetFileMeta>>`。`capacity = 0` 视为整层关闭：每次冷查都重新
/// GET + parse dump parquet。新加字段，与 `tantivy_result/tantivy_footer` 同款形态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetFileMetaDumpCacheSettings {
    #[serde(default = "default_parquet_file_meta_dump_cache_capacity")]
    pub capacity: u64,
    #[serde(default = "default_parquet_file_meta_dump_cache_ttl_secs")]
    pub ttl_secs: u32,
}

fn default_parquet_file_meta_dump_cache_capacity() -> u64 {
    10_000
}
fn default_parquet_file_meta_dump_cache_ttl_secs() -> u32 {
    600
}

impl Default for ParquetFileMetaDumpCacheSettings {
    fn default() -> Self {
        Self {
            capacity: default_parquet_file_meta_dump_cache_capacity(),
            ttl_secs: default_parquet_file_meta_dump_cache_ttl_secs(),
        }
    }
}

/// `[cache.tantivy_result]` —— tantivy 谓词结果 cache。
///
/// Key = `(index_object_key, field, term)`、Value = `count: u64`。`capacity = 0`
/// 视为整层关闭：`TantivyPruner::prune` 不查 cache、不写 cache，直接走 tantivy。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TantivyResultCacheSettings {
    #[serde(default = "default_tantivy_result_capacity")]
    pub capacity: u64,
    #[serde(default = "default_tantivy_result_ttl_secs")]
    pub ttl_secs: u32,
}

fn default_tantivy_result_capacity() -> u64 {
    1_000_000
}
fn default_tantivy_result_ttl_secs() -> u32 {
    600
}

impl Default for TantivyResultCacheSettings {
    fn default() -> Self {
        Self {
            capacity: default_tantivy_result_capacity(),
            ttl_secs: default_tantivy_result_ttl_secs(),
        }
    }
}

/// `[cache.tantivy_footer]` —— tantivy 归档 footer cache。
///
/// Key = `index_object_key`、Value = `Arc<TantivyFooter>`（archive bytes + schema 等元数据）。
/// `capacity = 0` 视为整层关闭：archive 重新打开时永远走对象存储 GET。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TantivyFooterCacheSettings {
    #[serde(default = "default_tantivy_footer_capacity")]
    pub capacity: u64,
    #[serde(default = "default_tantivy_footer_ttl_secs")]
    pub ttl_secs: u32,
}

fn default_tantivy_footer_capacity() -> u64 {
    // change `tantivy-puffin-migration`：footer value 从「整 archive bytes」（10s~100s KB）
    // 缩到「puffin meta + footer payload + schema」（~几 KB），同容量内存预算下可缓更多 entry。
    100_000
}
fn default_tantivy_footer_ttl_secs() -> u32 {
    3600
}

impl Default for TantivyFooterCacheSettings {
    fn default() -> Self {
        Self {
            capacity: default_tantivy_footer_capacity(),
            ttl_secs: default_tantivy_footer_ttl_secs(),
        }
    }
}

/// `[cache.disk_cache]` —— Parquet 本地磁盘二级缓存。
///
/// `enabled = false` 或 `max_size_gb = 0` 视为整层关闭：bootstrap 不实例化
/// `ParquetDiskCache`，缓存目录也不会被创建。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskCacheSettings {
    #[serde(default = "default_disk_cache_dir")]
    pub dir: PathBuf,
    /// 0 = 关闭（不建缓存、不创建目录）。默认 10 GB（启用）。
    #[serde(default = "default_disk_cache_max_size_gb")]
    pub max_size_gb: u32,
}

fn default_disk_cache_dir() -> PathBuf {
    PathBuf::from("./data/cache/parquet")
}

fn default_disk_cache_max_size_gb() -> u32 {
    10
}

impl Default for DiskCacheSettings {
    fn default() -> Self {
        Self {
            dir: default_disk_cache_dir(),
            max_size_gb: default_disk_cache_max_size_gb(),
        }
    }
}

impl DiskCacheSettings {
    /// 启用 ⟺ `max_size_gb > 0`（0 = 关闭）。
    pub fn is_effectively_enabled(&self) -> bool {
        self.max_size_gb > 0
    }

    /// 容量换算为字节，u64 防止 u32 溢出。
    pub fn max_size_bytes(&self) -> u64 {
        u64::from(self.max_size_gb) * 1024 * 1024 * 1024
    }
}
