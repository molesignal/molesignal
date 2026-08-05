// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 冷分区 ParquetFileMeta dump 进程内缓存（spec `caching/ParquetFileMeta Dump In-Process Cache`）。
//!
//! Key = `(org_id, stream, stream_type, dataset_kind, partition_level, partition_key)`，
//! Value = `Arc<Vec<ParquetFileMeta>>`（dump 文件解析后**全集**，不做 time_range 预过滤；
//! caller 用 `TimeRange` 在 cache hit 后本地 filter）。
//!
//! `capacity = 0` 整层关闭：`get` 永远 `None`、`insert` 是 no-op、`invalidate*`
//! 同样 no-op。

use std::{sync::Arc, time::Duration};

use moka::future::Cache;

use super::super::metrics::CacheMetrics;
use crate::{
    config::ParquetFileMetaDumpCacheSettings,
    domain::{
        storage::{ParquetFileMeta, PartitionLevel, PhysicalDatasetKind},
        stream::StreamType,
    },
    shared::ids::Id,
};

/// `Arc<str>` 字段让 clone 廉价；StreamType / PartitionLevel 是 `Copy`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DumpCacheKey {
    pub org_id: Arc<str>,
    pub stream: Arc<str>,
    pub stream_type: StreamType,
    pub dataset_kind: PhysicalDatasetKind,
    pub partition_level: PartitionLevel,
    pub partition_key: Arc<str>,
}

impl DumpCacheKey {
    pub fn new(
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        partition_level: PartitionLevel,
        partition_key: &str,
    ) -> Self {
        Self {
            org_id: Arc::from(org_id.0.as_str()),
            stream: Arc::from(stream),
            stream_type,
            dataset_kind,
            partition_level,
            partition_key: Arc::from(partition_key),
        }
    }
}

pub struct ParquetFileMetaDumpCache {
    inner: Option<Cache<DumpCacheKey, Arc<Vec<ParquetFileMeta>>>>,
    metrics: CacheMetrics,
}

pub type ParquetFileMetaDumpCacheRef = Arc<ParquetFileMetaDumpCache>;

impl ParquetFileMetaDumpCache {
    pub fn new(settings: &ParquetFileMetaDumpCacheSettings) -> Self {
        let metrics = CacheMetrics::register("parquet_file_meta_dump");
        if settings.capacity == 0 {
            return Self {
                inner: None,
                metrics,
            };
        }
        let evict = metrics.evictions();
        let cache = Cache::builder()
            .max_capacity(settings.capacity)
            .time_to_live(Duration::from_secs(u64::from(settings.ttl_secs)))
            .support_invalidation_closures()
            .async_eviction_listener(move |_k, _v, _cause| {
                let evict = evict.clone();
                Box::pin(async move {
                    evict.inc();
                })
            })
            .build();
        Self {
            inner: Some(cache),
            metrics,
        }
    }

    pub async fn get(&self, key: &DumpCacheKey) -> Option<Arc<Vec<ParquetFileMeta>>> {
        let cache = self.inner.as_ref()?;
        match cache.get(key).await {
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

    pub async fn insert(&self, key: DumpCacheKey, value: Arc<Vec<ParquetFileMeta>>) {
        if let Some(cache) = self.inner.as_ref() {
            cache.insert(key, value).await;
        }
    }

    pub async fn invalidate(&self, key: &DumpCacheKey) {
        if let Some(cache) = self.inner.as_ref() {
            cache.invalidate(key).await;
        }
    }

    /// 失效一个 `(org, stream, stream_type, dataset_kind, partition_level, partition_key)` 入口。
    /// 跟 `invalidate` 等价（提供给 caller 表达"按 partition 失效"语义）。
    pub async fn invalidate_partition(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        partition_level: PartitionLevel,
        partition_key: &str,
    ) {
        let key = DumpCacheKey::new(
            org_id,
            stream,
            stream_type,
            dataset_kind,
            partition_level,
            partition_key,
        );
        self.invalidate(&key).await;
    }

    /// 失效该 `(org, stream, stream_type)` 下所有 partition 的 entry。
    /// 用 moka 的 invalidation closure；O(N) on cache size。
    pub fn invalidate_stream(&self, org_id: &Id, stream: &str, stream_type: StreamType) {
        let Some(cache) = self.inner.as_ref() else {
            return;
        };
        let org: Arc<str> = Arc::from(org_id.0.as_str());
        let stream: Arc<str> = Arc::from(stream);
        let _ = cache.invalidate_entries_if(move |k, _v| {
            k.org_id == org && k.stream == stream && k.stream_type == stream_type
        });
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

    fn fm(seed: u64) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::from_string(format!("fm-{seed}")),
            org_id: Id::from_string("org-x"),
            stream: "app".into(),
            stream_type: StreamType::Logs,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: format!("k/{seed}"),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 0,
            size_bytes: 0,
            min_values: serde_json::Map::new(),
            max_values: serde_json::Map::new(),
            deleted: false,
        }
    }

    fn key(partition_key: &str) -> DumpCacheKey {
        DumpCacheKey::new(
            &Id::from_string("org-x"),
            "app",
            StreamType::Logs,
            PhysicalDatasetKind::Raw,
            PartitionLevel::Daily,
            partition_key,
        )
    }

    #[tokio::test]
    async fn hit_after_insert() {
        let cache = ParquetFileMetaDumpCache::new(&ParquetFileMetaDumpCacheSettings {
            capacity: 10,
            ttl_secs: 60,
        });
        let k = key("2026-01-15");
        assert!(cache.get(&k).await.is_none());
        cache.insert(k.clone(), Arc::new(vec![fm(1)])).await;
        let got = cache.get(&k).await.expect("hit");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, Id::from_string("fm-1"));
    }

    #[tokio::test]
    async fn capacity_zero_is_noop() {
        let cache = ParquetFileMetaDumpCache::new(&ParquetFileMetaDumpCacheSettings {
            capacity: 0,
            ttl_secs: 60,
        });
        let k = key("2026-01-15");
        cache.insert(k.clone(), Arc::new(vec![fm(1)])).await;
        assert!(cache.get(&k).await.is_none());
    }

    #[tokio::test]
    async fn invalidate_drops_entry() {
        let cache = ParquetFileMetaDumpCache::new(&ParquetFileMetaDumpCacheSettings {
            capacity: 10,
            ttl_secs: 60,
        });
        let k = key("2026-01-15");
        cache.insert(k.clone(), Arc::new(vec![fm(1)])).await;
        cache.invalidate(&k).await;
        assert!(cache.get(&k).await.is_none());
    }

    #[tokio::test]
    async fn invalidate_partition_matches_composite_key() {
        let cache = ParquetFileMetaDumpCache::new(&ParquetFileMetaDumpCacheSettings {
            capacity: 10,
            ttl_secs: 60,
        });
        let k = key("2026-01-15");
        cache.insert(k.clone(), Arc::new(vec![fm(1)])).await;
        cache
            .invalidate_partition(
                &Id::from_string("org-x"),
                "app",
                StreamType::Logs,
                PhysicalDatasetKind::Raw,
                PartitionLevel::Daily,
                "2026-01-15",
            )
            .await;
        assert!(cache.get(&k).await.is_none());
    }

    #[tokio::test]
    async fn ttl_expiry_evicts() {
        let cache = ParquetFileMetaDumpCache::new(&ParquetFileMetaDumpCacheSettings {
            capacity: 10,
            ttl_secs: 1,
        });
        let k = key("2026-01-15");
        cache.insert(k.clone(), Arc::new(vec![fm(1)])).await;
        assert!(cache.get(&k).await.is_some());
        // moka 的 ttl 是 lazy 的；睡一下再 get 触发淘汰。
        tokio::time::sleep(Duration::from_millis(1200)).await;
        // run housekeeping (moka 在 get 时也会触发)
        let _ = cache.get(&k).await;
        // 第二次 get 期望 None
        assert!(cache.get(&k).await.is_none());
    }
}
