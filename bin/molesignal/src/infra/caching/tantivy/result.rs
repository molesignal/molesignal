// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 谓词结果 cache（spec `caching/Tantivy Result Cache`）。
//!
//! Key = `(index_object_key, field, term)`，Value = `count: u64`。`TantivyPruner::prune`
//! 命中时直接复用 count，跳过 `IndexHandle::count_term`。底层用 `moka` 异步 LRU + TTL。
//!
//! `capacity = 0` 整层关闭：`get` 永远返 `None`，`insert` 是 no-op，调用方不需要写
//! 任何分支。指标按 spec 命名暴露 `cache_tantivy_result_*` family。

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use moka::future::Cache;
use prometheus::{Gauge, IntCounter, Opts};

use crate::{
    config::TantivyResultCacheSettings,
    shared::metrics::{global_registry, register_int_counter},
};

/// Cache key 元组：必须便宜 Clone（被 moka 持久持有）。
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TantivyResultKey {
    pub index_object_key: String,
    pub field: String,
    pub term: String,
}

impl TantivyResultKey {
    pub fn new(
        index_object_key: impl Into<String>,
        field: impl Into<String>,
        term: impl Into<String>,
    ) -> Self {
        Self {
            index_object_key: index_object_key.into(),
            field: field.into(),
            term: term.into(),
        }
    }
}

pub struct TantivyResultCache {
    inner: Option<Cache<TantivyResultKey, u64>>,
}

impl TantivyResultCache {
    pub fn new(settings: &TantivyResultCacheSettings) -> Self {
        let _ = metrics();
        if settings.capacity == 0 {
            return Self { inner: None };
        }
        let evict = &metrics().evictions;
        let evict = evict.clone();
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
        Self { inner: Some(cache) }
    }

    pub async fn get(&self, key: &TantivyResultKey) -> Option<u64> {
        let m = metrics();
        let cache = self.inner.as_ref()?;
        match cache.get(key).await {
            Some(v) => {
                m.hits.inc();
                m.hits_view.fetch_add(1, Ordering::Relaxed);
                refresh_ratio(m);
                Some(v)
            }
            None => {
                m.misses.inc();
                m.misses_view.fetch_add(1, Ordering::Relaxed);
                refresh_ratio(m);
                None
            }
        }
    }

    pub async fn insert(&self, key: TantivyResultKey, count: u64) {
        if let Some(cache) = self.inner.as_ref() {
            cache.insert(key, count).await;
        }
    }

    /// 单 archive 失效：合并 / retention 删除时调。N entries（每个 (field, term) 组合）
    /// 会被异步逐条 invalidate；调用方不需要枚举 predicates。
    pub async fn invalidate_index_object_keys(&self, index_object_keys: &[String]) {
        let Some(cache) = self.inner.as_ref() else {
            return;
        };
        if index_object_keys.is_empty() {
            return;
        }
        let prefixes: Vec<String> = index_object_keys.to_vec();
        cache
            .invalidate_entries_if(move |k, _v| prefixes.iter().any(|a| a == &k.index_object_key))
            .ok();
    }

    pub fn hit_ratio(&self) -> f64 {
        let (h, m) = snapshot();
        if h + m == 0 {
            0.0
        } else {
            h as f64 / (h + m) as f64
        }
    }

    pub fn record_error(&self) {
        metrics().errors.inc();
    }
}

/// Metric family（spec 命名固定）。OnceLock 保证幂等注册。
struct Metrics {
    hits: IntCounter,
    misses: IntCounter,
    evictions: IntCounter,
    errors: IntCounter,
    hit_ratio: Gauge,
    hits_view: AtomicU64,
    misses_view: AtomicU64,
}

fn metrics() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| {
        let hits = register_int_counter(
            "cache_tantivy_result_hits_total",
            "tantivy result cache hits",
        );
        let misses = register_int_counter(
            "cache_tantivy_result_misses_total",
            "tantivy result cache misses",
        );
        let evictions = register_int_counter(
            "cache_tantivy_result_evictions_total",
            "tantivy result cache LRU/TTL evictions",
        );
        let errors = register_int_counter(
            "cache_tantivy_result_errors_total",
            "tantivy result cache internal errors (falls through to direct query)",
        );
        let hit_ratio = {
            let g = Gauge::with_opts(Opts::new(
                "cache_tantivy_result_hit_ratio",
                "tantivy result cache hit ratio in [0.0, 1.0]",
            ))
            .expect("create gauge");
            match global_registry().register(Box::new(g.clone())) {
                Ok(()) | Err(prometheus::Error::AlreadyReg) => g,
                Err(e) => panic!("register gauge: {e}"),
            }
        };
        Metrics {
            hits,
            misses,
            evictions,
            errors,
            hit_ratio,
            hits_view: AtomicU64::new(0),
            misses_view: AtomicU64::new(0),
        }
    })
}

fn refresh_ratio(m: &Metrics) {
    let h = m.hits_view.load(Ordering::Relaxed) as f64;
    let miss = m.misses_view.load(Ordering::Relaxed) as f64;
    let total = h + miss;
    let ratio = if total == 0.0 { 0.0 } else { h / total };
    m.hit_ratio.set(ratio);
}

fn snapshot() -> (u64, u64) {
    let m = metrics();
    (
        m.hits_view.load(Ordering::Relaxed),
        m.misses_view.load(Ordering::Relaxed),
    )
}

/// `TantivyPruner` 持有的句柄类型别名，便于上下游统一签名。
pub type TantivyResultCacheRef = Arc<TantivyResultCache>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capacity_zero_acts_as_noop() {
        let c = TantivyResultCache::new(&TantivyResultCacheSettings {
            capacity: 0,
            ttl_secs: 60,
        });
        let key = TantivyResultKey::new("a", "f", "t");
        assert!(c.get(&key).await.is_none());
        c.insert(key.clone(), 7).await;
        assert!(c.get(&key).await.is_none(), "no-op cache must always miss");
    }

    #[tokio::test]
    async fn miss_then_insert_then_hit() {
        let c = TantivyResultCache::new(&TantivyResultCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        });
        let key = TantivyResultKey::new("arc-x", "message", "panic");
        assert!(c.get(&key).await.is_none());
        c.insert(key.clone(), 42).await;
        assert_eq!(c.get(&key).await, Some(42));
    }

    #[tokio::test]
    async fn invalidate_index_object_keys_drops_all_matching_entries() {
        let c = TantivyResultCache::new(&TantivyResultCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        });
        // 同 archive 多 predicate
        c.insert(TantivyResultKey::new("arc-a", "f1", "t1"), 1)
            .await;
        c.insert(TantivyResultKey::new("arc-a", "f2", "t2"), 2)
            .await;
        // 异 archive
        c.insert(TantivyResultKey::new("arc-b", "f1", "t1"), 3)
            .await;

        c.invalidate_index_object_keys(&["arc-a".to_string()]).await;
        // moka 的 invalidate_entries_if 是 best-effort 异步：sleep 一点让它跑。
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // 触发 housekeeping
        if let Some(inner) = c.inner.as_ref() {
            inner.run_pending_tasks().await;
        }

        assert!(
            c.get(&TantivyResultKey::new("arc-a", "f1", "t1"))
                .await
                .is_none()
        );
        assert!(
            c.get(&TantivyResultKey::new("arc-a", "f2", "t2"))
                .await
                .is_none()
        );
        assert_eq!(
            c.get(&TantivyResultKey::new("arc-b", "f1", "t1")).await,
            Some(3)
        );
    }
}
