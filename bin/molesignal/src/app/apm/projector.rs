// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Non-blocking bridge from the Trace candidate owner to the APM worker.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use super::{extract_apm_fact, metrics};
use crate::{
    config::ApmSettings,
    domain::apm::{ApmSpanFact, ApmWriteRepository, HistogramSchema, ProjectionGapReason},
    shared::{
        tail_sampling::CandidateDisposition, time::TimestampMicros,
        trace_normalization::CanonicalSpan,
    },
};

mod gaps;
mod worker;

use gaps::GapLedger;
use worker::{ProjectionCommand, run_worker};

pub trait ApmCandidateProjector: Send + Sync {
    fn extract(&self, org_id: &str, span: &CanonicalSpan) -> Option<ApmSpanFact>;
    fn project(&self, fact: ApmSpanFact, disposition: CandidateDisposition);
}

#[derive(Debug, Clone)]
pub struct ApmProjectorConfig {
    pub queue_capacity: usize,
    pub flush_interval: Duration,
    pub flush_max_snapshots: usize,
    pub shutdown_timeout: Duration,
    pub late_grace: Duration,
    pub max_exemplars_per_bucket: usize,
    pub max_error_samples_per_group: usize,
    pub histogram: HistogramSchema,
    pub cardinality: crate::config::ApmCardinalitySettings,
}

impl ApmProjectorConfig {
    pub fn from_settings(settings: &ApmSettings) -> Self {
        let mut upper_bounds_micros = settings
            .histogram
            .boundaries_ms
            .iter()
            .map(|value| value.saturating_mul(1_000))
            .collect::<Vec<_>>();
        upper_bounds_micros.push(u64::MAX);
        Self {
            queue_capacity: settings.queue_capacity,
            flush_interval: Duration::from_millis(settings.flush_interval_ms),
            flush_max_snapshots: settings.flush_max_snapshots,
            shutdown_timeout: Duration::from_secs(settings.shutdown_drain_secs),
            late_grace: Duration::from_secs(settings.late_grace_secs),
            max_exemplars_per_bucket: settings.max_exemplars_per_bucket,
            max_error_samples_per_group: settings.max_error_samples_per_group,
            histogram: HistogramSchema {
                version: settings.histogram.schema_version,
                upper_bounds_micros,
            },
            cardinality: settings.cardinality.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct ApmProjectorHealth {
    accepting: AtomicBool,
    queue_depth: AtomicUsize,
    accepted_facts: AtomicU64,
    duplicate_skips: AtomicU64,
    late_facts: AtomicU64,
    queue_drops: AtomicU64,
    cardinality_rejections: AtomicU64,
    flush_successes: AtomicU64,
    flush_failures: AtomicU64,
    pending_snapshots: AtomicUsize,
    last_success_at_micros: AtomicU64,
    latest_event_at_micros: AtomicU64,
    degraded: AtomicBool,
    last_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmProjectorHealthSnapshot {
    pub accepting: bool,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub accepted_facts: u64,
    pub duplicate_skips: u64,
    pub late_facts: u64,
    pub queue_drops: u64,
    pub cardinality_rejections: u64,
    pub flush_successes: u64,
    pub flush_failures: u64,
    pub pending_snapshots: usize,
    pub last_success_at: Option<TimestampMicros>,
    pub latest_event_at: Option<TimestampMicros>,
    pub degraded: bool,
    pub last_error: Option<String>,
}

pub struct BufferedApmProjector {
    sender: mpsc::Sender<ProjectionCommand>,
    capacity: usize,
    health: Arc<ApmProjectorHealth>,
    gaps: Arc<GapLedger>,
    cancel: CancellationToken,
    join: Mutex<Option<JoinHandle<()>>>,
    shutdown_timeout: Duration,
}

impl BufferedApmProjector {
    pub fn start(
        owner_id: String,
        repository: Arc<dyn ApmWriteRepository>,
        config: ApmProjectorConfig,
    ) -> crate::shared::Result<Arc<Self>> {
        config.histogram.validate()?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let health = Arc::new(ApmProjectorHealth::default());
        health.accepting.store(true, Ordering::Release);
        let gaps = Arc::new(GapLedger::new(4_096));
        let cancel = CancellationToken::new();
        metrics::set_queue(0, config.queue_capacity);
        metrics::set_health("projector", true);
        metrics::set_health("repository", true);
        let join = tokio::spawn(run_worker(
            owner_id,
            repository,
            config.clone(),
            receiver,
            health.clone(),
            gaps.clone(),
            cancel.clone(),
        ));
        Ok(Arc::new(Self {
            sender,
            capacity: config.queue_capacity,
            health,
            gaps,
            cancel,
            join: Mutex::new(Some(join)),
            shutdown_timeout: config.shutdown_timeout,
        }))
    }

    pub fn health(&self) -> ApmProjectorHealthSnapshot {
        let timestamp = |value: u64| {
            (value != 0).then(|| TimestampMicros(i64::try_from(value).unwrap_or(i64::MAX)))
        };
        ApmProjectorHealthSnapshot {
            accepting: self.health.accepting.load(Ordering::Acquire),
            queue_depth: self.health.queue_depth.load(Ordering::Relaxed),
            queue_capacity: self.capacity,
            accepted_facts: self.health.accepted_facts.load(Ordering::Relaxed),
            duplicate_skips: self.health.duplicate_skips.load(Ordering::Relaxed),
            late_facts: self.health.late_facts.load(Ordering::Relaxed),
            queue_drops: self.health.queue_drops.load(Ordering::Relaxed),
            cardinality_rejections: self.health.cardinality_rejections.load(Ordering::Relaxed),
            flush_successes: self.health.flush_successes.load(Ordering::Relaxed),
            flush_failures: self.health.flush_failures.load(Ordering::Relaxed),
            pending_snapshots: self.health.pending_snapshots.load(Ordering::Relaxed),
            last_success_at: timestamp(self.health.last_success_at_micros.load(Ordering::Relaxed)),
            latest_event_at: timestamp(self.health.latest_event_at_micros.load(Ordering::Relaxed)),
            degraded: self.health.degraded.load(Ordering::Acquire),
            last_error: self.health.last_error.read().clone(),
        }
    }

    pub async fn shutdown(&self) {
        if !self.health.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        self.cancel.cancel();
        let Some(mut join) = self.join.lock().await.take() else {
            return;
        };
        if timeout(self.shutdown_timeout, &mut join).await.is_err() {
            join.abort();
            let residue = self.health.queue_depth.swap(0, Ordering::Relaxed);
            self.health
                .queue_drops
                .fetch_add(residue as u64, Ordering::Relaxed);
            self.health.degraded.store(true, Ordering::Release);
            *self.health.last_error.write() = Some("projector shutdown timed out".into());
            self.gaps.record_now(
                None,
                ProjectionGapReason::ShutdownTimeout,
                residue.max(1) as u64,
            );
            metrics::set_queue(0, self.capacity);
            metrics::set_health("projector", false);
            tracing::warn!(
                target: "molesignal::app::apm",
                residue,
                "APM projector shutdown timed out"
            );
        }
    }
}

impl ApmCandidateProjector for BufferedApmProjector {
    fn extract(&self, org_id: &str, span: &CanonicalSpan) -> Option<ApmSpanFact> {
        let fact = extract_apm_fact(org_id, span);
        if fact.is_none() {
            metrics::record_fact("extract_failed");
        }
        fact
    }

    fn project(&self, fact: ApmSpanFact, disposition: CandidateDisposition) {
        let trace_available = match disposition {
            CandidateDisposition::Accepted => false,
            CandidateDisposition::LateKept => {
                self.health.late_facts.fetch_add(1, Ordering::Relaxed);
                metrics::record_fact("late_kept");
                true
            }
            CandidateDisposition::LateDropped => {
                self.health.late_facts.fetch_add(1, Ordering::Relaxed);
                metrics::record_fact("late_dropped");
                false
            }
            CandidateDisposition::IdenticalDuplicate
            | CandidateDisposition::ConflictingDuplicate => {
                self.health.duplicate_skips.fetch_add(1, Ordering::Relaxed);
                metrics::record_fact("duplicate_skip");
                return;
            }
        };
        if !self.health.accepting.load(Ordering::Acquire) {
            self.health.queue_drops.fetch_add(1, Ordering::Relaxed);
            self.gaps.record(
                &fact.org_id,
                fact.event_time,
                ProjectionGapReason::QueueFull,
                1,
            );
            metrics::record_fact("stopped");
            return;
        }
        self.health.queue_depth.fetch_add(1, Ordering::Relaxed);
        metrics::set_queue(
            self.health.queue_depth.load(Ordering::Relaxed),
            self.capacity,
        );
        match self.sender.try_send(ProjectionCommand {
            fact,
            trace_available,
        }) {
            Ok(()) => {
                self.health.accepted_facts.fetch_add(1, Ordering::Relaxed);
                metrics::record_fact("accepted");
            }
            Err(mpsc::error::TrySendError::Full(command))
            | Err(mpsc::error::TrySendError::Closed(command)) => {
                self.health.queue_depth.fetch_sub(1, Ordering::Relaxed);
                self.health.queue_drops.fetch_add(1, Ordering::Relaxed);
                self.gaps.record(
                    &command.fact.org_id,
                    command.fact.event_time,
                    ProjectionGapReason::QueueFull,
                    1,
                );
                metrics::set_queue(
                    self.health.queue_depth.load(Ordering::Relaxed),
                    self.capacity,
                );
                metrics::record_fact("queue_full");
            }
        }
    }
}

#[cfg(test)]
mod tests;
