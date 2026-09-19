// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[cache]` —— 进程内缓存与远端 ObjectStore 的本地 range-block 缓存。

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
    /// Immutable Index Artifact key → opened index handle.
    #[serde(default = "default_index_handle_cache")]
    pub index_handle: CacheLayerSettings,
    /// `blake3(stmt + org + time_range + role)` → `QueryResult`
    #[serde(default = "default_query_result_cache")]
    pub query_result: CacheLayerSettings,
    /// 远端 ObjectStore 的本地 range-block 缓存。LocalFileSystem 自动旁路。
    #[serde(default)]
    pub object: ObjectCacheSettings,
    /// `(index_object_key, field, term)` → `count: u64`，命中跳过 `IndexHandle::count_term`。
    /// `capacity = 0` 整层关闭，行为退化为无 cache。
    #[serde(default)]
    pub tantivy_result: TantivyResultCacheSettings,
    /// `index_object_key` → `Arc<TantivyFooter>` 缓存 tantivy 归档 bytes + 解析后的 schema，
    /// IndexHandle 过期后短路掉对象存储 GET。`capacity = 0` 整层关闭。
    #[serde(default)]
    pub tantivy_footer: TantivyFooterCacheSettings,
}

fn default_index_handle_cache() -> CacheLayerSettings {
    CacheLayerSettings::new(10_000, 600)
}

fn default_query_result_cache() -> CacheLayerSettings {
    CacheLayerSettings::new(1_000, 60)
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            index_handle: default_index_handle_cache(),
            query_result: default_query_result_cache(),
            object: ObjectCacheSettings::default(),
            tantivy_result: TantivyResultCacheSettings::default(),
            tantivy_footer: TantivyFooterCacheSettings::default(),
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
/// Key = immutable Index Artifact object key、Value = lightweight parsed footer metadata.
/// `capacity = 0` 视为整层关闭：archive 重新打开时重新执行 range reads。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TantivyFooterCacheSettings {
    #[serde(default = "default_tantivy_footer_capacity")]
    pub capacity: u64,
    #[serde(default = "default_tantivy_footer_ttl_secs")]
    pub ttl_secs: u32,
}

fn default_tantivy_footer_capacity() -> u64 {
    // Footer values contain Puffin metadata, footer payload and schema only (roughly a few KB),
    // not complete archive bytes.
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

/// `[cache.object]` —— 所有远端 Artifact/Manifest 共享的本地 block cache。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCacheSettings {
    #[serde(default = "default_object_cache_enabled")]
    pub enabled: bool,
    #[serde(default = "default_object_cache_root")]
    pub root: PathBuf,
    #[serde(default = "default_object_cache_max_bytes")]
    pub max_bytes: u64,
    #[serde(default = "default_object_cache_block_size_bytes")]
    pub block_size_bytes: u64,
    #[serde(default = "default_object_cache_min_free_bytes")]
    pub min_free_bytes: u64,
    #[serde(default = "default_object_cache_max_concurrent_fetches")]
    pub max_concurrent_fetches: usize,
    #[serde(default = "default_object_cache_max_concurrent_fetches_per_object")]
    pub max_concurrent_fetches_per_object: usize,
}

fn default_object_cache_enabled() -> bool {
    true
}

fn default_object_cache_root() -> PathBuf {
    PathBuf::from("./data/cache/objects")
}

fn default_object_cache_max_bytes() -> u64 {
    10 * 1024 * 1024 * 1024
}

fn default_object_cache_block_size_bytes() -> u64 {
    4 * 1024 * 1024
}

fn default_object_cache_min_free_bytes() -> u64 {
    1024 * 1024 * 1024
}

fn default_object_cache_max_concurrent_fetches() -> usize {
    16
}

fn default_object_cache_max_concurrent_fetches_per_object() -> usize {
    4
}

impl Default for ObjectCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_object_cache_enabled(),
            root: default_object_cache_root(),
            max_bytes: default_object_cache_max_bytes(),
            block_size_bytes: default_object_cache_block_size_bytes(),
            min_free_bytes: default_object_cache_min_free_bytes(),
            max_concurrent_fetches: default_object_cache_max_concurrent_fetches(),
            max_concurrent_fetches_per_object:
                default_object_cache_max_concurrent_fetches_per_object(),
        }
    }
}

impl ObjectCacheSettings {
    pub fn is_effectively_enabled(&self) -> bool {
        self.enabled && self.max_bytes > 0
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        const MIB: u64 = 1024 * 1024;
        if !(MIB..=16 * MIB).contains(&self.block_size_bytes) {
            anyhow::bail!("cache.object.block_size_bytes must be between 1 MiB and 16 MiB");
        }
        if self.max_concurrent_fetches == 0 {
            anyhow::bail!("cache.object.max_concurrent_fetches must be greater than zero");
        }
        if self.max_concurrent_fetches_per_object == 0 {
            anyhow::bail!(
                "cache.object.max_concurrent_fetches_per_object must be greater than zero"
            );
        }
        Ok(())
    }
}
