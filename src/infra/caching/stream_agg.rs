// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `StreamingAggCache`：range PromQL 的按窗口分桶增量缓存（design 扩展）。
//!
//! 与整结果级的 [`super::QueryResultCache`] 不同：实时仪表盘每次刷新 `end` 前移 →
//! 整结果 key 必变 → 100% miss。本缓存粒度细到 **(查询指纹, 已封存窗口桶)**：
//! 缓存键 = `blake3(org + 函数 + 选择器 + step + window)`（指纹），值 = 该指纹下「已封存
//! 桶集合」[`SealedSeries`]。时间窗前移刷新时，与上次重叠的稳定桶直接命中，仅活跃区重算。
//!
//! 「已封存」= 窗口右端早于水位（`min(now - safe_lookback, ingest 水位)`）。封存桶值不再
//! 变化，可跨刷新复用；近段（活跃）桶每次重算、不入缓存，以天然规避多数晚到数据。
//!
//! 容量上限按「指纹条目数」计（moka `max_capacity`）；单条目内再以 `max_series_per_query`
//! 限制 series 数、`max_buckets_per_series` 滑窗 prune 老桶，双重约束封顶内存。

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
    time::Duration,
};

use moka::future::Cache;

use super::metrics::CacheMetrics;
use crate::config::StreamAggCacheSettings;

/// 缓存里的 label 集表示，与 promql 引擎的 `LabelSet` 同构（稳定排序、hash 友好）。
pub type CachedLabels = BTreeMap<String, String>;

/// 单条 series 滑窗保留的桶数上限：超出按 grid 从老端 prune。
/// 取 range 路径每 series 输出点数上限（1000）的数倍，给滑动窗口留足复用余量。
const MAX_BUCKETS_PER_SERIES: usize = 4_096;

/// 某查询指纹下「已连续封存」的窗口桶集合。
///
/// 不变量：对 `[stable_from, stable_through]`（grid ts，闭区间）内的每个桶，
/// **所有在该桶有数据的 series 都已算入并缓存**。因此服务这段区间时遍历全部 series 即可，
/// 停止上报的 series 不会被漏掉、新出现的 series 在其封存时自然补入。
#[derive(Debug)]
pub struct SealedSeries {
    stable_from: i64,
    stable_through: i64,
    series: HashMap<CachedLabels, BTreeMap<i64, f64>>,
}

impl Default for SealedSeries {
    fn default() -> Self {
        Self {
            stable_from: i64::MAX,
            stable_through: i64::MIN,
            series: HashMap::new(),
        }
    }
}

impl SealedSeries {
    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    /// 已封存区间下界（grid ts，含）；空时为 `i64::MAX`。
    pub fn stable_from(&self) -> i64 {
        self.stable_from
    }

    /// 已封存区间上界（grid ts，含）；空时为 `i64::MIN`。
    pub fn stable_through(&self) -> i64 {
        self.stable_through
    }

    /// 把一批新算出的封存桶并入。`new_buckets` 的每项是 `(labels, [(bucket_ts, value)])`，
    /// 且这些桶都 `<= watermark`（由调用方保证）。`sealed_from..=sealed_through` 是本次封存
    /// 覆盖的 grid 桶区间。`step_us` + `max_buckets`（每 series 桶数上限，0 = 不限）决定老桶
    /// prune（统一下界，保封存区间不变量）；`max_series`（0 = 不限）限制单指纹缓存的 series 数。
    pub fn seal(
        &mut self,
        new_buckets: Vec<(CachedLabels, Vec<(i64, f64)>)>,
        sealed_from: i64,
        sealed_through: i64,
        step_us: i64,
        max_buckets: usize,
        max_series: usize,
    ) {
        if sealed_from > sealed_through {
            return; // 没有可封存的桶（活跃区为空 / 水位早于查询）
        }
        if self.series.is_empty() {
            self.stable_from = sealed_from;
            self.stable_through = sealed_through;
        } else {
            self.stable_from = self.stable_from.min(sealed_from);
            self.stable_through = self.stable_through.max(sealed_through);
        }

        for (labels, buckets) in new_buckets {
            if buckets.is_empty() {
                continue;
            }
            if let Some(existing) = self.series.get_mut(&labels) {
                existing.extend(buckets);
            } else if max_series == 0 || self.series.len() < max_series {
                self.series.insert(labels, buckets.into_iter().collect());
            }
            // 超 max_series 的新 series：本次结果已含其计算点（调用方已 emit），仅不进缓存。
        }

        // 统一按 grid 从老端 prune，封顶单 series 桶数；统一下界保证封存区间不变量不破。
        if step_us > 0 && max_buckets > 0 {
            let span = (max_buckets as i64).saturating_mul(step_us);
            let retain_from = self.stable_through.saturating_sub(span);
            if retain_from > self.stable_from {
                self.series.retain(|_, m| {
                    *m = m.split_off(&retain_from);
                    !m.is_empty()
                });
                self.stable_from = retain_from;
            }
        }
    }

    /// 读出 `[lo, hi]`（grid ts，闭区间）内所有 series 的缓存桶点，逐点回调
    /// `(labels, ts, value)`；返回回调次数（命中点数）。
    pub fn serve(&self, lo: i64, hi: i64, mut emit: impl FnMut(&CachedLabels, i64, f64)) -> u64 {
        if lo > hi {
            return 0;
        }
        let mut n = 0u64;
        for (labels, buckets) in &self.series {
            for (&ts, &v) in buckets.range(lo..=hi) {
                emit(labels, ts, v);
                n += 1;
            }
        }
        n
    }
}

/// range PromQL 窗口聚合增量缓存。进程内、单节点（与 `QueryResultCache` 一致）。
pub struct StreamingAggCache {
    inner: Cache<String, Arc<RwLock<SealedSeries>>>,
    metrics: CacheMetrics,
    max_series_per_query: usize,
}

impl StreamingAggCache {
    pub fn new(settings: &StreamAggCacheSettings) -> Self {
        let metrics = CacheMetrics::register("streaming_agg");
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
        Self {
            inner,
            metrics,
            max_series_per_query: settings.max_series_per_query,
        }
    }

    pub async fn get(&self, fingerprint: &str) -> Option<Arc<RwLock<SealedSeries>>> {
        self.inner.get(fingerprint).await
    }

    pub async fn insert(&self, fingerprint: String, entry: Arc<RwLock<SealedSeries>>) {
        self.inner.insert(fingerprint, entry).await;
    }

    pub fn max_series_per_query(&self) -> usize {
        self.max_series_per_query
    }

    /// 单 series 滑窗保留桶数上限，传给 [`SealedSeries::seal`] 控制 prune。
    pub fn max_buckets_per_series(&self) -> usize {
        MAX_BUCKETS_PER_SERIES
    }

    pub fn record_hits(&self, n: u64) {
        self.metrics.record_hits(n);
    }

    pub fn record_misses(&self, n: u64) {
        self.metrics.record_misses(n);
    }

    /// 命中率（含本缓存实例累计的 hit/miss），观测 + 测试用。
    pub fn hit_ratio(&self) -> f64 {
        let (h, m) = self.metrics.snapshot();
        if h + m == 0 {
            0.0
        } else {
            h as f64 / (h + m) as f64
        }
    }

    /// 累计 `(hits, misses)`，测试断言用。
    pub fn stats(&self) -> (u64, u64) {
        self.metrics.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> CachedLabels {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn seal_then_serve_returns_cached_buckets() {
        let mut s = SealedSeries::default();
        assert!(s.is_empty());
        let a = labels(&[("method", "GET")]);
        s.seal(
            vec![(a.clone(), vec![(100, 1.0), (200, 2.0), (300, 3.0)])],
            100,
            300,
            100,
            0,
            0,
        );
        assert_eq!(s.stable_from(), 100);
        assert_eq!(s.stable_through(), 300);

        let mut got: Vec<(i64, f64)> = Vec::new();
        let n = s.serve(100, 300, |l, ts, v| {
            assert_eq!(l, &a);
            got.push((ts, v));
        });
        assert_eq!(n, 3);
        assert_eq!(got, vec![(100, 1.0), (200, 2.0), (300, 3.0)]);
    }

    #[test]
    fn serve_clips_to_requested_range() {
        let mut s = SealedSeries::default();
        s.seal(
            vec![(
                labels(&[("m", "a")]),
                vec![(100, 1.0), (200, 2.0), (300, 3.0)],
            )],
            100,
            300,
            100,
            0,
            0,
        );
        let mut got = Vec::new();
        s.serve(150, 250, |_, ts, _| got.push(ts));
        assert_eq!(got, vec![200], "only the bucket inside [150,250]");
    }

    #[test]
    fn seal_merges_new_buckets_and_advances_through() {
        let mut s = SealedSeries::default();
        let a = labels(&[("m", "a")]);
        s.seal(
            vec![(a.clone(), vec![(100, 1.0), (200, 2.0)])],
            100,
            200,
            100,
            0,
            0,
        );
        // 下一次刷新封存了新桶 300（活跃→稳定），并复算了 200（幂等）。
        s.seal(
            vec![(a.clone(), vec![(200, 2.0), (300, 3.0)])],
            200,
            300,
            100,
            0,
            0,
        );
        assert_eq!(s.stable_from(), 100);
        assert_eq!(s.stable_through(), 300);
        let mut got = Vec::new();
        s.serve(100, 300, |_, ts, v| got.push((ts, v)));
        got.sort_by_key(|&(t, _)| t);
        assert_eq!(got, vec![(100, 1.0), (200, 2.0), (300, 3.0)]);
    }

    #[test]
    fn seal_keeps_disappeared_series() {
        // series b 在第二次封存里没数据；它的旧桶仍应保留、可服务。
        let mut s = SealedSeries::default();
        let a = labels(&[("m", "a")]);
        let b = labels(&[("m", "b")]);
        s.seal(
            vec![(a.clone(), vec![(100, 1.0)]), (b.clone(), vec![(100, 9.0)])],
            100,
            100,
            100,
            0,
            0,
        );
        s.seal(vec![(a.clone(), vec![(200, 2.0)])], 200, 200, 100, 0, 0);
        let mut by_label: std::collections::HashMap<String, Vec<i64>> = Default::default();
        s.serve(100, 200, |l, ts, _| {
            by_label
                .entry(l.get("m").unwrap().clone())
                .or_default()
                .push(ts);
        });
        assert_eq!(by_label.get("a").unwrap(), &vec![100, 200]);
        assert_eq!(
            by_label.get("b").unwrap(),
            &vec![100],
            "disappeared series retained"
        );
    }

    #[test]
    fn seal_respects_max_series_cap() {
        let mut s = SealedSeries::default();
        s.seal(
            vec![
                (labels(&[("m", "a")]), vec![(100, 1.0)]),
                (labels(&[("m", "b")]), vec![(100, 2.0)]),
                (labels(&[("m", "c")]), vec![(100, 3.0)]),
            ],
            100,
            100,
            100,
            0,
            2, // cap = 2 series
        );
        let mut count = 0u64;
        s.serve(100, 100, |_, _, _| count += 1);
        assert_eq!(count, 2, "third series skipped by cap");
    }

    #[test]
    fn seal_prunes_old_buckets_beyond_cap() {
        // max_buckets = 3, step = 100：封存到 600 后保留下界 = 600 - 3*100 = 300，
        // 桶 100/200 应被 prune，stable_from 抬到 300。
        let mut s = SealedSeries::default();
        let a = labels(&[("m", "a")]);
        s.seal(
            vec![(
                a,
                vec![
                    (100, 1.0),
                    (200, 2.0),
                    (300, 3.0),
                    (400, 4.0),
                    (500, 5.0),
                    (600, 6.0),
                ],
            )],
            100,
            600,
            100,
            3, // max_buckets
            0,
        );
        assert_eq!(s.stable_from(), 300, "prune floor raised stable_from");
        assert_eq!(s.stable_through(), 600);
        let mut got = Vec::new();
        s.serve(0, 1000, |_, ts, _| got.push(ts));
        got.sort();
        assert_eq!(got, vec![300, 400, 500, 600], "old buckets pruned");
    }

    #[test]
    fn empty_seal_is_noop() {
        let mut s = SealedSeries::default();
        // sealed_from > sealed_through（活跃区为空）→ 不改任何状态。
        s.seal(
            vec![(labels(&[("m", "a")]), vec![(100, 1.0)])],
            200,
            100,
            100,
            0,
            0,
        );
        assert!(s.is_empty());
        assert_eq!(s.stable_through(), i64::MIN);
    }
}
