// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Single-owner candidate loop. Compact APM facts are extracted before the
//! sampler can discard a Trace, then admitted according to its deduplication
//! disposition.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

use super::{SinkEndpoint, TracePipelineConfig, fanout};
use crate::{
    app::apm::ApmCandidateProjector,
    shared::{
        tail_sampling::{TailSampler, TraceCandidate},
        trace_metrics,
    },
};

pub(super) struct CandidateOwnerRuntime {
    pub sampler: Arc<TailSampler>,
    pub apm_projector: Option<Arc<dyn ApmCandidateProjector>>,
    pub receiver: mpsc::Receiver<TraceCandidate>,
    pub depth: Arc<AtomicUsize>,
    pub accepting: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub config: TracePipelineConfig,
    pub self_endpoint: Option<SinkEndpoint>,
    pub external_endpoint: Option<SinkEndpoint>,
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
    } = runtime;
    tokio::spawn(async move {
        let mut tick =
            tokio::time::interval_at(Instant::now() + config.decision_tick, config.decision_tick);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {
                    fanout(
                        sampler.tick(),
                        self_endpoint.as_ref(),
                        external_endpoint.as_ref(),
                    );
                }
                candidate = receiver.recv() => {
                    let Some(candidate) = candidate else { break; };
                    decrement_depth(&depth, config.candidate_capacity);
                    accept_candidate(
                        sampler.as_ref(),
                        apm_projector.as_deref(),
                        candidate,
                        self_endpoint.as_ref(),
                        external_endpoint.as_ref(),
                    );
                }
            }
        }
        accepting.store(false, Ordering::Release);
        receiver.close();
        while let Ok(candidate) = receiver.try_recv() {
            decrement_depth(&depth, config.candidate_capacity);
            accept_candidate(
                sampler.as_ref(),
                apm_projector.as_deref(),
                candidate,
                self_endpoint.as_ref(),
                external_endpoint.as_ref(),
            );
        }
        fanout(
            sampler.flush(),
            self_endpoint.as_ref(),
            external_endpoint.as_ref(),
        );
        // Endpoint senders drop here; isolated sinks then drain and exit.
    })
}

fn accept_candidate(
    sampler: &TailSampler,
    apm_projector: Option<&dyn ApmCandidateProjector>,
    candidate: TraceCandidate,
    self_endpoint: Option<&SinkEndpoint>,
    external_endpoint: Option<&SinkEndpoint>,
) {
    let fact =
        apm_projector.and_then(|projector| projector.extract(&candidate.org_id, &candidate.span));
    let output = sampler.accept(candidate);
    if let (Some(projector), Some(fact), Some(disposition)) =
        (apm_projector, fact, output.disposition)
    {
        projector.project(fact, disposition);
    }
    fanout(output.decided, self_endpoint, external_endpoint);
}

fn decrement_depth(depth: &AtomicUsize, capacity: usize) {
    depth.fetch_sub(1, Ordering::Relaxed);
    trace_metrics::set_queue("candidate", depth.load(Ordering::Relaxed), capacity);
}
