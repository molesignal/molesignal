// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service graph aggregator + repository.
//!
//! Workflow:
//! 1. Before intake flushes a RecordBatch of the traces stream, it iterates over each span and
//!    accumulates the samples of each client->server edge, via
//!    [`ServiceGraphAggregator::record`], into `DashMap<EdgeKey, Bucket>`.
//! 2. At each minute boundary (when `bucket_at = floor(now / 60s)` changes),
//!    [`ServiceGraphAggregator::flush_due`] drains the buckets of the previous window, converts
//!    them to [`EdgeSnapshot`], and writes them to the `service_graph_edges` table through
//!    [`ServiceGraphRepository::insert_many`].
//! 3. HTTP `GET /api/v1/traces/service_graph` returns the edge set via
//!    [`ServiceGraphRepository::query`].
//!
//! Quantiles use a sliding sample per bucket (capped at 1024, so long tails cannot blow up
//! memory) plus a simplified nearest-rank.

use std::sync::Arc;

use ::domain::intake::{RawEvent, ServiceGraphObserver};
use dashmap::DashMap;
use kernel::{ids::Id, time::TimestampMicros};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use signals::trace_normalization::effective_service_name as canonical_effective_service_name;

mod repository;

pub use repository::{PgServiceGraphRepository, ServiceGraphRepository};

const BUCKET_SIZE_US: i64 = 60 * 1_000_000;
const SAMPLE_CAP: usize = 1024;

/// Cap on buffered unpaired spans (prevents unbounded memory growth if a span_id never pairs).
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

/// A span that has been observed and awaits pairing (indexed by span_id).
#[derive(Debug, Clone)]
struct PendingSpan {
    service: String,
    environment: String,
    ts: TimestampMicros,
    duration_us: i64,
    is_error: bool,
    /// Wall-clock time (μs) when observed, used by [`ServiceGraphAggregator::prune`] to clean up
    /// expired entries.
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
    /// `(org_id, trace_id, span_id)` -> span info, which lets a child span that arrives later
    /// find its parent.
    pending: DashMap<(Id, String, String), PendingSpan>,
    /// `(org_id, trace_id, parent_span_id)` -> child spans that have arrived while their parent
    /// has not yet arrived.
    waiting: DashMap<(Id, String, String), Vec<PendingSpan>>,
}

impl ServiceGraphAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one edge sample derived from a span.
    /// `ts` is when the span occurred; `duration_us` is the end-to-end duration; `is_error` is
    /// derived from the status code.
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

    /// Observes one span: pairs it by span_id and parent-child relationship, and when the pair
    /// crosses services, accumulates one call edge.
    ///
    /// Pairing is bidirectional: when a child span arrives and its parent is already recorded
    /// (and the services differ) -> connect the edge immediately; if the parent has not arrived
    /// yet -> buffer the child and wait, backfilling when the parent arrives. Edge direction =
    /// parent (caller) service -> child (callee) service; the edge's latency and errors come from
    /// the **child span** (RED metrics from the callee's perspective). Parent and child spans in
    /// the same service form no edge.
    ///
    /// Note: the pairing state is held in the memory of a single process. In a distributed
    /// deployment, if the parent and child spans of the same trace land on different intake
    /// processes, they cannot be paired (fewer edges result, but never incorrect ones).
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

        // 1) Forward: is my parent already recorded? If so, connect the edge immediately,
        // otherwise park this span in waiting until the parent arrives.
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
                Some(_) => {} // Edge within a single service, ignored
                None => {
                    self.waiting.entry(pk).or_default().push(me.clone());
                }
            }
        }

        // 2) Record itself so that child spans arriving later can pair with it; over the cap,
        // only the storing is skipped (pairings already stored are unaffected).
        let myk = (org_id.clone(), trace_id.to_string(), span_id.to_string());

        if self.pending.len() < SPAN_CAP {
            self.pending.insert(myk.clone(), me.clone());
        }

        // 3) Reverse: backfill the child spans previously waiting for "me" as their parent.
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

    /// Removes unpaired spans older than `cutoff_micros` (by observation wall-clock time) to
    /// prevent memory buildup. Called once per tick by the flush worker.
    pub fn prune(&self, cutoff_micros: i64) {
        self.pending.retain(|_, v| v.seen_at_us >= cutoff_micros);

        self.waiting.retain(|_, v| {
            v.retain(|c| c.seen_at_us >= cutoff_micros);
            !v.is_empty()
        });
    }

    /// Drains all buckets with `bucket_at < cutoff` and converts them to snapshots.
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

/// A [`ServiceGraphObserver`] adapter that feeds trace event batches to
/// [`ServiceGraphAggregator`] on a side path.
///
/// Extracts fields following the OTLP/native trace intake convention: `trace_id` / `span_id` /
/// `parent_span_id` / `service.name` / `duration_ns` / `status_code` (events without a
/// Trace/Span ID are skipped).
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
        // p99 must be < SAMPLE_CAP (which shows only the first cap samples were used)
        assert!(snaps[0].p99_us.unwrap() < SAMPLE_CAP as i64);
    }

    #[test]
    fn observe_pairs_parent_then_child_as_edge() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);
        // Parent (caller) span arrives first
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 10_000, false);
        // Child (callee) span arrives later, parent = p1
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 5_000, true);
        let snaps = a.flush_due(121 * BUCKET_SIZE_US);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].client_service, "web");
        assert_eq!(snaps[0].server_service, "api");
        assert_eq!(snaps[0].request_count, 1);
        assert_eq!(
            snaps[0].error_count, 1,
            "the error count follows the child span's status"
        );
    }

    #[test]
    fn observe_pairs_child_before_parent_via_backfill() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(120 * BUCKET_SIZE_US);
        // Child arrives first -> goes into waiting, no edge yet
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 5_000, false);
        assert!(
            a.flush_due(121 * BUCKET_SIZE_US).is_empty(),
            "no edge before the parent arrives"
        );
        // Parent arrives -> backfilled into an edge
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
            "no edge between a parent and child in the same service"
        );
    }

    #[test]
    fn prune_drops_stale_unpaired_spans() {
        let a = ServiceGraphAggregator::new();
        let org = Id("o".to_string());
        let ts = TimestampMicros(0);
        a.observe_span(&org, "trace-a", "c1", Some("p1"), "api", ts, 1_000, false);
        a.prune(i64::MAX); // Clear all unpaired state
        a.observe_span(&org, "trace-a", "p1", None, "web", ts, 1_000, false);
        assert!(
            a.flush_due(BUCKET_SIZE_US).is_empty(),
            "the child was pruned, so the arriving parent must not backfill an edge"
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
            "the same span_id must not pair across traces"
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
