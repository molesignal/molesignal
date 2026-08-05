// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询与检索：`[querier]`、`[search]`（含 admission / streaming_agg_cache）、`[search_jobs]`。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerierSettings {
    #[serde(default)]
    pub concurrency: u32,
    #[serde(default = "default_max_scan")]
    pub max_scan_rows: u64,
    /// planner 估算 rows 超过该阈值 → 强制转 async search-job。
    /// 0 表示禁用自动转 async。当前用「时间窗 + 单位窗口估算 rows」估计：
    /// `estimated_rows ≈ window_secs * default_throughput_per_sec`，
    /// 真实 DataFusion planner estimate 留 follow-up。
    #[serde(default = "default_auto_async_rows")]
    pub auto_async_threshold_rows: u64,
    /// 当前估算用：单位时间窗（秒）的平均行数；保守值。
    #[serde(default = "default_throughput")]
    pub estimate_throughput_per_sec: u64,
}

fn default_max_scan() -> u64 {
    100_000_000
}
fn default_auto_async_rows() -> u64 {
    50_000_000
}
fn default_throughput() -> u64 {
    1_000
}

impl Default for QuerierSettings {
    fn default() -> Self {
        Self {
            concurrency: 0,
            max_scan_rows: default_max_scan(),
            auto_async_threshold_rows: default_auto_async_rows(),
            estimate_throughput_per_sec: default_throughput(),
        }
    }
}

/// Search 差异化设置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchSettings {
    #[serde(default)]
    pub admission: AdmissionSettings,
    /// 结果行安全上限（ResultRowGuard AnalyzerRule）：无 LIMIT 且非聚合的查询注入此上限；
    /// 0 = 关闭（默认）。
    #[serde(default)]
    pub max_result_rows: usize,
    /// range PromQL 的按窗口分桶增量缓存（实时仪表盘刷新只重算活跃窗口）。
    /// 默认 disabled → 零行为变化。
    #[serde(default)]
    pub stream_agg_cache: StreamAggCacheSettings,
}

/// range PromQL 窗口聚合增量缓存设置。
///
/// 对 `rate(metric[range])` / `*_over_time` 等 range 查询，把按 step 网格对齐的
/// 已封存窗口桶（窗口右端 < 水位）缓存起来；仪表盘刷新（时间窗前移）时稳定桶直接
/// 命中缓存、仅活跃区扫描重算。`enabled = false`（默认）时整条路径短路，行为与
/// 未接缓存完全一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAggCacheSettings {
    /// moka 条目上限：按「不同查询指纹（org+函数+选择器+step+window）」计数。
    /// **0 = 关闭**（不实例化缓存、range 路径保持现状）；默认 0（OSS 默认关）。
    #[serde(default = "default_streaming_agg_capacity")]
    pub capacity: u64,
    /// 条目 TTL（秒）：兜底超出 `safe_lookback` 的乱序晚到导致的陈旧值。
    #[serde(default = "default_streaming_agg_ttl_secs")]
    pub ttl_secs: u64,
    /// 安全回看窗口（秒）：仅缓存窗口右端早于 `now - safe_lookback` 的桶，把近段
    /// 全划为活跃（不缓存）以天然规避多数晚到数据。
    #[serde(default = "default_streaming_agg_safe_lookback_secs")]
    pub safe_lookback_secs: u64,
    /// 单个查询指纹下缓存的 series 数上限（防高基数标签把单条目撑爆）；0 = 不限。
    #[serde(default = "default_streaming_agg_max_series")]
    pub max_series_per_query: usize,
}

fn default_streaming_agg_capacity() -> u64 {
    0
}
fn default_streaming_agg_ttl_secs() -> u64 {
    300
}
fn default_streaming_agg_safe_lookback_secs() -> u64 {
    300
}
fn default_streaming_agg_max_series() -> usize {
    10_000
}

impl Default for StreamAggCacheSettings {
    fn default() -> Self {
        Self {
            capacity: default_streaming_agg_capacity(),
            ttl_secs: default_streaming_agg_ttl_secs(),
            safe_lookback_secs: default_streaming_agg_safe_lookback_secs(),
            max_series_per_query: default_streaming_agg_max_series(),
        }
    }
}

/// Search admission（并发槽位准入）设置。空 / 0 = 不限（OSS 默认零行为）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdmissionSettings {
    /// 未在 `groups` 显式配置的工作组的**节点**并发上限；0 = 不限。
    #[serde(default)]
    pub default_max_concurrent: usize,
    /// 工作组名 → 节点并发上限。
    #[serde(default)]
    pub groups: std::collections::HashMap<String, usize>,
    /// 角色名（owner/admin/editor/viewer）→ 工作组名；缺省落 `default`。
    #[serde(default)]
    pub role_map: std::collections::HashMap<String, String>,
    /// 未在 `cluster_groups` 配置的工作组的**集群**（全节点合计）软上限；0 = 不限。
    #[serde(default)]
    pub cluster_default_max_concurrent: usize,
    /// 工作组名 → 集群软上限。
    #[serde(default)]
    pub cluster_groups: std::collections::HashMap<String, usize>,
    /// 跨节点负载发布 / 集群视图刷新间隔（秒）；0 = 用默认 5s。
    #[serde(default)]
    pub cluster_sync_interval_secs: u64,
    /// 集群负载行新鲜度窗口（秒）；超此未更新的节点行不计入集群视图。0 = 默认 30s。
    #[serde(default)]
    pub cluster_stale_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchJobsSettings {
    #[serde(default = "default_search_workers")]
    pub workers: u32,
    #[serde(default = "default_search_idle")]
    pub idle_poll_secs: u64,
    #[serde(default = "default_search_cleanup")]
    pub cleanup_interval_secs: u64,
}
fn default_search_workers() -> u32 {
    2
}
fn default_search_idle() -> u64 {
    2
}
fn default_search_cleanup() -> u64 {
    3600
}
impl Default for SearchJobsSettings {
    fn default() -> Self {
        Self {
            workers: default_search_workers(),
            idle_poll_secs: default_search_idle(),
            cleanup_interval_secs: default_search_cleanup(),
        }
    }
}
