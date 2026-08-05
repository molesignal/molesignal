// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 缓存层 metric family：每级 cache 拥有一组独立 metric，按 `level` 标签区分。
//!
//! - `cache_<level>_hits_total` / `cache_<level>_misses_total` /
//!   `cache_<level>_evictions_total` / `cache_<level>_hit_ratio`
//!
//! `CacheMetrics::register(level)` 是幂等的（底层 prometheus registry 已容忍 AlreadyReg），
//! 同名 level 多次注册返回同一 metric 句柄。

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use prometheus::{IntCounter, IntCounterVec};

use crate::shared::metrics::register_int_counter_vec;

/// 三个 cache level 共享的 vec：用 `level` label 区分。
fn hits_vec() -> IntCounterVec {
    register_int_counter_vec("cache_hits_total", "cache hit count", &["level"])
}
fn misses_vec() -> IntCounterVec {
    register_int_counter_vec("cache_misses_total", "cache miss count", &["level"])
}
fn evictions_vec() -> IntCounterVec {
    register_int_counter_vec("cache_evictions_total", "cache eviction count", &["level"])
}

/// 命中率 gauge 在 `gather_text()` 之前由 caller 主动刷新。
fn hit_ratio_vec() -> prometheus::GaugeVec {
    use prometheus::{GaugeVec, Opts};
    let gauge = GaugeVec::new(
        Opts::new("cache_hit_ratio", "cache hit ratio in [0.0, 1.0]"),
        &["level"],
    )
    .expect("create gauge vec");
    match crate::shared::metrics::global_registry().register(Box::new(gauge.clone())) {
        Ok(()) | Err(prometheus::Error::AlreadyReg) => gauge,
        Err(e) => panic!("register gauge: {e}"),
    }
}

#[derive(Clone)]
pub(crate) struct CacheMetrics {
    level: &'static str,
    pub(crate) hits: IntCounter,
    pub(crate) misses: IntCounter,
    pub(crate) evictions: IntCounter,
    pub(crate) hit_ratio: prometheus::Gauge,
    /// 用 atomic 加和方便测试，并刷新 hit_ratio。
    hits_view: Arc<AtomicU64>,
    misses_view: Arc<AtomicU64>,
}

impl CacheMetrics {
    pub(crate) fn register(level: &'static str) -> Self {
        let hits = hits_vec().with_label_values(&[level]);
        let misses = misses_vec().with_label_values(&[level]);
        let evictions = evictions_vec().with_label_values(&[level]);
        let hit_ratio = hit_ratio_vec().with_label_values(&[level]);
        Self {
            level,
            hits,
            misses,
            evictions,
            hit_ratio,
            hits_view: Arc::new(AtomicU64::new(0)),
            misses_view: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn record_hit(&self) {
        self.hits.inc();
        self.hits_view.fetch_add(1, Ordering::Relaxed);
        self.refresh_ratio();
    }

    pub(crate) fn record_miss(&self) {
        self.misses.inc();
        self.misses_view.fetch_add(1, Ordering::Relaxed);
        self.refresh_ratio();
    }

    /// 批量记 n 次命中（一次 atomic + 一次 ratio 刷新），用于一次查询里逐 (series, 桶)
    /// 命中计数的热路径，避免逐点调用 [`record_hit`] 重复刷新 ratio。
    pub(crate) fn record_hits(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.hits.inc_by(n);
        self.hits_view.fetch_add(n, Ordering::Relaxed);
        self.refresh_ratio();
    }

    /// 批量记 n 次 miss，见 [`record_hits`]。
    pub(crate) fn record_misses(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.misses.inc_by(n);
        self.misses_view.fetch_add(n, Ordering::Relaxed);
        self.refresh_ratio();
    }

    pub(crate) fn evictions(&self) -> IntCounter {
        self.evictions.clone()
    }

    fn refresh_ratio(&self) {
        let h = self.hits_view.load(Ordering::Relaxed) as f64;
        let m = self.misses_view.load(Ordering::Relaxed) as f64;
        let total = h + m;
        let ratio = if total == 0.0 { 0.0 } else { h / total };
        self.hit_ratio.set(ratio);
    }

    pub(crate) fn snapshot(&self) -> (u64, u64) {
        (
            self.hits_view.load(Ordering::Relaxed),
            self.misses_view.load(Ordering::Relaxed),
        )
    }

    #[allow(dead_code)]
    pub(crate) fn level(&self) -> &'static str {
        self.level
    }
}
