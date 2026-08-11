// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 归档 footer cache（spec `caching/Tantivy Footer Cache`）。
//!
//! Key = `index_object_key`、Value = `Arc<TantivyFooter>`（archive bytes + 解析后的 schema）。
//! IndexHandle cache TTL 过期后，重新打开归档时优先查本 cache：命中即用 bytes 重建
//! handle，避免对象存储 GET；miss 才走完整下载 + 解析。
//!
//! `capacity = 0` 整层关闭：`get` 永远 `None`、`insert` 是 no-op。

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use moka::future::Cache;
use prometheus::{Gauge, IntCounter, Opts};

// `TantivyFooter` 经 `search::tantivy_index` 转出口暴露；底层类型来自
// `molesignal_tantivy`，change `tantivy-puffin-migration` 后已经从「整 archive bytes」
// 改成「puffin footer payload + meta + schema」（约几 KB），cache value 轻量化。
use crate::infra::search::tantivy_index::TantivyFooter;
use crate::{
    config::TantivyFooterCacheSettings,
    shared::metrics::{global_registry, register_int_counter},
};

pub struct TantivyFooterCache {
    inner: Option<Cache<String, Arc<TantivyFooter>>>,
}

impl TantivyFooterCache {
    pub fn new(settings: &TantivyFooterCacheSettings) -> Self {
        let _ = metrics();
        if settings.capacity == 0 {
            return Self { inner: None };
        }
        let evict = metrics().evictions.clone();
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

    pub async fn get(&self, index_object_key: &str) -> Option<Arc<TantivyFooter>> {
        let m = metrics();
        let cache = self.inner.as_ref()?;
        match cache.get(index_object_key).await {
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

    pub async fn insert(&self, index_object_key: String, footer: Arc<TantivyFooter>) {
        if let Some(cache) = self.inner.as_ref() {
            cache.insert(index_object_key, footer).await;
        }
    }

    pub async fn invalidate_index_object_keys(&self, index_object_keys: &[String]) {
        let Some(cache) = self.inner.as_ref() else {
            return;
        };
        for k in index_object_keys {
            cache.invalidate(k).await;
        }
    }

    pub fn hit_ratio(&self) -> f64 {
        let m = metrics();
        let h = m.hits_view.load(Ordering::Relaxed);
        let mi = m.misses_view.load(Ordering::Relaxed);
        if h + mi == 0 {
            0.0
        } else {
            h as f64 / (h + mi) as f64
        }
    }

    pub fn record_error(&self) {
        metrics().errors.inc();
    }
}

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
            "cache_tantivy_footer_hits_total",
            "tantivy footer cache hits",
        );
        let misses = register_int_counter(
            "cache_tantivy_footer_misses_total",
            "tantivy footer cache misses",
        );
        let evictions = register_int_counter(
            "cache_tantivy_footer_evictions_total",
            "tantivy footer cache LRU/TTL evictions",
        );
        let errors = register_int_counter(
            "cache_tantivy_footer_errors_total",
            "tantivy footer cache internal errors (falls through to full parse)",
        );
        let hit_ratio = {
            let g = Gauge::with_opts(Opts::new(
                "cache_tantivy_footer_hit_ratio",
                "tantivy footer cache hit ratio in [0.0, 1.0]",
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
    let mi = m.misses_view.load(Ordering::Relaxed) as f64;
    let total = h + mi;
    let ratio = if total == 0.0 { 0.0 } else { h / total };
    m.hit_ratio.set(ratio);
}

pub type TantivyFooterCacheRef = Arc<TantivyFooterCache>;

#[cfg(test)]
mod tests {
    use tantivy::schema::{Schema as TSchema, TEXT};

    use super::*;

    fn dummy_footer() -> Arc<TantivyFooter> {
        let mut sb = TSchema::builder();
        sb.add_text_field("f", TEXT);
        let puffin_meta = crate::tantivy::PuffinMeta {
            blobs: Vec::new(),
            properties: Default::default(),
        };
        Arc::new(TantivyFooter {
            puffin_meta: Arc::new(puffin_meta),
            footer_payload_bytes: bytes::Bytes::from_static(b"placeholder"),
            schema: sb.build(),
            atomic_files: Arc::new(Default::default()),
            object_size: 0,
        })
    }

    #[tokio::test]
    async fn capacity_zero_acts_as_noop() {
        let c = TantivyFooterCache::new(&TantivyFooterCacheSettings {
            capacity: 0,
            ttl_secs: 60,
        });
        assert!(c.get("k").await.is_none());
        c.insert("k".into(), dummy_footer()).await;
        assert!(c.get("k").await.is_none());
    }

    #[tokio::test]
    async fn miss_then_insert_then_hit() {
        let c = TantivyFooterCache::new(&TantivyFooterCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        });
        assert!(c.get("k1").await.is_none());
        c.insert("k1".into(), dummy_footer()).await;
        assert!(c.get("k1").await.is_some());
    }

    #[tokio::test]
    async fn invalidate_index_object_keys_drops_specific_entries() {
        let c = TantivyFooterCache::new(&TantivyFooterCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        });
        c.insert("a".into(), dummy_footer()).await;
        c.insert("b".into(), dummy_footer()).await;
        c.invalidate_index_object_keys(&["a".to_string()]).await;
        assert!(c.get("a").await.is_none());
        assert!(c.get("b").await.is_some());
    }
}
