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
use parking_lot::RwLock;
use serde::Serialize;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    app::ingestion::IngestService,
    domain::{
        ingestion::IngestBatch,
        storage::PhysicalDatasetKind,
        stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
    },
    shared::{
        ids::Id,
        self_telemetry::with_suppression,
        tail_sampling::{DecidedTrace, SamplerOutput, TailSampler, TraceCandidate},
        time::TimestampMicros,
        trace_metrics,
        trace_normalization::{TraceLimits, validate_sink_invariants},
    },
};

pub mod candidate_router;
pub mod export;
mod owner;
mod summary;

#[derive(Debug, Clone, Copy)]
pub struct TraceSinkWorkerConfig {
    pub queue_capacity: usize,
    pub batch_size: usize,
    pub batch_delay: Duration,
    pub export_timeout: Duration,
    pub max_attempts: usize,
    pub initial_backoff: Duration,
}

impl Default for TraceSinkWorkerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 8_192,
            batch_size: 256,
            batch_delay: Duration::from_secs(1),
            export_timeout: Duration::from_secs(5),
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
        }
    }
}

impl TraceSinkWorkerConfig {
    pub fn validate(self) -> Result<Self, String> {
        if self.queue_capacity == 0
            || self.batch_size == 0
            || self.batch_delay.is_zero()
            || self.export_timeout.is_zero()
            || self.max_attempts == 0
            || self.initial_backoff.is_zero()
        {
            return Err("Trace sink queue/batch/timeout/retry settings must be non-zero".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TracePipelineConfig {
    pub candidate_capacity: usize,
    pub decision_tick: Duration,
    pub shutdown_timeout: Duration,
    pub self_ingest: TraceSinkWorkerConfig,
    pub external: TraceSinkWorkerConfig,
}

impl Default for TracePipelineConfig {
    fn default() -> Self {
        Self {
            candidate_capacity: 16_384,
            decision_tick: Duration::from_millis(100),
            shutdown_timeout: Duration::from_secs(10),
            self_ingest: TraceSinkWorkerConfig::default(),
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
        self.self_ingest.validate()?;
        self.external.validate()?;
        Ok(self)
    }
}

#[async_trait]
pub trait TraceSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn export(&self, traces: &[DecidedTrace]) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct TraceSinkHealth {
    queue_depth: AtomicUsize,
    queue_capacity: AtomicUsize,
    queued_spans: AtomicUsize,
    in_flight_spans: AtomicUsize,
    exported_spans: AtomicU64,
    failed_batches: AtomicU64,
    dropped_spans: AtomicU64,
    retries: AtomicU64,
    degraded: AtomicBool,
    last_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceSinkHealthSnapshot {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub queued_spans: usize,
    pub in_flight_spans: usize,
    pub exported_spans: u64,
    pub failed_batches: u64,
    pub dropped_spans: u64,
    pub retries: u64,
    pub degraded: bool,
    pub last_error: Option<String>,
}

impl TraceSinkHealth {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            queue_capacity: AtomicUsize::new(capacity),
            ..Self::default()
        }
    }

    pub fn snapshot(&self) -> TraceSinkHealthSnapshot {
        TraceSinkHealthSnapshot {
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            queue_capacity: self.queue_capacity.load(Ordering::Relaxed),
            queued_spans: self.queued_spans.load(Ordering::Relaxed),
            in_flight_spans: self.in_flight_spans.load(Ordering::Relaxed),
            exported_spans: self.exported_spans.load(Ordering::Relaxed),
            failed_batches: self.failed_batches.load(Ordering::Relaxed),
            dropped_spans: self.dropped_spans.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            degraded: self.degraded.load(Ordering::Relaxed),
            last_error: self.last_error.read().clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TracePipelineHealthSnapshot {
    pub accepting: bool,
    pub candidate_queue_depth: usize,
    pub candidate_queue_capacity: usize,
    pub candidate_drops: u64,
    pub self_ingest: Option<TraceSinkHealthSnapshot>,
    pub external: Option<TraceSinkHealthSnapshot>,
}

pub(super) struct SinkEndpoint {
    sender: mpsc::Sender<DecidedTrace>,
    health: Arc<TraceSinkHealth>,
    name: &'static str,
    capacity: usize,
}

pub struct TracePipeline {
    candidate_sender: mpsc::Sender<TraceCandidate>,
    candidate_depth: Arc<AtomicUsize>,
    candidate_capacity: usize,
    candidate_drops: AtomicU64,
    accepting: Arc<AtomicBool>,
    self_ingest_health: Option<Arc<TraceSinkHealth>>,
    external_health: Option<Arc<TraceSinkHealth>>,
    cancel: CancellationToken,
    joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    shutdown_timeout: Duration,
    limits: TraceLimits,
}

impl TracePipeline {
    pub fn start(
        sampler: Arc<TailSampler>,
        self_ingest_sink: Option<Arc<dyn TraceSink>>,
        external_sink: Option<Arc<dyn TraceSink>>,
        config: TracePipelineConfig,
        limits: TraceLimits,
    ) -> Result<Arc<Self>, String> {
        Self::start_with_apm(
            sampler,
            None,
            self_ingest_sink,
            external_sink,
            config,
            limits,
        )
    }

    pub fn start_with_apm(
        sampler: Arc<TailSampler>,
        apm_projector: Option<Arc<dyn crate::app::apm::ApmCandidateProjector>>,
        self_ingest_sink: Option<Arc<dyn TraceSink>>,
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
        let (self_endpoint, self_health, self_join) =
            make_sink_worker(self_ingest_sink, config.self_ingest, limits);
        let (external_endpoint, external_health, external_join) =
            make_sink_worker(external_sink, config.external, limits);

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
            candidate_depth,
            candidate_capacity: config.candidate_capacity,
            candidate_drops: AtomicU64::new(0),
            accepting,
            self_ingest_health: self_health,
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
        crate::shared::trace_normalization::sanitize_and_limit_span(
            &mut candidate.span,
            self.limits,
        );
        self.candidate_depth.fetch_add(1, Ordering::Relaxed);
        trace_metrics::set_queue(
            "candidate",
            self.candidate_depth.load(Ordering::Relaxed),
            self.candidate_capacity,
        );
        match self.candidate_sender.try_send(candidate) {
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

    pub fn submit_output_for_test(&self, _output: SamplerOutput) {
        // Intentionally unavailable in production: all retained sets must originate at sampler.
    }

    pub fn health(&self) -> TracePipelineHealthSnapshot {
        TracePipelineHealthSnapshot {
            accepting: self.accepting.load(Ordering::Acquire),
            candidate_queue_depth: self.candidate_depth.load(Ordering::Relaxed),
            candidate_queue_capacity: self.candidate_capacity,
            candidate_drops: self.candidate_drops.load(Ordering::Relaxed),
            self_ingest: self
                .self_ingest_health
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
            let self_ingest_residue =
                record_shutdown_residue("self_ingest", self.self_ingest_health.as_ref());
            let external_residue =
                record_shutdown_residue("external", self.external_health.as_ref());
            tracing::warn!(
                target: "molesignal::app::trace",
                candidate_residue,
                self_ingest_residue,
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
}

fn make_sink_worker(
    sink: Option<Arc<dyn TraceSink>>,
    config: TraceSinkWorkerConfig,
    limits: TraceLimits,
) -> (
    Option<SinkEndpoint>,
    Option<Arc<TraceSinkHealth>>,
    Option<JoinHandle<()>>,
) {
    let Some(sink) = sink else {
        return (None, None, None);
    };
    let name = sink.name();
    let (sender, receiver) = mpsc::channel(config.queue_capacity);
    trace_metrics::set_queue(name, 0, config.queue_capacity);
    let health = Arc::new(TraceSinkHealth::with_capacity(config.queue_capacity));
    let join = tokio::spawn(run_sink_worker(
        sink,
        receiver,
        config,
        limits,
        health.clone(),
    ));
    (
        Some(SinkEndpoint {
            sender,
            health: health.clone(),
            name,
            capacity: config.queue_capacity,
        }),
        Some(health),
        Some(join),
    )
}

fn fanout(
    traces: Vec<DecidedTrace>,
    self_ingest: Option<&SinkEndpoint>,
    external: Option<&SinkEndpoint>,
) {
    for trace in traces.into_iter().filter(|trace| trace.kept) {
        enqueue_sink(self_ingest, trace.clone());
        let suppress_external = trace.spans.iter().any(|span| {
            span.attributes
                .get("molesignal.trace.suppress_external")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        });
        if !suppress_external {
            enqueue_sink(external, trace);
        }
    }
}

fn enqueue_sink(endpoint: Option<&SinkEndpoint>, trace: DecidedTrace) {
    let Some(endpoint) = endpoint else {
        return;
    };
    let spans = trace.spans.len() as u64;
    endpoint.health.queue_depth.fetch_add(1, Ordering::Relaxed);
    endpoint
        .health
        .queued_spans
        .fetch_add(spans as usize, Ordering::Relaxed);
    trace_metrics::set_queue(
        endpoint.name,
        endpoint.health.queue_depth.load(Ordering::Relaxed),
        endpoint.capacity,
    );
    match endpoint.sender.try_send(trace) {
        Ok(()) => {}
        Err(_) => {
            endpoint.health.queue_depth.fetch_sub(1, Ordering::Relaxed);
            endpoint
                .health
                .queued_spans
                .fetch_sub(spans as usize, Ordering::Relaxed);
            trace_metrics::set_queue(
                endpoint.name,
                endpoint.health.queue_depth.load(Ordering::Relaxed),
                endpoint.capacity,
            );
            endpoint
                .health
                .dropped_spans
                .fetch_add(spans, Ordering::Relaxed);
            trace_metrics::record_spans(endpoint.name, "queue_full", spans);
            endpoint.health.degraded.store(true, Ordering::Release);
            *endpoint.health.last_error.write() = Some("bounded sink queue exhausted".into());
        }
    }
}

async fn run_sink_worker(
    sink: Arc<dyn TraceSink>,
    mut receiver: mpsc::Receiver<DecidedTrace>,
    config: TraceSinkWorkerConfig,
    limits: TraceLimits,
    health: Arc<TraceSinkHealth>,
) {
    let sink_name = sink.name();
    loop {
        let first = receiver.recv().await;
        let Some(first) = first else {
            break;
        };
        health.queue_depth.fetch_sub(1, Ordering::Relaxed);
        health
            .queued_spans
            .fetch_sub(first.spans.len(), Ordering::Relaxed);
        trace_metrics::set_queue(
            sink_name,
            health.queue_depth.load(Ordering::Relaxed),
            config.queue_capacity,
        );
        let mut batch = vec![first];
        let deadline = sleep(config.batch_delay);
        tokio::pin!(deadline);
        while batch.len() < config.batch_size {
            tokio::select! {
                _ = &mut deadline => break,
                next = receiver.recv() => match next {
                    Some(trace) => {
                        health.queue_depth.fetch_sub(1, Ordering::Relaxed);
                        health
                            .queued_spans
                            .fetch_sub(trace.spans.len(), Ordering::Relaxed);
                        trace_metrics::set_queue(
                            sink_name,
                            health.queue_depth.load(Ordering::Relaxed),
                            config.queue_capacity,
                        );
                        batch.push(trace);
                    }
                    None => break,
                }
            }
        }

        let invalid = batch
            .iter()
            .flat_map(|trace| &trace.spans)
            .find_map(|span| {
                validate_sink_invariants(span, limits)
                    .err()
                    .map(|error| error.to_string())
            });
        if let Some(error) = invalid {
            let spans = span_count(&batch);
            health
                .dropped_spans
                .fetch_add(spans as u64, Ordering::Relaxed);
            health.failed_batches.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_export(sink_name, "invalid", spans as u64, Duration::ZERO);
            health.degraded.store(true, Ordering::Release);
            *health.last_error.write() = Some(format!("pre-sink invariant failed: {error}"));
            continue;
        }

        let spans = span_count(&batch);
        health.in_flight_spans.store(spans, Ordering::Relaxed);
        let mut last_error = None;
        let mut retry_reason = "export_failed";
        let export_started = Instant::now();
        for attempt in 0..config.max_attempts {
            if attempt > 0 {
                health.retries.fetch_add(1, Ordering::Relaxed);
                trace_metrics::record_retry(sink_name, retry_reason);
                let shift = u32::try_from(attempt - 1).unwrap_or(31).min(16);
                sleep(config.initial_backoff.saturating_mul(1_u32 << shift)).await;
            }
            match timeout(config.export_timeout, sink.export(&batch)).await {
                Ok(Ok(())) => {
                    health
                        .exported_spans
                        .fetch_add(spans as u64, Ordering::Relaxed);
                    health.degraded.store(false, Ordering::Release);
                    *health.last_error.write() = None;
                    trace_metrics::record_export(
                        sink_name,
                        "exported",
                        spans as u64,
                        export_started.elapsed(),
                    );
                    last_error = None;
                    break;
                }
                Ok(Err(error)) => {
                    retry_reason = "export_failed";
                    last_error = Some(error);
                }
                Err(_) => {
                    retry_reason = "timeout";
                    last_error = Some("export timeout".into());
                }
            }
        }
        if let Some(error) = last_error {
            health.failed_batches.fetch_add(1, Ordering::Relaxed);
            health
                .dropped_spans
                .fetch_add(spans as u64, Ordering::Relaxed);
            trace_metrics::record_export(
                sink_name,
                "failed",
                spans as u64,
                export_started.elapsed(),
            );
            health.degraded.store(true, Ordering::Release);
            *health.last_error.write() = Some(error.clone());
            tracing::warn!(
                target: "molesignal::app::trace",
                sink = sink.name(),
                error = %error,
                spans,
                "Trace sink batch dropped after bounded retries"
            );
        }
        health.in_flight_spans.store(0, Ordering::Relaxed);
    }
}

fn record_shutdown_residue(
    sink_name: &'static str,
    health: Option<&Arc<TraceSinkHealth>>,
) -> usize {
    let Some(health) = health else {
        return 0;
    };
    let queued_spans = health.queued_spans.swap(0, Ordering::Relaxed);
    let in_flight_spans = health.in_flight_spans.swap(0, Ordering::Relaxed);
    let residue = queued_spans.saturating_add(in_flight_spans);
    health.queue_depth.store(0, Ordering::Relaxed);
    trace_metrics::set_queue(sink_name, 0, health.queue_capacity.load(Ordering::Relaxed));
    if residue > 0 {
        health
            .dropped_spans
            .fetch_add(residue as u64, Ordering::Relaxed);
        health.failed_batches.fetch_add(1, Ordering::Relaxed);
        health.degraded.store(true, Ordering::Release);
        *health.last_error.write() = Some("shutdown timeout with unexported spans".into());
        trace_metrics::record_export(
            sink_name,
            "shutdown_timeout",
            residue as u64,
            Duration::ZERO,
        );
    }
    residue
}

fn span_count(traces: &[DecidedTrace]) -> usize {
    traces.iter().map(|trace| trace.spans.len()).sum()
}

pub struct SelfIngestTraceSink {
    ingestion: Arc<IngestService>,
    system_org_id: Id,
}

impl SelfIngestTraceSink {
    pub fn new(ingestion: Arc<IngestService>, system_org_id: Id) -> Self {
        Self {
            ingestion,
            system_org_id,
        }
    }
}

#[async_trait]
impl TraceSink for SelfIngestTraceSink {
    fn name(&self) -> &'static str {
        "self_ingest"
    }

    async fn export(&self, traces: &[DecidedTrace]) -> Result<(), String> {
        let mut groups: BTreeMap<(String, String, bool), (Vec<_>, Vec<_>)> = BTreeMap::new();
        for trace in traces {
            let (org_id, stream, internal) = match &trace.stream {
                Some(stream) => (trace.org_id.clone(), stream.clone(), false),
                None => (
                    self.system_org_id.0.clone(),
                    MOLESIGNAL_SYSTEM_STREAM.into(),
                    true,
                ),
            };
            let group = groups.entry((org_id, stream, internal)).or_default();
            group.0.extend(summary::span_events(trace));
            group.1.extend(summary::summary_event(trace));
        }
        for ((org_id, stream, internal), (spans, summaries)) in groups {
            if spans.is_empty() {
                continue;
            }
            let batch = IngestBatch {
                batch_id: Id::new(),
                org_id: Id(org_id.clone()),
                stream: stream.clone(),
                stream_type: StreamType::Traces,
                events: spans,
                received_at: TimestampMicros::now(),
            };
            let result = if internal {
                with_suppression(self.ingestion.ingest_self_telemetry(batch)).await
            } else {
                with_suppression(self.ingestion.ingest(batch)).await
            }
            .map_err(|error| error.to_string())?;
            if result.rejected != 0 {
                return Err(format!("Trace storage rejected {} spans", result.rejected));
            }
            if summaries.is_empty() {
                continue;
            }
            let summary_batch = IngestBatch {
                batch_id: Id::new(),
                org_id: Id(org_id),
                stream,
                stream_type: StreamType::Traces,
                events: summaries,
                received_at: TimestampMicros::now(),
            };
            let result = if internal {
                with_suppression(self.ingestion.ingest_self_telemetry_dataset(
                    summary_batch,
                    PhysicalDatasetKind::TraceSummary,
                ))
                .await
            } else {
                with_suppression(
                    self.ingestion
                        .ingest_derived_dataset(summary_batch, PhysicalDatasetKind::TraceSummary),
                )
                .await
            }
            .map_err(|error| error.to_string())?;
            if result.rejected != 0 {
                return Err(format!(
                    "Trace summary storage rejected {} rows",
                    result.rejected
                ));
            }
        }
        Ok(())
    }
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

    async fn export(&self, traces: &[DecidedTrace]) -> Result<(), String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("injected failure".into());
        }
        let mut stored = self.traces.lock().await;
        for trace in traces {
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
    use super::*;
    use crate::{
        app::apm::ApmCandidateProjector,
        domain::apm::ApmSpanFact,
        shared::{
            tail_sampling::{CandidateDisposition, ForceKeep, TraceRuntimePolicy},
            trace_fixtures,
            trace_normalization::CanonicalSpan,
        },
    };

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
            "self_ingest"
        }

        async fn export(&self, _traces: &[DecidedTrace]) -> Result<(), String> {
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
            self_ingest: sink,
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
        assert!(health.self_ingest.unwrap().degraded);
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
        config.self_ingest.export_timeout = Duration::from_secs(10);
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
        let health = pipeline.health().self_ingest.unwrap();
        assert!(health.degraded);
        assert!(health.dropped_spans >= 1);
        assert_eq!(health.in_flight_spans, 0);
        assert_eq!(
            health.last_error.as_deref(),
            Some("shutdown timeout with unexported spans")
        );
    }
}
