// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service graph aggregator + repository。
//!
//! 工作流：
//! 1. intake 在 flush traces stream 的 RecordBatch 之前，遍历每个 span，
//!    经 [`ServiceGraphAggregator::record`] 把 client→server 这条边的样本累计到
//!    `DashMap<EdgeKey, Bucket>`。
//! 2. 每分钟边界（`bucket_at = floor(now / 60s)` 变化时）由
//!    [`ServiceGraphAggregator::flush_due`] 把上一窗口的桶 drain，转 [`EdgeSnapshot`]
//!    通过 [`ServiceGraphRepository::insert_many`] 落 `service_graph_edges` 表。
//! 3. HTTP `GET /api/v1/traces/service_graph` 走 [`ServiceGraphRepository::query`] 返边集。
//!
//! 分位数采用每桶滑动样本（capped 1024 上限，避免长尾爆内存）+ 简化 nearest-rank。

use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    domain::intake::{RawEvent, ServiceGraphObserver},
    shared::{
        ids::Id, time::TimestampMicros,
        trace_normalization::effective_service_name as canonical_effective_service_name,
    },
};

pub mod domain {
    pub use ::domain::*;
}

mod repository;

pub use repository::{PgServiceGraphRepository, ServiceGraphRepository};

pub mod infra {
    pub mod persistence {
        pub use postgres::sqlx_err;
    }
}

pub mod shared {
    pub use kernel::{Result, ids, time};
    pub use signals::trace_normalization;
}

const BUCKET_SIZE_US: i64 = 60 * 1_000_000;
const SAMPLE_CAP: usize = 1024;

/// 未配对 span 暂存上限（防 span_id 永不配对时内存无界增长）。
const SPAN_CAP: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EdgeKey {
    pub org_id: Id,
    pub client_service: String,
    pub server_service: String,
    /// The caller's `deployment.environment(.name)`, else the callee's; empty when neither says.
    pub environment: String,
    pub bucket_at_micros: i64,
}

#[derive(Debug, Default)]
struct Bucket {
    request_count: u64,
    error_count: u64,
    samples_us: Vec<i64>,
}

/// 一条已观察、待配对的 span（按 span_id 索引）。
#[derive(Debug, Clone)]
struct PendingSpan {
    service: String,
    environment: String,
    ts: TimestampMicros,
    duration_us: i64,
    is_error: bool,
    /// 观察时的墙钟时间（μs），用于 [`ServiceGraphAggregator::prune`] 过期清理。
    seen_at_us: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSnapshot {
    pub id: Id,
    pub org_id: Id,
    pub client_service: String,
    pub server_service: String,
    #[serde(default)]
    pub environment: String,
    pub bucket_at: i64,
    pub request_count: u64,
    pub error_count: u64,
    pub p50_us: Option<i64>,
    pub p95_us: Option<i64>,
    pub p99_us: Option<i64>,
}

#[derive(Default)]
pub struct ServiceGraphAggregator {
    buckets: DashMap<EdgeKey, Mutex<Bucket>>,
    /// `(org_id, trace_id, span_id)` → span 信息：供后到的子 span 找到其父。
    pending: DashMap<(Id, String, String), PendingSpan>,
    /// `(org_id, trace_id, parent_span_id)` → 已到但其父尚未到的子 span。
    waiting: DashMap<(Id, String, String), Vec<PendingSpan>>,
}

impl ServiceGraphAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一条 span 派生的 edge 样本。
    /// `ts` 为 span 的发生时间；`duration_us` 端到端耗时；`is_error` 来自 status code。
    pub fn record(
        &self,
        org_id: &Id,
        client_service: &str,
        server_service: &str,
        ts: TimestampMicros,
        duration_us: i64,
        is_error: bool,
    ) {
        self.record_in(
            org_id,
            client_service,
            server_service,
            "",
            ts,
            duration_us,
            is_error,
        );
    }

    /// [`Self::record`] for an edge in `environment`.
    #[allow(clippy::too_many_arguments)]
    pub fn record_in(
        &self,
        org_id: &Id,
        client_service: &str,
        server_service: &str,
        environment: &str,
        ts: TimestampMicros,
        duration_us: i64,
        is_error: bool,
    ) {
        let bucket_at = (ts.0 / BUCKET_SIZE_US) * BUCKET_SIZE_US;

        let key = EdgeKey {
            org_id: org_id.clone(),
            client_service: client_service.to_string(),
            server_service: server_service.to_string(),
            environment: environment.to_string(),
            bucket_at_micros: bucket_at,
        };

        let entry = self.buckets.entry(key).or_default();
        let mut b = entry.lock();
        b.request_count += 1;

        if is_error {
            b.error_count += 1;
        }

        if b.samples_us.len() < SAMPLE_CAP {
            b.samples_us.push(duration_us.max(0));
        }
    }

    /// 观察一条 span：按 span_id 与父子关系配对，跨服务即累计一条调用边。
    ///
    /// 配对是双向的：子 span 到达时若其父已记录（且服务不同）→ 立即连边；父尚未到达 →
    /// 暂存等待，父到达时回填。边方向 = 父(调用方)服务 → 子(被调方)服务；该边的时延与
    /// 错误取**子 span**（被调端视角的 RED 指标）。同一服务内部的父子 span 不连边。
    ///
    /// 注：配对状态是单进程内存态——分布式部署下同一 trace 的父子 span 若落到不同
    /// intake 进程则无法配对（边会偏少，但不会出现错误的边）。
    #[allow(clippy::too_many_arguments)]
    pub fn observe_span(
        &self,
        org_id: &Id,
        trace_id: &str,
        span_id: &str,
        parent_span_id: Option<&str>,
        service: &str,
        ts: TimestampMicros,
        duration_us: i64,
        is_error: bool,
    ) {
        self.observe_span_in(
            org_id,
            trace_id,
            span_id,
            parent_span_id,
            service,
            "",
            ts,
            duration_us,
            is_error,
        );
    }

    /// [`Self::observe_span`] for a span of a service in `environment`: the
    /// edge is in the caller's environment, else in the callee's.
    #[allow(clippy::too_many_arguments)]
    pub fn observe_span_in(
        &self,
        org_id: &Id,
        trace_id: &str,
        span_id: &str,
        parent_span_id: Option<&str>,
        service: &str,
        environment: &str,
        ts: TimestampMicros,
        duration_us: i64,
        is_error: bool,
    ) {
        if trace_id.is_empty() || span_id.is_empty() {
            return;
        }

        let me = PendingSpan {
            service: service.to_string(),
            environment: environment.to_string(),
            ts,
            duration_us: duration_us.max(0),
            is_error,
            seen_at_us: TimestampMicros::now().0,
        };

        // 1) 正向：我的父是否已记录？记录了就立即连边，否则把自己挂到 waiting 等父到达。
        if let Some(parent) = parent_span_id.filter(|p| !p.is_empty()) {
            let pk = (org_id.clone(), trace_id.to_string(), parent.to_string());
            let parent = self
                .pending
                .get(&pk)
                .map(|p| (p.service.clone(), p.environment.clone()));

            match parent {
                Some((psvc, penv)) if psvc != service => {
                    self.record_in(
                        org_id,
                        &psvc,
                        service,
                        edge_environment(&penv, environment),
                        ts,
                        me.duration_us,
                        is_error,
                    );
                }
                Some(_) => {} // 同服务内部边，忽略
                None => {
                    self.waiting.entry(pk).or_default().push(me.clone());
                }
            }
        }

        // 2) 记录自身，供后到的子 span 配对；超上限只跳过存储（不影响已存配对）。
        let myk = (org_id.clone(), trace_id.to_string(), span_id.to_string());

        if self.pending.len() < SPAN_CAP {
            self.pending.insert(myk.clone(), me.clone());
        }

        // 3) 反向：回填此前等待"我"作为父的子 span。
        if let Some((_, children)) = self.waiting.remove(&myk) {
            for child in children {
                if me.service != child.service {
                    self.record_in(
                        org_id,
                        &me.service,
                        &child.service,
                        edge_environment(&me.environment, &child.environment),
                        child.ts,
                        child.duration_us,
                        child.is_error,
                    );
                }
            }
        }
    }

    /// 清理早于 `cutoff_micros`（按观察墙钟时间）的未配对 span，防内存累积。
    /// 由 flush worker 每个 tick 调一次。
    pub fn prune(&self, cutoff_micros: i64) {
        self.pending.retain(|_, v| v.seen_at_us >= cutoff_micros);

        self.waiting.retain(|_, v| {
            v.retain(|c| c.seen_at_us >= cutoff_micros);
            !v.is_empty()
        });
    }

    /// 把所有 `bucket_at < cutoff` 的桶 drain 出来转 snapshot。
    pub fn flush_due(&self, cutoff_micros: i64) -> Vec<EdgeSnapshot> {
        let mut drained: Vec<EdgeKey> = Vec::new();

        for kv in self.buckets.iter() {
            if kv.key().bucket_at_micros < cutoff_micros {
                drained.push(kv.key().clone());
            }
        }

        let mut snaps = Vec::with_capacity(drained.len());

        for key in drained {
            if let Some((k, m)) = self.buckets.remove(&key) {
                let b = m.into_inner();
                let mut s = b.samples_us;
                s.sort_unstable();

                let p = |q: f64| -> Option<i64> {
                    if s.is_empty() {
                        None
                    } else {
                        let idx = ((s.len() as f64) * q).ceil() as usize;
                        let idx = idx.saturating_sub(1).min(s.len() - 1);
                        Some(s[idx])
                    }
                };

                snaps.push(EdgeSnapshot {
                    id: Id::new(),
                    org_id: k.org_id,
                    client_service: k.client_service,
                    server_service: k.server_service,
                    environment: k.environment,
                    bucket_at: k.bucket_at_micros,
                    request_count: b.request_count,
                    error_count: b.error_count,
                    p50_us: p(0.50),
                    p95_us: p(0.95),
                    p99_us: p(0.99),
                });
            }
        }

        snaps
    }
}

/// The environment of an edge: the caller's, else the callee's.
fn edge_environment<'a>(caller: &'a str, callee: &'a str) -> &'a str {
    if caller.is_empty() { callee } else { caller }
}

/// 把 trace 事件批旁路喂给 [`ServiceGraphAggregator`] 的 [`ServiceGraphObserver`] 适配器。
///
/// 从 OTLP/native trace 接入约定的字段抽取：`trace_id` / `span_id` / `parent_span_id` /
/// `service.name` / `duration_ns` / `status_code`（缺 Trace/Span ID 的事件跳过）。
pub struct ServiceGraphObserverImpl {
    agg: Arc<ServiceGraphAggregator>,
}

impl ServiceGraphObserverImpl {
    pub fn new(agg: Arc<ServiceGraphAggregator>) -> Self {
        Self { agg }
    }
}

impl ServiceGraphObserver for ServiceGraphObserverImpl {
    fn observe(&self, org_id: &Id, events: &[RawEvent]) {
        for ev in events {
            let f = &ev.fields;

            let Some(trace_id) = f.get("trace_id").and_then(|v| v.as_str()) else {
                continue;
            };

            let Some(span_id) = f.get("span_id").and_then(|v| v.as_str()) else {
                continue;
            };

            let service = effective_service_name(
                f.get("service.name").and_then(|v| v.as_str()),
                f.get("service.namespace").and_then(|v| v.as_str()),
                f.get("molesignal.execution.role").and_then(|v| v.as_str()),
            );

            let parent = f.get("parent_span_id").and_then(|v| v.as_str());
            let environment = ENVIRONMENT_FIELDS
                .iter()
                .find_map(|field| f.get(*field).and_then(|v| v.as_str()))
                .unwrap_or_default();

            let duration_us = f
                .get("duration_ns")
                .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
                .unwrap_or(0)
                / 1_000;

            let is_error = f
                .get("status_code")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("ERROR"))
                .unwrap_or(false);

            self.agg.observe_span_in(
                org_id,
                trace_id,
                span_id,
                parent,
                &service,
                environment,
                ev.timestamp,
                duration_us,
                is_error,
            );
        }
    }
}

/// The fields that say a span's environment, as APM reads them.
pub const ENVIRONMENT_FIELDS: [&str; 2] = ["deployment.environment.name", "deployment.environment"];

/// Returns the canonical service identity used by both live and recomputed graph edges.
pub fn effective_service_name(
    service: Option<&str>,
    namespace: Option<&str>,
    execution_role: Option<&str>,
) -> String {
    canonical_effective_service_name(service, namespace, execution_role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregator_buckets_by_minute_and_emits_quantiles() {
        let a = ServiceGraphAggregator::new();
        let org = Id("orgA".to_string());
        let base = TimestampMicros(120 * BUCKET_SIZE_US);
        for i in 0..100u32 {
            a.record(&org, "web", "api", base, i as i64 * 1_000, i % 10 == 0);
        }
        // next minute
        let next = TimestampMicros(121 * BUCKET_SIZE_US);
        a.record(&org, "web", "api", next, 5_000, false);

        let snaps = a.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        let s = &snaps[0];
        assert_eq!(s.request_count, 100);
        assert_eq!(s.error_count, 10);
        assert!(s.p50_us.unwrap() < s.p95_us.unwrap());
        assert!(s.p95_us.unwrap() <= s.p99_us.unwrap());
    }

    #[test]
    fn samples_capped() {
        let a = ServiceGraphAggregator::new();
        let org = Id("orgA".to_string());
        let base = TimestampMicros(0);
        for i in 0..(SAMPLE_CAP * 3) {
            a.record(&org, "a", "b", base, i as i64, false);
        }
        let snaps = a.flush_due(BUCKET_SIZE_US);
        assert_eq!(snaps[0].request_count, (SAMPLE_CAP * 3) as u64);
        // p99 必须 < SAMPLE_CAP（说明只用了前 cap 个样本）
        assert!(snaps[0].p99_us.unwrap() < SAMPLE_CAP as i64);
    }

    #[test]
    fn observe_pairs_parent_then_child_as_edge() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);
        // 父(调用方) span 先到
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 10_000, false);
        // 子(被调方) span 后到，parent = p1
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 5_000, true);
        let snaps = a.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].client_service, "web");
        assert_eq!(snaps[0].server_service, "api");
        assert_eq!(snaps[0].request_count, 1);
        assert_eq!(snaps[0].error_count, 1, "错误取子 span 的 status");
    }

    #[test]
    fn observe_pairs_child_before_parent_via_backfill() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);
        // 子先到 → 进 waiting，尚无边
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 5_000, false);
        assert!(
            a.flush_due(121 * BUCKET_SIZE_US).is_empty(),
            "父未到不应有边"
        );
        // 父到达 → 回填成边
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 10_000, false);
        let snaps = a.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].client_service, "web");
        assert_eq!(snaps[0].server_service, "api");
    }

    #[test]
    fn an_edge_is_in_the_callers_environment_else_the_callees() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);

        a.observe_span_in(&org, "t1", "p1", None, "web", "prod", ts, 10_000, false);
        a.observe_span_in(
            &org,
            "t1",
            "c1",
            Some("p1"),
            "api",
            "staging",
            ts,
            5_000,
            false,
        );
        a.observe_span_in(
            &org,
            "t2",
            "c2",
            Some("p2"),
            "api",
            "prod",
            ts,
            5_000,
            false,
        );
        a.observe_span_in(&org, "t2", "p2", None, "cron", "", ts, 10_000, false);

        let mut snaps = a.flush_due(121 * BUCKET_SIZE_US);
        snaps.sort_by(|left, right| left.client_service.cmp(&right.client_service));

        assert_eq!(snaps.len(), 2);
        assert_eq!(
            (
                snaps[0].client_service.as_str(),
                snaps[0].environment.as_str()
            ),
            ("cron", "prod"),
        );
        assert_eq!(
            (
                snaps[1].client_service.as_str(),
                snaps[1].environment.as_str()
            ),
            ("web", "prod"),
        );
    }

    #[test]
    fn observe_skips_intra_service_parent_child() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 10_000, false);
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "web", ts, 5_000, false);
        assert!(
            a.flush_due(121 * BUCKET_SIZE_US).is_empty(),
            "同服务内部父子不连边"
        );
    }

    #[test]
    fn prune_drops_stale_unpaired_spans() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(0);
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 1_000, false);
        a.prune(i64::MAX); // 清空所有未配对态
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 1_000, false);
        assert!(
            a.flush_due(BUCKET_SIZE_US).is_empty(),
            "子已被 prune，父到达不应回填"
        );
    }

    #[test]
    fn observer_impl_extracts_edges_from_trace_events() {
        use serde_json::json;
        let agg = Arc::new(ServiceGraphAggregator::new());
        let obs = ServiceGraphObserverImpl::new(agg.clone());
        let org = Id("o".to_string());
        let mk = |span: &str, parent: Option<&str>, svc: &str, status: &str| {
            let mut m = json!({
                "trace_id": "trace-a",
                "span_id": span,
                "service.name": svc,
                "duration_ns": 2_000_000u64,
                "status_code": status,
            })
            .as_object()
            .unwrap()
            .clone();
            if let Some(p) = parent {
                m.insert("parent_span_id".into(), json!(p));
            }
            RawEvent {
                timestamp: TimestampMicros(120 * BUCKET_SIZE_US),
                fields: m,
            }
        };
        obs.observe(
            &org,
            &[
                mk("p1", None, "web", "OK"),
                mk("c1", Some("p1"), "api", "ERROR"),
            ],
        );
        let snaps = agg.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].client_service, "web");
        assert_eq!(snaps[0].server_service, "api");
        assert_eq!(snaps[0].error_count, 1);
        assert_eq!(snaps[0].p95_us, Some(2_000), "2_000_000ns → 2000us");
    }

    #[test]
    fn pairing_isolated_by_trace_id_and_execution_role_is_effective_service() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);

        a.observe_span(
            &org,
            "trace-a",
            "child",
            Some("shared-parent"),
            "molesignal-querier",
            ts,
            2_000,
            false,
        );
        a.observe_span(
            &org,
            "trace-b",
            "shared-parent",
            None,
            "molesignal-router",
            ts,
            3_000,
            false,
        );
        assert!(
            a.flush_due(121 * BUCKET_SIZE_US).is_empty(),
            "相同 span_id 不得跨 Trace 配对"
        );

        a.observe_span(
            &org,
            "trace-a",
            "shared-parent",
            None,
            "molesignal-router",
            ts,
            3_000,
            false,
        );
        let snaps = a.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].client_service, "molesignal-router");
        assert_eq!(snaps[0].server_service, "molesignal-querier");
        assert_eq!(
            effective_service_name(Some("molesignal"), Some("molesignal"), Some("alert")),
            "molesignal-alert"
        );
        assert_eq!(
            effective_service_name(Some("checkout"), None, Some("router")),
            "checkout"
        );
    }
}
