// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `ParquetMetaCache`：`object_key` → 调用方自定义 `V`（实际是 `Arc<ParquetMetaData>`
//! 或 Tantivy `IndexHandle`）。`try_get_with` 保证并发雷击下 `load` 只触发一次。

use std::{sync::Arc, time::Duration};

use moka::future::Cache;

use super::metrics::CacheMetrics;
use crate::config::CacheLayerSettings;

pub struct ParquetMetaCache<V>
where
    V: Send + Sync + Clone + 'static,
{
    inner: Cache<String, V>,
    metrics: CacheMetrics,
}

impl<V> ParquetMetaCache<V>
where
    V: Send + Sync + Clone + 'static,
{
    pub fn new(settings: CacheLayerSettings) -> Self {
        let metrics = CacheMetrics::register("parquet_meta");
        let evict = metrics.evictions();
        let inner = Cache::builder()
            .max_capacity(settings.capacity)
            .time_to_live(Duration::from_secs(settings.ttl_secs))
            .async_eviction_listener(move |_k, _v, _cause| {
                let evict = evict.clone();
                Box::pin(async move {
                    evict.inc();
                })
            })
            .build();
        Self { inner, metrics }
    }

    /// 并发雷击安全：N 个 caller 同 key 仅触发一次 `load`。
    /// hit/miss 精确：未命中时 `record_miss` 并跑 `try_get_with` 兜底单跑。
    pub async fn get_or_load<F, Fut>(&self, key: String, load: F) -> anyhow::Result<V>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<V>> + Send,
    {
        if let Some(v) = self.inner.get(&key).await {
            self.metrics.record_hit();
            return Ok(v);
        }
        self.metrics.record_miss();
        self.inner
            .try_get_with(key, async move { load().await })
            .await
            .map_err(|e: Arc<anyhow::Error>| anyhow::anyhow!("parquet meta cache load: {e}"))
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn parquet_meta_cache_try_get_with_runs_loader_once() {
        let cache: Arc<ParquetMetaCache<Arc<Vec<u8>>>> =
            Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
        let counter = Arc::new(AtomicUsize::new(0));
        let key = "obj-1".to_string();

        let mut handles = Vec::new();
        for _ in 0..16 {
            let key_clone = key.clone();
            let counter = counter.clone();
            let cache = cache.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_load(key_clone, move || {
                        let counter = counter.clone();
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            Ok::<_, anyhow::Error>(Arc::new(vec![1u8, 2, 3]))
                        }
                    })
                    .await
            }));
        }
        let results = futures::future::join_all(handles).await;
        for r in results {
            let v = r.unwrap().unwrap();
            assert_eq!(*v, vec![1, 2, 3]);
        }
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "loader must run exactly once"
        );
    }
}
