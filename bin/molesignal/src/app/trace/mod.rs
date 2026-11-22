// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CanonicalSpan producer → tail sampler → 两个隔离 sink 的统一管线。

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use serde::Serialize;
use signals::{
    tail_sampling::{DecidedTrace, SamplerOutput, TailSampler, TraceCandidate},
    trace_metrics,
    trace_normalization::TraceLimits,
};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

pub mod candidate_router;
pub mod durable;
pub mod export;
mod owner;
mod processing;
mod self_intake;
mod sink_worker;
pub(crate) mod summary;

use durable::{SpanOrigin, SpanSettlement};
pub use self_intake::SelfIntakeTraceSink;
use sink_worker::{Delivery, SinkEndpoint, fanout, make_sink_worker, record_shutdown_residue};
pub use sink_worker::{TraceSinkHealth, TraceSinkHealthSnapshot, TraceSinkWorkerConfig};

#[derive(Debug, Clone, Copy)]
pub struct TracePipelineConfig {
    pub candidate_capacity: usize,
    pub decision_tick: Duration,
    pub shutdown_timeout: Duration,
    pub self_intake: TraceSinkWorkerConfig,
    pub external: TraceSinkWorkerConfig,
}

impl Default for TracePipelineConfig {
    fn default() -> Self {
        Self {
            candidate_capacity: 16_384,
            decision_tick: Duration::from_millis(100),
            shutdown_timeout: Duration::from_secs(10),
            self_intake: TraceSinkWorkerConfig::default(),
            external: TraceSinkWorkerConfig::default(),
        }
    }
}

impl TracePipelineConfig {
    pub fn validate(self) -> Result<Self, String> {
        if self.candidate_capacity == 0
            || self.decision_tick.is_zero()
            || self.shutdown_timeout.is_zero()
        {
            return Err("Trace pipeline capacity/timeouts must be non-zero".into());
        }

        self.self_intake.validate()?;
        self.external.validate()?;
        Ok(self)
    }
}

#[async_trait]
pub trait TraceSink: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether the sink wants a trace. One it does not want reaches neither its queue nor its counts.
    fn takes(&self, _trace: &DecidedTrace) -> bool {
        true
    }

    /// Writes `traces`. On failure the sink leaves in `traces` only what it has not written, so
    /// the retry that follows writes nothing twice; a sink that cannot tell leaves them all.
    async fn export(&self, traces: &mut Vec<DecidedTrace>) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracePipelineHealthSnapshot {
    pub accepting: bool,
    pub candidate_queue_depth: usize,
    pub candidate_queue_capacity: usize,
    pub candidate_drops: u64,
    pub self_intake: Option<TraceSinkHealthSnapshot>,
    pub external: Option<TraceSinkHealthSnapshot>,
}

/// A candidate in the owner's queue, with its place in the candidate log when it is logged.
pub(super) type QueuedSpan = (TraceCandidate, Option<SpanOrigin>);

/// Told when logged spans are settled; attached once at start-up, shared with the workers.
pub(super) type SettlementSlot = Arc<std::sync::OnceLock<Arc<dyn SpanSettlement>>>;

pub struct TracePipeline {
    candidate_sender: mpsc::Sender<QueuedSpan>,
    settlement: SettlementSlot,
    candidate_depth: Arc<AtomicUsize>,
    candidate_capacity: usize,
    candidate_drops: AtomicU64,
    accepting: Arc<AtomicBool>,
    self_intake_health: Option<Arc<TraceSinkHealth>>,
    external_health: Option<Arc<TraceSinkHealth>>,
    cancel: CancellationToken,
    joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    shutdown_timeout: Duration,
    limits: TraceLimits,
}

impl TracePipeline {
    pub fn start(
        sampler: Arc<TailSampler>,
        self_intake_sink: Option<Arc<dyn TraceSink>>,
        external_sink: Option<Arc<dyn TraceSink>>,
        config: TracePipelineConfig,
        limits: TraceLimits,
    ) -> Result<Arc<Self>, String> {
        Self::start_with_apm(
            sampler,
            None,
            self_intake_sink,
            external_sink,
            config,
            limits,
        )
    }

    pub fn start_with_apm(
        sampler: Arc<TailSampler>,
        apm_projector: Option<Arc<dyn crate::app::apm::ApmCandidateProjector>>,
        self_intake_sink: Option<Arc<dyn TraceSink>>,
        external_sink: Option<Arc<dyn TraceSink>>,
        config: TracePipelineConfig,
        limits: TraceLimits,
    ) -> Result<Arc<Self>, String> {
        let config = config.validate()?;
        let (candidate_sender, candidate_receiver) = mpsc::channel(config.candidate_capacity);
        let candidate_depth = Arc::new(AtomicUsize::new(0));
        trace_metrics::set_queue("candidate", 0, config.candidate_capacity);
        let accepting = Arc::new(AtomicBool::new(true));
        let cancel = CancellationToken::new();
        let settlement = SettlementSlot::default();
        let (self_endpoint, self_health, self_join) = make_sink_worker(
            self_intake_sink,
            config.self_intake,
            limits,
            Delivery::Lossless,
            settlement.clone(),
        );
        let (external_endpoint, external_health, external_join) = make_sink_worker(
            external_sink,
            config.external,
            limits,
            Delivery::BestEffort,
            SettlementSlot::default(),
        );

        let owner_join = owner::spawn_candidate_owner(owner::CandidateOwnerRuntime {
            sampler,
            apm_projector,
            receiver: candidate_receiver,
            depth: candidate_depth.clone(),
            accepting: accepting.clone(),
            cancel: cancel.clone(),
            config,
            self_endpoint,
            external_endpoint,
            settlement: settlement.clone(),
        });

        let mut joins = vec![owner_join];

        if let Some(join) = self_join {
            joins.push(join);
        }

        if let Some(join) = external_join {
            joins.push(join);
        }

        Ok(Arc::new(Self {
            candidate_sender,
            settlement,
            candidate_depth,
            candidate_capacity: config.candidate_capacity,
            candidate_drops: AtomicU64::new(0),
            accepting,
            self_intake_health: self_health,
            external_health,
            cancel,
            joins: Mutex::new(Some(joins)),
            shutdown_timeout: config.shutdown_timeout,
            limits,
        }))
    }

    /// producer 热路径：只做 `try_send`，永不等待 owner/sink/network。
    pub fn try_submit(&self, mut candidate: TraceCandidate) -> Result<(), TraceSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            self.candidate_drops.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("candidate", "stopped", 1);
            return Err(TraceSubmitError::Stopped);
        }

        // Producer enqueue 与 sink export 各做一次同源 invariant check，避免 RPC/WAL
        // 边界把未清理的用户值带入 sampler cache。
        signals::trace_normalization::sanitize_and_limit_span(&mut candidate.span, self.limits);

        self.candidate_depth.fetch_add(1, Ordering::Relaxed);

        trace_metrics::set_queue(
            "candidate",
            self.candidate_depth.load(Ordering::Relaxed),
            self.candidate_capacity,
        );

        match self.candidate_sender.try_send((candidate, None)) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.candidate_depth.fetch_sub(1, Ordering::Relaxed);
                self.candidate_drops.fetch_add(1, Ordering::Relaxed);

                trace_metrics::set_queue(
                    "candidate",
                    self.candidate_depth.load(Ordering::Relaxed),
                    self.candidate_capacity,
                );

                trace_metrics::record_spans("candidate", "queue_full", 1);
                Err(TraceSubmitError::Full)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.candidate_depth.fetch_sub(1, Ordering::Relaxed);
                self.candidate_drops.fetch_add(1, Ordering::Relaxed);

                trace_metrics::set_queue(
                    "candidate",
                    self.candidate_depth.load(Ordering::Relaxed),
                    self.candidate_capacity,
                );

                trace_metrics::record_spans("candidate", "stopped", 1);
                Err(TraceSubmitError::Stopped)
            }
        }
    }

    /// Waits for room in the candidate queue instead of refusing: the router hands over tenant
    /// spans their senders were already told were accepted, and a full queue pushes back on the
    /// router rather than dropping them.
    ///
    /// `origin` is the span's place in the candidate log; it is settled once the span's fate is
    /// durable.
    pub async fn submit(
        &self,
        mut candidate: TraceCandidate,
        origin: Option<SpanOrigin>,
    ) -> Result<(), TraceSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            self.candidate_drops.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("candidate", "stopped", 1);
            return Err(TraceSubmitError::Stopped);
        }

        signals::trace_normalization::sanitize_and_limit_span(&mut candidate.span, self.limits);

        let Ok(permit) = self.candidate_sender.reserve().await else {
            self.candidate_drops.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("candidate", "stopped", 1);
            return Err(TraceSubmitError::Stopped);
        };

        self.candidate_depth.fetch_add(1, Ordering::Relaxed);

        trace_metrics::set_queue(
            "candidate",
            self.candidate_depth.load(Ordering::Relaxed),
            self.candidate_capacity,
        );

        permit.send((candidate, origin));
        Ok(())
    }

    /// Takes a producer's batch as a sampler owner: room for it is reserved, its tenant spans
    /// are written to this node's candidate log when `log` is given, and only then do the spans
    /// enter the queue, carrying their places in the log. A refusal of the first chunk leaves
    /// nothing behind; a batch larger than the queue goes in chunks, the later ones waiting for
    /// room.
    pub async fn submit_logged_batch(
        &self,
        candidates: Vec<TraceCandidate>,
        log: Option<&durable::TraceCandidateLog>,
    ) -> Result<(), TraceSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(TraceSubmitError::Stopped);
        }

        let mut candidates = candidates;

        for candidate in &mut candidates {
            signals::trace_normalization::sanitize_and_limit_span(&mut candidate.span, self.limits);
        }

        let chunk_size = self.candidate_capacity.max(1);
        let mut first = true;

        for chunk in candidates.chunks(chunk_size) {
            let permits = if first {
                match self.candidate_sender.try_reserve_many(chunk.len()) {
                    Ok(permits) => permits,
                    Err(mpsc::error::TrySendError::Full(())) => {
                        trace_metrics::record_spans("candidate", "queue_full", chunk.len() as u64);
                        return Err(TraceSubmitError::Full);
                    }
                    Err(mpsc::error::TrySendError::Closed(())) => {
                        return Err(TraceSubmitError::Stopped);
                    }
                }
            } else {
                self.candidate_sender
                    .reserve_many(chunk.len())
                    .await
                    .map_err(|_| TraceSubmitError::Stopped)?
            };

            first = false;

            let origins = match log {
                Some(log) => log.append_candidates(chunk).await.map_err(|error| {
                    tracing::warn!(%error, "a candidate batch could not be logged");
                    TraceSubmitError::Unlogged
                })?,
                None => vec![None; chunk.len()],
            };

            self.candidate_depth
                .fetch_add(chunk.len(), Ordering::Relaxed);

            trace_metrics::set_queue(
                "candidate",
                self.candidate_depth.load(Ordering::Relaxed),
                self.candidate_capacity,
            );

            for ((permit, candidate), origin) in permits.zip(chunk.iter().cloned()).zip(origins) {
                permit.send((candidate, origin));
            }
        }

        Ok(())
    }

    /// Settles logged spans as their fate becomes durable. Called once, before the first span.
    pub fn attach_settlement(&self, settlement: Arc<dyn SpanSettlement>) {
        if self.settlement.set(settlement).is_err() {
            tracing::warn!("a span settlement was attached twice; the first one is kept");
        }
    }

    pub fn submit_output_for_test(&self, _output: SamplerOutput) {
        // Intentionally unavailable in production: all retained sets must originate at sampler.
    }

    pub fn health(&self) -> TracePipelineHealthSnapshot {
        TracePipelineHealthSnapshot {
            accepting: self.accepting.load(Ordering::Acquire),
            candidate_queue_depth: self.candidate_depth.load(Ordering::Relaxed),
            candidate_queue_capacity: self.candidate_capacity,
            candidate_drops: self.candidate_drops.load(Ordering::Relaxed),
            self_intake: self
                .self_intake_health
                .as_ref()
                .map(|health| health.snapshot()),
            external: self
                .external_health
                .as_ref()
                .map(|health| health.snapshot()),
        }
    }

    /// 先停 candidate，再决策/flush tail cache 与两个 sink；deadline 后 abort 并记残留。
    pub async fn shutdown(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }

        self.cancel.cancel();

        let Some(mut joins) = self.joins.lock().await.take() else {
            return;
        };

        if timeout(self.shutdown_timeout, async {
            for join in &mut joins {
                let _ = join.await;
            }
        })
        .await
        .is_err()
        {
            for join in joins {
                join.abort();
            }

            let candidate_residue = self.candidate_depth.swap(0, Ordering::Relaxed);
            self.candidate_drops
                .fetch_add(candidate_residue as u64, Ordering::Relaxed);
            trace_metrics::set_queue("candidate", 0, self.candidate_capacity);
            trace_metrics::record_spans("candidate", "shutdown_timeout", candidate_residue as u64);
            let self_intake_residue =
                record_shutdown_residue("self_intake", self.self_intake_health.as_ref());
            let external_residue =
                record_shutdown_residue("external", self.external_health.as_ref());
            tracing::warn!(
                target: "molesignal::app::trace",
                candidate_residue,
                self_intake_residue,
                external_residue,
                "Trace pipeline shutdown timed out"
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSubmitError {
    Full,
    Stopped,
    /// The candidate log could not store the spans.
    Unlogged,
}

/// 测试/诊断 sink：按 org 聚合，生产代码不使用。
#[derive(Default)]
pub struct MemoryTraceSink {
    traces: Mutex<BTreeMap<String, Vec<DecidedTrace>>>,
    fail: AtomicBool,
}

impl MemoryTraceSink {
    pub async fn traces(&self, org_id: &str) -> Vec<DecidedTrace> {
        self.traces
            .lock()
            .await
            .get(org_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::Release);
    }
}

#[async_trait]
impl TraceSink for MemoryTraceSink {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn export(&self, traces: &mut Vec<DecidedTrace>) -> Result<(), String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("injected failure".into());
        }

        let mut stored = self.traces.lock().await;

        for trace in traces.iter() {
            stored
                .entry(trace.org_id.clone())
                .or_default()
                .push(trace.clone());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use domain::apm::ApmSpanFact;
    use signals::{
        tail_sampling::{CandidateDisposition, ForceKeep, TraceRuntimePolicy},
        trace_fixtures,
        trace_normalization::CanonicalSpan,
    };
    use tokio::time::{Instant, sleep};

    use super::*;
    use crate::app::apm::ApmCandidateProjector;

    #[derive(Default)]
    struct RecordingApmProjector {
        dispositions: parking_lot::Mutex<Vec<CandidateDisposition>>,
    }

    impl ApmCandidateProjector for RecordingApmProjector {
        fn extract(&self, org_id: &str, span: &CanonicalSpan) -> Option<ApmSpanFact> {
            crate::app::apm::extract_apm_fact(org_id, span)
        }

        fn project(&self, _fact: ApmSpanFact, disposition: CandidateDisposition) {
            self.dispositions.lock().push(disposition);
        }
    }

    struct BlockingTraceSink {
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl TraceSink for BlockingTraceSink {
        fn name(&self) -> &'static str {
            "self_intake"
        }

        async fn export(&self, _traces: &mut Vec<DecidedTrace>) -> Result<(), String> {
            self.started.notify_one();
            std::future::pending().await
        }
    }

    fn fast_config() -> TracePipelineConfig {
        let sink = TraceSinkWorkerConfig {
            queue_capacity: 4,
            batch_size: 1,
            batch_delay: Duration::from_millis(1),
            export_timeout: Duration::from_millis(20),
            max_attempts: 1,
            initial_backoff: Duration::from_millis(1),
        };
        TracePipelineConfig {
            candidate_capacity: 4,
            decision_tick: Duration::from_millis(2),
            shutdown_timeout: Duration::from_secs(1),
            self_intake: sink,
            external: sink,
        }
    }

    fn test_sampler() -> Arc<TailSampler> {
        let policy = TraceRuntimePolicy {
            normal_sample_ratio: 1.0,
            decision_window_ms: 5_000,
            root_grace_ms: 1,
            decision_cache_ms: 10_000,
            ..TraceRuntimePolicy::default()
        };
        Arc::new(TailSampler::new(policy, false, TraceLimits::default()).unwrap())
    }

    #[tokio::test]
    async fn both_sinks_receive_the_exact_same_retained_set() {
        let left = Arc::new(MemoryTraceSink::default());
        let right = Arc::new(MemoryTraceSink::default());
        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(left.clone()),
            Some(right.clone()),
            fast_config(),
            TraceLimits::default(),
        )
        .unwrap();
        pipeline
            .try_submit(TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span: trace_fixtures::canonical_http_trace().remove(0),
                force_keep: ForceKeep::None,
            })
            .unwrap();
        sleep(Duration::from_millis(30)).await;
        pipeline.shutdown().await;
        let left = left.traces("org").await;
        let right = right.traces("org").await;
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].trace_id, right[0].trace_id);
        assert_eq!(left[0].reason, right[0].reason);
    }

    #[derive(Default)]
    struct TenantTracesSink(MemoryTraceSink);

    #[async_trait]
    impl TraceSink for TenantTracesSink {
        fn name(&self) -> &'static str {
            "self_intake"
        }

        fn takes(&self, trace: &DecidedTrace) -> bool {
            trace.stream.is_some()
        }

        async fn export(&self, traces: &mut Vec<DecidedTrace>) -> Result<(), String> {
            self.0.export(traces).await
        }
    }

    #[tokio::test]
    async fn a_sink_neither_queues_nor_counts_a_trace_it_does_not_take() {
        let sink = Arc::new(TenantTracesSink::default());

        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(sink.clone()),
            None,
            fast_config(),
            TraceLimits::default(),
        )
        .unwrap();

        let tenant_span = trace_fixtures::canonical_http_trace().remove(0);
        let mut own_span = tenant_span.clone();

        own_span.trace_id = own_span.trace_id.chars().rev().collect();

        for (stream, span) in [(Some("default".to_string()), tenant_span), (None, own_span)] {
            pipeline
                .try_submit(TraceCandidate {
                    org_id: "org".into(),
                    stream,
                    span,
                    force_keep: ForceKeep::None,
                })
                .unwrap();
        }

        pipeline.shutdown().await;

        let health = pipeline.health().self_intake.unwrap();
        let stored = sink.0.traces("org").await;

        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].stream.as_deref(), Some("default"));
        assert_eq!(health.exported_spans, 1);
        assert_eq!(health.dropped_spans, 0);
    }

    #[tokio::test]
    async fn one_sink_failure_does_not_block_the_other() {
        let failing = Arc::new(MemoryTraceSink::default());
        failing.set_fail(true);
        let healthy = Arc::new(MemoryTraceSink::default());
        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(failing),
            Some(healthy.clone()),
            fast_config(),
            TraceLimits::default(),
        )
        .unwrap();
        pipeline
            .try_submit(TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span: trace_fixtures::canonical_error_trace().remove(0),
                force_keep: ForceKeep::None,
            })
            .unwrap();
        sleep(Duration::from_millis(30)).await;
        let health = pipeline.health();
        pipeline.shutdown().await;
        assert_eq!(healthy.traces("org").await.len(), 1);
        assert!(health.self_intake.unwrap().degraded);
        assert!(!health.external.unwrap().degraded);
    }

    #[tokio::test]
    async fn producer_queue_is_bounded_and_non_blocking() {
        let mut config = fast_config();
        config.candidate_capacity = 1;
        let pipeline =
            TracePipeline::start(test_sampler(), None, None, config, TraceLimits::default())
                .unwrap();
        let candidate = TraceCandidate {
            org_id: "org".into(),
            stream: None,
            span: trace_fixtures::canonical_http_trace().remove(0),
            force_keep: ForceKeep::None,
        };
        let _ = pipeline.try_submit(candidate.clone());
        let _ = pipeline.try_submit(candidate);
        assert!(pipeline.health().candidate_queue_depth <= 1);
        pipeline.shutdown().await;
    }

    #[tokio::test]
    async fn sampled_out_candidate_is_projected_before_decision_and_retry_is_identified() {
        let policy = TraceRuntimePolicy {
            normal_sample_ratio: 0.0,
            decision_window_ms: 5_000,
            root_grace_ms: 1,
            decision_cache_ms: 10_000,
            ..TraceRuntimePolicy::default()
        };
        let sampler =
            Arc::new(TailSampler::new(policy, false, TraceLimits::default()).expect("sampler"));
        let apm = Arc::new(RecordingApmProjector::default());
        let pipeline = TracePipeline::start_with_apm(
            sampler,
            Some(apm.clone()),
            None,
            None,
            fast_config(),
            TraceLimits::default(),
        )
        .expect("pipeline");
        let candidate = TraceCandidate {
            org_id: "org".into(),
            stream: None,
            span: trace_fixtures::canonical_http_trace().remove(0),
            force_keep: ForceKeep::None,
        };
        pipeline.try_submit(candidate.clone()).expect("first");
        pipeline.try_submit(candidate).expect("retry");
        sleep(Duration::from_millis(30)).await;
        pipeline.shutdown().await;
        assert_eq!(
            apm.dispositions.lock().as_slice(),
            [
                CandidateDisposition::Accepted,
                CandidateDisposition::IdenticalDuplicate
            ]
        );
    }

    #[tokio::test]
    async fn shutdown_timeout_records_unexported_sink_residue() {
        let started = Arc::new(tokio::sync::Notify::new());
        let sink = Arc::new(BlockingTraceSink {
            started: started.clone(),
        });
        let mut config = fast_config();
        config.shutdown_timeout = Duration::from_millis(20);
        config.self_intake.export_timeout = Duration::from_secs(10);
        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(sink),
            None,
            config,
            TraceLimits::default(),
        )
        .unwrap();
        pipeline
            .try_submit(TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span: trace_fixtures::canonical_error_trace().remove(0),
                force_keep: ForceKeep::None,
            })
            .unwrap();
        timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("blocking sink started");

        let shutdown_started = Instant::now();
        pipeline.shutdown().await;
        assert!(shutdown_started.elapsed() < Duration::from_secs(1));
        let health = pipeline.health().self_intake.unwrap();
        assert!(health.degraded);
        assert!(health.dropped_spans >= 1);
        assert_eq!(health.in_flight_spans, 0);
        assert_eq!(
            health.last_error.as_deref(),
            Some("shutdown timeout with unexported spans")
        );
    }
    fn tenant_candidate(trace_id: &str) -> TraceCandidate {
        let mut span = trace_fixtures::canonical_http_trace().remove(0);
        span.trace_id = trace_id.into();

        TraceCandidate {
            org_id: "org".into(),
            stream: Some("default".into()),
            span,
            force_keep: ForceKeep::None,
        }
    }

    async fn wait_for(mut done: impl FnMut() -> bool) {
        timeout(Duration::from_secs(3), async {
            while !done() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("condition reached in time");
    }

    #[tokio::test]
    async fn a_failing_store_holds_tenant_traces_until_it_recovers() {
        let sink = Arc::new(MemoryTraceSink::default());
        sink.set_fail(true);

        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(sink.clone()),
            None,
            fast_config(),
            TraceLimits::default(),
        )
        .unwrap();

        let trace_ids = [
            "0af7651916cd43dd8448eb211c80319c",
            "1af7651916cd43dd8448eb211c80319c",
            "2af7651916cd43dd8448eb211c80319c",
        ];

        for trace_id in trace_ids {
            pipeline
                .submit(tenant_candidate(trace_id), None)
                .await
                .unwrap();
        }

        wait_for(|| pipeline.health().self_intake.unwrap().retries >= 3).await;
        assert_eq!(pipeline.health().self_intake.unwrap().dropped_spans, 0);

        sink.set_fail(false);

        timeout(Duration::from_secs(3), async {
            while sink.traces("org").await.len() < trace_ids.len() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("every trace written once the store is back");

        let health = pipeline.health().self_intake.unwrap();
        pipeline.shutdown().await;

        assert_eq!(sink.traces("org").await.len(), trace_ids.len());
        assert_eq!(health.dropped_spans, 0);
        assert!(!health.degraded);
    }

    /// Writes the first trace of its first batch, then fails, leaving the rest unwritten.
    #[derive(Default)]
    struct HalfWritingSink {
        written: parking_lot::Mutex<Vec<String>>,
        failed: AtomicBool,
    }

    #[async_trait]
    impl TraceSink for HalfWritingSink {
        fn name(&self) -> &'static str {
            "self_intake"
        }

        async fn export(&self, traces: &mut Vec<DecidedTrace>) -> Result<(), String> {
            if !self.failed.swap(true, Ordering::AcqRel) {
                let first = traces.remove(0);
                self.written.lock().push(first.trace_id);
                return Err("written halfway".into());
            }

            self.written
                .lock()
                .extend(traces.iter().map(|trace| trace.trace_id.clone()));

            Ok(())
        }
    }

    #[tokio::test]
    async fn a_retry_writes_only_what_the_sink_left_unwritten() {
        let sink = Arc::new(HalfWritingSink::default());
        let mut config = fast_config();
        config.self_intake.batch_size = 8;
        config.self_intake.batch_delay = Duration::from_millis(30);

        let pipeline = TracePipeline::start(
            test_sampler(),
            Some(sink.clone()),
            None,
            config,
            TraceLimits::default(),
        )
        .unwrap();

        for trace_id in [
            "0bf7651916cd43dd8448eb211c80319c",
            "1bf7651916cd43dd8448eb211c80319c",
            "2bf7651916cd43dd8448eb211c80319c",
        ] {
            pipeline
                .submit(tenant_candidate(trace_id), None)
                .await
                .unwrap();
        }

        wait_for(|| sink.written.lock().len() >= 3).await;
        pipeline.shutdown().await;

        let mut written = sink.written.lock().clone();
        written.sort();
        let all = written.len();
        written.dedup();

        assert_eq!(all, 3, "every trace written exactly once: {written:?}");
        assert_eq!(written.len(), 3);
    }
}
