// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The queue and worker in front of each Trace sink.
//!
//! A lossless sink (the deployment's own storage) never drops a tenant's kept trace for lack of
//! room or a failed write: the owner waits for queue room, and the worker retries the batch until
//! it is written, so overload and storage outages push back on the router and from there on the
//! senders, which are refused before anything is taken. The process's own traces, and every
//! trace bound for a best-effort sink (an external OTLP copy), are still dropped and counted when
//! their queue is full or their retries run out, so neither can hold tenant data back.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use parking_lot::RwLock;
use serde::Serialize;
use signals::{
    tail_sampling::DecidedTrace,
    trace_metrics,
    trace_normalization::{TraceLimits, validate_sink_invariants},
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};

use super::{
    SettlementSlot, TraceSink,
    durable::{SpanOrigin, SpanSettlement},
};

/// A decided trace on its way to a sink, with the candidate log places of its spans.
type QueuedTrace = (DecidedTrace, Vec<SpanOrigin>);

/// The longest wait between two attempts at a batch a lossless sink keeps retrying.
const MAX_LOSSLESS_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
pub struct TraceSinkWorkerConfig {
    pub queue_capacity: usize,
    pub batch_size: usize,
    pub batch_delay: Duration,
    pub export_timeout: Duration,
    /// Attempts at one batch before a best-effort sink drops it. A lossless sink keeps trying.
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
#[serde(rename_all = "camelCase")]
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

    fn fail(&self, error: impl Into<String>) {
        self.degraded.store(true, Ordering::Release);
        *self.last_error.write() = Some(error.into());
    }
}

/// Whether a sink may drop what it was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Delivery {
    /// Tenant traces wait for room and are retried until written.
    Lossless,
    /// Everything is dropped and counted when the queue is full or the retries run out.
    BestEffort,
}

pub(super) struct SinkEndpoint {
    sink: Arc<dyn TraceSink>,
    sender: mpsc::Sender<QueuedTrace>,
    health: Arc<TraceSinkHealth>,
    name: &'static str,
    capacity: usize,
    delivery: Delivery,
}

pub(super) fn make_sink_worker(
    sink: Option<Arc<dyn TraceSink>>,
    config: TraceSinkWorkerConfig,
    limits: TraceLimits,
    delivery: Delivery,
    settlement: SettlementSlot,
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

    let worker = SinkWorker {
        sink: sink.clone(),
        config,
        limits,
        health: health.clone(),
        delivery,
        settlement,
    };

    let join = tokio::spawn(worker.run(receiver));

    (
        Some(SinkEndpoint {
            sink,
            sender,
            health: health.clone(),
            name,
            capacity: config.queue_capacity,
            delivery,
        }),
        Some(health),
        Some(join),
    )
}

/// Hands every kept trace to the sinks that want it. A lossless sink makes this wait for room
/// when the trace is a tenant's.
///
/// The log places of a trace's spans go with it to the storage sink, which settles them once the
/// write is acknowledged. They are settled here when nothing will be written: the trace was
/// sampled out, or no storage sink takes it.
pub(super) async fn fanout(
    traces: Vec<QueuedTrace>,
    self_intake: Option<&SinkEndpoint>,
    external: Option<&SinkEndpoint>,
    settlement: Option<&dyn SpanSettlement>,
) {
    for (trace, origins) in traces {
        if !trace.kept {
            settle(settlement, &origins);
            continue;
        }

        let suppress_external = trace.spans.iter().any(|span| {
            span.attributes
                .get("molesignal.trace.suppress_external")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        });

        if !suppress_external {
            enqueue_sink(external, (trace.clone(), Vec::new())).await;
        }

        let stored = self_intake.is_some_and(|endpoint| endpoint.sink.takes(&trace));

        if stored {
            enqueue_sink(self_intake, (trace, origins)).await;
        } else {
            settle(settlement, &origins);
        }
    }
}

fn settle(settlement: Option<&dyn SpanSettlement>, origins: &[SpanOrigin]) {
    if let Some(settlement) = settlement.filter(|_| !origins.is_empty()) {
        settlement.settle(origins);
    }
}

/// Queues one trace. A trace that cannot be queued is dropped and counted, its log places left
/// unsettled: that happens only to a best-effort trace, or when the sink is shutting down, and
/// then the candidate log replays it.
async fn enqueue_sink(endpoint: Option<&SinkEndpoint>, queued: QueuedTrace) {
    let Some(endpoint) = endpoint else {
        return;
    };

    let trace = &queued.0;

    if !endpoint.sink.takes(trace) {
        return;
    }

    let spans = trace.spans.len();
    let waits = endpoint.delivery == Delivery::Lossless && trace.stream.is_some();

    // Counted before the send: the worker may take the trace before `send` returns.
    endpoint.health.queue_depth.fetch_add(1, Ordering::Relaxed);
    endpoint
        .health
        .queued_spans
        .fetch_add(spans, Ordering::Relaxed);
    trace_metrics::set_queue(
        endpoint.name,
        endpoint.health.queue_depth.load(Ordering::Relaxed),
        endpoint.capacity,
    );

    let sent = if waits {
        endpoint.sender.send(queued).await.is_ok()
    } else {
        endpoint.sender.try_send(queued).is_ok()
    };

    if sent {
        return;
    }

    endpoint.health.queue_depth.fetch_sub(1, Ordering::Relaxed);
    endpoint
        .health
        .queued_spans
        .fetch_sub(spans, Ordering::Relaxed);
    trace_metrics::set_queue(
        endpoint.name,
        endpoint.health.queue_depth.load(Ordering::Relaxed),
        endpoint.capacity,
    );

    endpoint
        .health
        .dropped_spans
        .fetch_add(spans as u64, Ordering::Relaxed);

    trace_metrics::record_spans(endpoint.name, "queue_full", spans as u64);
    endpoint.health.fail("bounded sink queue exhausted");
}

struct SinkWorker {
    sink: Arc<dyn TraceSink>,
    config: TraceSinkWorkerConfig,
    limits: TraceLimits,
    health: Arc<TraceSinkHealth>,
    delivery: Delivery,
    settlement: SettlementSlot,
}

impl SinkWorker {
    async fn run(self, mut receiver: mpsc::Receiver<QueuedTrace>) {
        while let Some((first, mut origins)) = receiver.recv().await {
            self.dequeued(&first);
            let mut batch = vec![first];
            let deadline = sleep(self.config.batch_delay);
            tokio::pin!(deadline);

            while batch.len() < self.config.batch_size {
                tokio::select! {
                    _ = &mut deadline => break,
                    next = receiver.recv() => match next {
                        Some((trace, more)) => {
                            self.dequeued(&trace);
                            batch.push(trace);
                            origins.extend(more);
                        }
                        None => break,
                    }
                }
            }

            self.write(batch).await;

            // Written, or refused for good: either way nothing of the batch needs a replay. A
            // batch still unwritten at shutdown never gets here, and is replayed.
            if let Some(settlement) = self.settlement.get().filter(|_| !origins.is_empty()) {
                settlement.settle(&origins);
            }
        }
    }

    fn dequeued(&self, trace: &DecidedTrace) {
        self.health.queue_depth.fetch_sub(1, Ordering::Relaxed);

        self.health
            .queued_spans
            .fetch_sub(trace.spans.len(), Ordering::Relaxed);

        trace_metrics::set_queue(
            self.sink.name(),
            self.health.queue_depth.load(Ordering::Relaxed),
            self.config.queue_capacity,
        );
    }

    /// Writes one batch. The sink leaves in `pending` what it has not written yet, so a retry
    /// never writes a trace twice.
    async fn write(&self, mut pending: Vec<DecidedTrace>) {
        let sink_name = self.sink.name();

        let invalid = pending
            .iter()
            .flat_map(|trace| &trace.spans)
            .find_map(|span| {
                validate_sink_invariants(span, self.limits)
                    .err()
                    .map(|error| error.to_string())
            });

        // A span the sink can never take would hold the batch forever: it is refused for good.
        if let Some(error) = invalid {
            let spans = span_count(&pending);

            self.health
                .dropped_spans
                .fetch_add(spans as u64, Ordering::Relaxed);

            self.health.failed_batches.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_export(sink_name, "invalid", spans as u64, Duration::ZERO);
            self.health
                .fail(format!("pre-sink invariant failed: {error}"));
            return;
        }

        let spans = span_count(&pending);
        self.health.in_flight_spans.store(spans, Ordering::Relaxed);
        let export_started = Instant::now();
        let mut attempt = 0_usize;

        loop {
            let error =
                match timeout(self.config.export_timeout, self.sink.export(&mut pending)).await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(("export_failed", error)),
                    Err(_) => Some(("timeout", "export timeout".to_string())),
                };

            let Some((reason, error)) = error else {
                self.health
                    .exported_spans
                    .fetch_add(spans as u64, Ordering::Relaxed);

                self.health.degraded.store(false, Ordering::Release);
                *self.health.last_error.write() = None;
                trace_metrics::record_export(
                    sink_name,
                    "exported",
                    spans as u64,
                    export_started.elapsed(),
                );
                break;
            };

            attempt += 1;
            self.health
                .in_flight_spans
                .store(span_count(&pending), Ordering::Relaxed);
            self.health.fail(error.clone());

            // A lossless sink gives up only on the process's own traces: once nothing of a
            // tenant's is left, the batch is best effort like any other.
            let gives_up = match self.delivery {
                Delivery::BestEffort => true,
                Delivery::Lossless => pending.iter().all(|trace| trace.stream.is_none()),
            };

            if gives_up && attempt >= self.config.max_attempts {
                self.dropped_after_retries(&pending, &error, export_started);
                break;
            }

            if attempt == 1 || attempt.is_power_of_two() {
                tracing::warn!(
                    target: "molesignal::app::trace",
                    sink = sink_name,
                    error = %error,
                    attempt,
                    spans = span_count(&pending),
                    "Trace sink write failed; retrying"
                );
            }

            self.health.retries.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_retry(sink_name, reason);
            sleep(self.backoff(attempt)).await;
        }

        self.health.in_flight_spans.store(0, Ordering::Relaxed);
    }

    fn backoff(&self, attempt: usize) -> Duration {
        let shift = u32::try_from(attempt - 1).unwrap_or(31).min(16);
        let backoff = self.config.initial_backoff.saturating_mul(1_u32 << shift);

        match self.delivery {
            Delivery::Lossless => backoff.min(MAX_LOSSLESS_BACKOFF),
            Delivery::BestEffort => backoff,
        }
    }

    fn dropped_after_retries(&self, pending: &[DecidedTrace], error: &str, started: Instant) {
        let spans = span_count(pending);
        self.health.failed_batches.fetch_add(1, Ordering::Relaxed);

        self.health
            .dropped_spans
            .fetch_add(spans as u64, Ordering::Relaxed);

        trace_metrics::record_export(self.sink.name(), "failed", spans as u64, started.elapsed());
        tracing::warn!(
            target: "molesignal::app::trace",
            sink = self.sink.name(),
            error = %error,
            spans,
            "Trace sink batch dropped after bounded retries"
        );
    }
}

pub(super) fn record_shutdown_residue(
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
        health.fail("shutdown timeout with unexported spans");

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
