// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `QueryResultCache`：`blake3(stmt + org + time_range + role)` → `Arc<QueryResult>`。
//! `time_range.end > now - 5min` 的开窗查询直通不缓存。

use std::{sync::Arc, time::Duration};

use moka::future::Cache;

use super::metrics::CacheMetrics;
use crate::{
    config::CacheLayerSettings,
    domain::query::{QueryLanguage, QueryRequest, QueryResult},
};

/// `QueryResultCache` 直通窗口：`time_range.end > now - 5min` 时不缓存。
pub const QUERY_FRESH_WINDOW_MICROS: i64 = 5 * 60 * 1_000_000;

pub struct QueryResultCache {
    inner: Cache<String, Arc<QueryResult>>,
    metrics: CacheMetrics,
}

impl QueryResultCache {
    pub fn new(settings: CacheLayerSettings) -> Self {
        let metrics = CacheMetrics::register("query_result");
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

    /// 返回 `(result, cache_hit)`。`cache_hit = false` 同时覆盖"直通"与"miss 后写入"两种语义。
    pub async fn get_or_insert<F, Fut>(
        &self,
        req: &QueryRequest,
        role_filter: &str,
        now_micros: i64,
        run: F,
    ) -> anyhow::Result<(QueryResult, bool)>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<QueryResult>>,
    {
        if req.time_range.end.0 > now_micros - QUERY_FRESH_WINDOW_MICROS {
            // open window → 不缓存，直通跑
            let res = run().await?;
            return Ok((res, false));
        }
        let key = Self::key_for(req, role_filter);
        if let Some(v) = self.inner.get(&key).await {
            self.metrics.record_hit();
            return Ok(((*v).clone(), true));
        }
        self.metrics.record_miss();
        let res = run().await?;
        self.inner.insert(key, Arc::new(res.clone())).await;
        Ok((res, false))
    }

    /// 缓存键必须覆盖每一个会改变结果的输入。除 org / 语言 / 语句 / 时间窗 / 角色过滤外
    /// 还包含三项，少任何一项都会串结果：
    ///
    /// - `limit`：不只是截断行数。PromQL 侧 `limit <= 1` 走 instant vector、`> 1` 走 range
    ///   matrix，`range_step_us` 也用它算步长 —— 同一条语句不同 limit 的结果形态不同。
    /// - `stream`：同名语句可以指向不同的 stream hint。
    /// - `federation_clusters`：联邦查询与纯本地查询的结果集不同，共享键会让联邦结果
    ///   命中本地缓存。
    fn key_for(req: &QueryRequest, role_filter: &str) -> String {
        let lang = match req.language {
            QueryLanguage::Sql => "sql",
            QueryLanguage::Promql => "promql",
        };
        let stream = req
            .stream
            .as_ref()
            .map(|s| format!("{}:{:?}", s.name, s.stream_type))
            .unwrap_or_default();
        // 排序后再拼：集群列表的顺序不影响结果，不该分裂缓存键。
        let mut clusters: Vec<&str> = req.federation_clusters.iter().map(|c| c.as_str()).collect();
        clusters.sort_unstable();
        let payload = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            req.org_id,
            lang,
            req.statement,
            req.time_range.start.0,
            req.time_range.end.0,
            role_filter,
            req.limit.map_or_else(|| "-".to_string(), |l| l.to_string()),
            stream,
            clusters.join(","),
        );
        blake3::hash(payload.as_bytes()).to_hex().to_string()
    }

    /// 时间窗是否还在新鲜窗口内 —— 是则不参与缓存（读写都跳过）。
    fn window_open(req: &QueryRequest, now_micros: i64) -> bool {
        req.time_range.end.0 > now_micros - QUERY_FRESH_WINDOW_MICROS
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

#[async_trait::async_trait]
impl crate::domain::query::QueryResultCachePort for QueryResultCache {
    async fn get(
        &self,
        req: &QueryRequest,
        role_filter: &str,
        now_micros: i64,
    ) -> Option<QueryResult> {
        if Self::window_open(req, now_micros) {
            return None;
        }
        let key = Self::key_for(req, role_filter);
        match self.inner.get(&key).await {
            Some(v) => {
                self.metrics.record_hit();
                Some((*v).clone())
            }
            None => {
                self.metrics.record_miss();
                None
            }
        }
    }

    async fn put(
        &self,
        req: &QueryRequest,
        role_filter: &str,
        now_micros: i64,
        result: QueryResult,
    ) {
        if Self::window_open(req, now_micros) {
            return;
        }
        self.inner
            .insert(Self::key_for(req, role_filter), Arc::new(result))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    /// 键必须区分每一个会改变结果的输入。少任何一项 = 两条本应不同的查询共享一份结果。
    #[test]
    fn key_covers_limit_stream_and_federation() {
        use crate::domain::{query::StreamHint, stream::StreamType};

        let base = QueryRequest {
            org_id: Id::from_string("org-x"),
            language: QueryLanguage::Sql,
            statement: "SELECT 1".into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            stream: None,
            limit: None,
            federation_clusters: Vec::new(),
        };
        let k0 = QueryResultCache::key_for(&base, "role-x");

        // limit：PromQL 侧 <=1 走 instant vector、>1 走 range matrix，结果形态不同。
        let with_limit = QueryRequest {
            limit: Some(1),
            ..base.clone()
        };
        let with_bigger_limit = QueryRequest {
            limit: Some(100),
            ..base.clone()
        };
        assert_ne!(k0, QueryResultCache::key_for(&with_limit, "role-x"));
        assert_ne!(
            QueryResultCache::key_for(&with_limit, "role-x"),
            QueryResultCache::key_for(&with_bigger_limit, "role-x"),
            "limit=1 与 limit=100 的结果形态不同，不能共享缓存"
        );

        // stream hint
        let with_stream = QueryRequest {
            stream: Some(StreamHint {
                name: "app".into(),
                stream_type: StreamType::Logs,
            }),
            ..base.clone()
        };
        assert_ne!(k0, QueryResultCache::key_for(&with_stream, "role-x"));

        // 联邦集群：联邦结果不能命中本地查询的缓存。
        let federated = QueryRequest {
            federation_clusters: vec!["eu".into()],
            ..base.clone()
        };
        assert_ne!(k0, QueryResultCache::key_for(&federated, "role-x"));

        // 集群列表顺序不影响结果，不该分裂缓存键。
        let ab = QueryRequest {
            federation_clusters: vec!["a".into(), "b".into()],
            ..base.clone()
        };
        let ba = QueryRequest {
            federation_clusters: vec!["b".into(), "a".into()],
            ..base.clone()
        };
        assert_eq!(
            QueryResultCache::key_for(&ab, "role-x"),
            QueryResultCache::key_for(&ba, "role-x"),
        );

        // org 与 role 仍然参与（回归保护）。
        let other_org = QueryRequest {
            org_id: Id::from_string("org-y"),
            ..base.clone()
        };
        assert_ne!(k0, QueryResultCache::key_for(&other_org, "role-x"));
        assert_ne!(k0, QueryResultCache::key_for(&base, "role-y"));
    }

    #[tokio::test]
    async fn query_result_cache_skips_open_window() {
        let c = QueryResultCache::new(CacheLayerSettings::new(100, 60));
        let now = 1_000_000_000i64;
        let req = QueryRequest {
            org_id: Id::from_string("org-x"),
            language: QueryLanguage::Sql,
            statement: "SELECT 1".into(),
            time_range: TimeRange::new(
                TimestampMicros(now - 1_000_000_000),
                TimestampMicros(now - 1_000_000), // end in the recent 5min window
            ),
            stream: None,
            limit: None,
            federation_clusters: Vec::new(),
        };
        let call_count = Arc::new(AtomicUsize::new(0));
        let mk_run = |c: Arc<AtomicUsize>| {
            move || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(QueryResult {
                        columns: vec!["x".into()],
                        rows: vec![vec![serde_json::json!(1)]],
                        scanned_rows: 0,
                        took_ms: 0,
                        federation: None,
                    })
                }
            }
        };
        // 两次都直通跑（不缓存），cache_hit 都是 false
        let (_, h1) = c
            .get_or_insert(&req, "role-x", now, mk_run(call_count.clone()))
            .await
            .unwrap();
        let (_, h2) = c
            .get_or_insert(&req, "role-x", now, mk_run(call_count.clone()))
            .await
            .unwrap();
        assert!(!h1 && !h2);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn query_result_cache_hits_on_closed_window() {
        let c = QueryResultCache::new(CacheLayerSettings::new(100, 60));
        let now = 10_000_000_000i64;
        let req = QueryRequest {
            org_id: Id::from_string("org-x"),
            language: QueryLanguage::Sql,
            statement: "SELECT 2".into(),
            time_range: TimeRange::new(
                TimestampMicros(now - 100_000_000_000),
                TimestampMicros(now - 10 * QUERY_FRESH_WINDOW_MICROS), // safely closed
            ),
            stream: None,
            limit: None,
            federation_clusters: Vec::new(),
        };
        let call_count = Arc::new(AtomicUsize::new(0));
        let mk_run = |c: Arc<AtomicUsize>| {
            move || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(QueryResult {
                        columns: vec!["x".into()],
                        rows: vec![vec![serde_json::json!(1)]],
                        scanned_rows: 0,
                        took_ms: 0,
                        federation: None,
                    })
                }
            }
        };
        let (_, h1) = c
            .get_or_insert(&req, "role-x", now, mk_run(call_count.clone()))
            .await
            .unwrap();
        let (_, h2) = c
            .get_or_insert(&req, "role-x", now, mk_run(call_count.clone()))
            .await
            .unwrap();
        assert!(!h1, "first must be miss");
        assert!(h2, "second must be cache hit");
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "loader runs once");
    }
}
