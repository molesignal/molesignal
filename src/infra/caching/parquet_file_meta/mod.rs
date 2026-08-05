// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `ParquetFileMetaCache`：`(org, stream, stream_type, time_bucket_hour)` → `Arc<Vec<ParquetFileMeta>>`。
//!
//! 外层 `DashMap` by prefix，内层 moka cache by time bucket —— moka 不支持前缀失效，
//! sub-map 设计让 `invalidate_prefix` 把整组 entry 一次丢弃。
//!
//! # 现状：已实现，但**未接入查询路径**
//!
//! 生产代码从不 `get` / `insert` 本缓存；唯一的调用是 ingester flush 后的
//! [`ParquetFileMetaCache::invalidate_prefix`]，即对一个永远为空的 map 做删除。这不是接线遗漏，
//! 下面两点都不是"补失效钩子"能绕过的：
//!
//! 1. **失效是进程内的，跨不了角色。** `Ingester` / `Querier` / `Compactor` 是可分离部署的
//!    独立角色（见 [`crate::config::Role`]，默认 `Standalone` 才同进程）。compactor 合并
//!    完会 `store.delete` 掉旧对象，但它的 `invalidate_prefix` 只作用于自己那个进程 ——
//!    querier 节点会在整个 TTL 内继续列出已被删掉的对象，查询随即读到 404。
//!
//!    这与 `TantivyResultCache` / `TantivyFooterCache` 的处境**不同**：那两层按
//!    `index_object_key` 内容寻址，key 对应的字节不可变，陈旧条目只是永不再被命中的垃圾，
//!    失效纯粹是回收内存。本缓存的 value 是**可变的文件列表**，陈旧即错误答案。
//!
//! 2. **收益存疑。** [`crate::infra::persistence::repositories::parquet_file_meta`] 的 `find` 有覆盖
//!    索引 `idx_parquet_file_meta_scan(org_id, stream, stream_type, time_start_micros,
//!    time_end_micros)`，三个等值谓词加时间范围全被它吃掉，是索引区间扫描而非全表扫，
//!    单次只返回该 stream 在窗口内的那几十行。
//!
//! 另有一个接线时才会碰到的形状问题：本缓存 key 是**小时桶**，而 `find` 收的是任意时间
//! 范围，中间还需要一层桶分解 + 结果按桶切分。
//!
//! 要真正接线，得先有跨节点失效（pub/sub 或 PG 里的 per-stream 版本号），或把语义降级成
//! "读到 404 就失效重试"。在那之前不要仅凭本模块存在就假定 parquet_file_meta 查询被缓存兜住了。

use std::{sync::Arc, time::Duration};

pub mod dump;

use dashmap::DashMap;
use moka::future::Cache;

use super::metrics::CacheMetrics;
use crate::{
    config::CacheLayerSettings,
    domain::{storage::ParquetFileMeta, stream::StreamType},
    shared::ids::Id,
};

/// ParquetFileMetaCache 的前缀键：(org_id, stream_name, stream_type)。
pub type ParquetFileMetaPrefix = (Id, String, StreamType);

/// 小时级时间桶（向下取整的 hour 序号），用作 ParquetFileMetaCache 内层 key。
pub type TimeBucketHour = i64;

#[inline]
pub fn bucket_of_hour(ts_micros: i64) -> TimeBucketHour {
    ts_micros.div_euclid(3_600_000_000)
}

type ParquetFileMetaSubCache = Cache<TimeBucketHour, Arc<Vec<ParquetFileMeta>>>;

pub struct ParquetFileMetaCache {
    prefixes: DashMap<ParquetFileMetaPrefix, Arc<ParquetFileMetaSubCache>>,
    settings: CacheLayerSettings,
    metrics: CacheMetrics,
}

impl ParquetFileMetaCache {
    pub fn new(settings: CacheLayerSettings) -> Self {
        Self {
            prefixes: DashMap::new(),
            settings,
            metrics: CacheMetrics::register("parquet_file_meta"),
        }
    }

    fn sub_cache(&self, prefix: &ParquetFileMetaPrefix) -> Arc<ParquetFileMetaSubCache> {
        if let Some(c) = self.prefixes.get(prefix) {
            return c.clone();
        }
        let evict = self.metrics.evictions();
        let cache: ParquetFileMetaSubCache = Cache::builder()
            .max_capacity(self.settings.capacity)
            .time_to_live(Duration::from_secs(self.settings.ttl_secs))
            .async_eviction_listener(move |_k, _v, _cause| {
                let evict = evict.clone();
                Box::pin(async move {
                    evict.inc();
                })
            })
            .build();
        let arc = Arc::new(cache);
        self.prefixes.insert(prefix.clone(), arc.clone());
        arc
    }

    pub async fn get(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        bucket: TimeBucketHour,
    ) -> Option<Arc<Vec<ParquetFileMeta>>> {
        let prefix = (org_id.clone(), stream.to_string(), stream_type);
        if !self.prefixes.contains_key(&prefix) {
            self.metrics.record_miss();
            return None;
        }
        let cache = self.sub_cache(&prefix);
        match cache.get(&bucket).await {
            Some(v) => {
                self.metrics.record_hit();
                Some(v)
            }
            None => {
                self.metrics.record_miss();
                None
            }
        }
    }

    pub async fn insert(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        bucket: TimeBucketHour,
        files: Arc<Vec<ParquetFileMeta>>,
    ) {
        let prefix = (org_id.clone(), stream.to_string(), stream_type);
        let cache = self.sub_cache(&prefix);
        cache.insert(bucket, files).await;
    }

    /// 把 (org, stream, stream_type) 整组 entry 一次失效。
    ///
    /// **注意**：生产路径里只有 ingester flush 成功后会调它，`ParquetFileMetaRepository` 的
    /// `insert` / `replace` / `mark_deleted` 写路径**并没有**接（compactor 的合并、
    /// retention 标删都不会触发失效）。由于本缓存至今未被 `insert` 填充，这目前是对空 map
    /// 做删除、无实际效果。接线前必读模块文档里的两条前提。
    pub fn invalidate_prefix(&self, org_id: &Id, stream: &str, stream_type: StreamType) {
        let prefix = (org_id.clone(), stream.to_string(), stream_type);
        self.prefixes.remove(&prefix);
    }

    pub fn hit_ratio(&self) -> f64 {
        let (h, m) = self.metrics.snapshot();
        if h + m == 0 {
            0.0
        } else {
            h as f64 / (h + m) as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::{TimeRange, TimestampMicros};

    fn fm(stream: &str, st: StreamType) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::new(),
            org_id: Id::from_string("org-x"),
            stream: stream.to_string(),
            stream_type: st,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: format!("k/{}", Id::new()),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 0,
            size_bytes: 0,
            min_values: serde_json::Map::new(),
            max_values: serde_json::Map::new(),
            deleted: false,
        }
    }

    #[tokio::test]
    async fn parquet_file_meta_cache_tracks_hits_and_misses() {
        let c = ParquetFileMetaCache::new(CacheLayerSettings::new(100, 60));
        let org = Id::from_string("org-x");
        // miss
        assert!(c.get(&org, "app", StreamType::Logs, 0).await.is_none());
        // insert + hit
        c.insert(
            &org,
            "app",
            StreamType::Logs,
            0,
            Arc::new(vec![fm("app", StreamType::Logs)]),
        )
        .await;
        assert!(c.get(&org, "app", StreamType::Logs, 0).await.is_some());
        assert!(c.get(&org, "app", StreamType::Logs, 1).await.is_none()); // miss on bucket 1
        let r = c.hit_ratio();
        assert!(r > 0.0 && r < 1.0, "hit_ratio mid range, got {r}");
    }

    #[tokio::test]
    async fn parquet_file_meta_invalidate_prefix_drops_sub_map() {
        let c = ParquetFileMetaCache::new(CacheLayerSettings::new(100, 60));
        let org = Id::from_string("org-x");
        c.insert(
            &org,
            "app",
            StreamType::Logs,
            0,
            Arc::new(vec![fm("app", StreamType::Logs)]),
        )
        .await;
        c.insert(
            &org,
            "app",
            StreamType::Logs,
            1,
            Arc::new(vec![fm("app", StreamType::Logs)]),
        )
        .await;
        c.invalidate_prefix(&org, "app", StreamType::Logs);
        assert!(c.get(&org, "app", StreamType::Logs, 0).await.is_none());
        assert!(c.get(&org, "app", StreamType::Logs, 1).await.is_none());
    }
}
