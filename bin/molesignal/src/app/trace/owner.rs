// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Single-owner candidate loop. Compact APM facts are extracted before the
//! sampler can discard a Trace, then admitted according to its deduplication
//! disposition.
//!
//! The loop also follows logged spans to their fate: a span's place in the candidate log is held
//! with its trace until the trace is decided, then settled at once when the trace is sampled out
//! (or the span was a duplicate), or handed to the storage sink with the trace, which settles it
//! once the write is acknowledged.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use signals::{
    tail_sampling::{CandidateDisposition, DecidedTrace, TailSampler},
    trace_metrics,
};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

use super::{
    QueuedSpan, SettlementSlot, SinkEndpoint, TracePipelineConfig, durable::SpanOrigin, fanout,
};
use crate::app::apm::ApmCandidateProjector;

pub(super) struct CandidateOwnerRuntime {
    pub sampler: Arc<TailSampler>,
    pub apm_projector: Option<Arc<dyn ApmCandidateProjector>>,
    pub receiver: mpsc::Receiver<QueuedSpan>,
    pub depth: Arc<AtomicUsize>,
    pub accepting: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub config: TracePipelineConfig,
    pub self_endpoint: Option<SinkEndpoint>,
    pub external_endpoint: Option<SinkEndpoint>,
    pub settlement: SettlementSlot,
}

pub(super) fn spawn_candidate_owner(runtime: CandidateOwnerRuntime) -> JoinHandle<()> {
    let CandidateOwnerRuntime {
        sampler,
        apm_projector,
        mut receiver,
        depth,
        accepting,
        cancel,
        config,
        self_endpoint,
        external_endpoint,
        settlement,
    } = runtime;

    let mut owner = Owner {
        sampler,
        apm_projector,
        self_endpoint,
        external_endpoint,
        settlement,
        origins: HashMap::new(),
    };

    tokio::spawn(async move {
        let mut tick =
            tokio::time::interval_at(Instant::now() + config.decision_tick, config.decision_tick);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {
                    let decided = owner.sampler.tick();
                    owner.hand_on(decided).await;
                }
                candidate = receiver.recv() => {
                    let Some((candidate, origin)) = candidate else { break; };
                    decrement_depth(&depth, config.candidate_capacity);
                    owner.accept(candidate, origin).await;
                }
            }
        }

        accepting.store(false, Ordering::Release);
        receiver.close();

        while let Ok((candidate, origin)) = receiver.try_recv() {
            decrement_depth(&depth, config.candidate_capacity);
            owner.accept(candidate, origin).await;
        }

        let decided = owner.sampler.flush();
        owner.hand_on(decided).await;
        // Endpoint senders drop here; isolated sinks then drain and exit.
    })
}

struct Owner {
    sampler: Arc<TailSampler>,
    apm_projector: Option<Arc<dyn ApmCandidateProjector>>,
    self_endpoint: Option<SinkEndpoint>,
    external_endpoint: Option<SinkEndpoint>,
    settlement: SettlementSlot,
    /// Log places of the spans of traces not decided yet, by `(org, trace id)`.
    origins: HashMap<(String, String), Vec<SpanOrigin>>,
}

impl Owner {
    /// Feeds one candidate to the sampler and hands on what it decided. Waiting here for room in
    /// a lossless sink's queue is what stops the loop taking more candidates while storage lags.
    async fn accept(
        &mut self,
        candidate: signals::tail_sampling::TraceCandidate,
        origin: Option<SpanOrigin>,
    ) {
        let key = (candidate.org_id.clone(), candidate.span.trace_id.clone());

        let fact = self
            .apm_projector
            .as_deref()
            .and_then(|projector| projector.extract(&candidate.org_id, &candidate.span));

        let output = self.sampler.accept(candidate);

        if let (Some(projector), Some(fact), Some(disposition)) =
            (self.apm_projector.as_deref(), fact, output.disposition)
        {
            projector.project(fact, disposition);
        }

        if let Some(origin) = origin {
            match output.disposition {
                // The span joined its trace: it settles with the trace.
                None | Some(CandidateDisposition::Accepted | CandidateDisposition::LateKept) => {
                    self.origins.entry(key).or_default().push(origin);
                }
                // Already held, or dropped by policy: nothing of it is left to write.
                Some(_) => self.settle(&[origin]),
            }
        }

        self.hand_on(output.decided).await;
    }

    /// Hands decided traces to the sinks, each with the log places of its spans.
    async fn hand_on(&mut self, decided: Vec<DecidedTrace>) {
        if decided.is_empty() {
            return;
        }

        let traces = decided
            .into_iter()
            .map(|trace| {
                let key = (trace.org_id.clone(), trace.trace_id.clone());
                let origins = self.origins.remove(&key).unwrap_or_default();
                (trace, origins)
            })
            .collect();

        let settlement = self.settlement.get().cloned();

        fanout(
            traces,
            self.self_endpoint.as_ref(),
            self.external_endpoint.as_ref(),
            settlement.as_deref(),
        )
        .await;
    }

    fn settle(&self, origins: &[SpanOrigin]) {
        if let Some(settlement) = self.settlement.get() {
            settlement.settle(origins);
        }
    }
}

fn decrement_depth(depth: &AtomicUsize, capacity: usize) {
    depth.fetch_sub(1, Ordering::Relaxed);
    trace_metrics::set_queue("candidate", depth.load(Ordering::Relaxed), capacity);
}
