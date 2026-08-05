// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{ApmProjectorConfig, ApmProjectorHealth, GapLedger};
use crate::{
    app::apm::{ApmAggregator, ApmCardinalityLimiter, metrics},
    domain::apm::{ApmSpanFact, ApmWriteRepository, ProjectionGapReason},
    shared::time::TimestampMicros,
};

pub(super) struct ProjectionCommand {
    pub(super) fact: ApmSpanFact,
    pub(super) trace_available: bool,
}

pub(super) async fn run_worker(
    owner_id: String,
    repository: Arc<dyn ApmWriteRepository>,
    config: ApmProjectorConfig,
    mut receiver: mpsc::Receiver<ProjectionCommand>,
    health: Arc<ApmProjectorHealth>,
    gaps: Arc<GapLedger>,
    cancel: CancellationToken,
) {
    let mut aggregator = match ApmAggregator::new(
        owner_id,
        config.histogram.clone(),
        config.max_exemplars_per_bucket,
        config.max_error_samples_per_group,
    ) {
        Ok(aggregator) => aggregator,
        Err(_) => {
            health.degraded.store(true, Ordering::Release);
            *health.last_error.write() = Some("invalid histogram schema".into());
            metrics::set_health("projector", false);
            return;
        }
    };
    let mut limiter = ApmCardinalityLimiter::new(config.cardinality.clone());
    let mut tick = tokio::time::interval(config.flush_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                decrement_queue(&health, config.queue_capacity);
                observe(
                    command,
                    &mut limiter,
                    &mut aggregator,
                    &health,
                    &gaps,
                    config.late_grace,
                );
            }
            _ = tick.tick() => {
                flush(
                    repository.as_ref(),
                    &mut aggregator,
                    &health,
                    &gaps,
                    &config,
                ).await;
            }
        }
    }

    health.accepting.store(false, Ordering::Release);
    receiver.close();
    while let Ok(command) = receiver.try_recv() {
        decrement_queue(&health, config.queue_capacity);
        observe(
            command,
            &mut limiter,
            &mut aggregator,
            &health,
            &gaps,
            config.late_grace,
        );
    }
    loop {
        let pending_before = aggregator.pending_snapshot_count();
        flush(
            repository.as_ref(),
            &mut aggregator,
            &health,
            &gaps,
            &config,
        )
        .await;
        let pending_after = aggregator.pending_snapshot_count();
        if pending_after == 0 || pending_after >= pending_before {
            break;
        }
    }
    metrics::set_queue(0, config.queue_capacity);
}

fn observe(
    command: ProjectionCommand,
    limiter: &mut ApmCardinalityLimiter,
    aggregator: &mut ApmAggregator,
    health: &ApmProjectorHealth,
    gaps: &GapLedger,
    late_grace: std::time::Duration,
) {
    let now = TimestampMicros::now();
    let late_cutoff = now
        .0
        .saturating_sub(i64::try_from(late_grace.as_micros()).unwrap_or(i64::MAX));
    if command.fact.event_time.0 < late_cutoff {
        gaps.record(
            &command.fact.org_id,
            command.fact.event_time,
            ProjectionGapReason::LateDropped,
            1,
        );
        metrics::record_fact("late_dropped");
        return;
    }
    let event_micros = u64::try_from(command.fact.event_time.0).unwrap_or_default();
    health
        .latest_event_at_micros
        .fetch_max(event_micros, Ordering::Relaxed);
    metrics::set_lag(
        "projection",
        now.0.saturating_sub(command.fact.event_time.0),
    );
    let mut fact = command.fact;
    let admission = limiter.admit(&mut fact);
    for reason in &admission.reasons {
        metrics::record_cardinality(reason.as_str());
    }
    if !admission.accepted {
        health
            .cardinality_rejections
            .fetch_add(1, Ordering::Relaxed);
        gaps.record(
            &fact.org_id,
            fact.event_time,
            ProjectionGapReason::CardinalityRejected,
            1,
        );
        metrics::record_fact("cardinality_rejected");
        return;
    }
    if aggregator.observe(fact, command.trace_available).is_err() {
        health.degraded.store(true, Ordering::Release);
        *health.last_error.write() = Some("fact aggregation failed".into());
        metrics::set_health("projector", false);
    }
    health
        .pending_snapshots
        .store(aggregator.pending_snapshot_count(), Ordering::Relaxed);
}

async fn flush(
    repository: &dyn ApmWriteRepository,
    aggregator: &mut ApmAggregator,
    health: &ApmProjectorHealth,
    gaps: &GapLedger,
    config: &ApmProjectorConfig,
) {
    let now = TimestampMicros::now();
    let batch = aggregator.flush_batch(now, config.flush_max_snapshots);
    let pending_gaps = gaps.take();
    if batch.is_empty() && pending_gaps.is_empty() {
        return;
    }
    let started = Instant::now();
    let result = flush_batch(
        repository,
        &batch,
        &pending_gaps,
        config.max_error_samples_per_group,
    )
    .await;
    match result {
        Ok(()) => {
            aggregator.acknowledge(&batch);
            let cutoff =
                TimestampMicros(now.0.saturating_sub(
                    i64::try_from(config.late_grace.as_micros()).unwrap_or(i64::MAX),
                ));
            aggregator.evict_acked_before(cutoff);
            health.flush_successes.fetch_add(1, Ordering::Relaxed);
            health
                .last_success_at_micros
                .store(u64::try_from(now.0).unwrap_or_default(), Ordering::Relaxed);
            health.degraded.store(false, Ordering::Release);
            *health.last_error.write() = None;
            metrics::record_flush(true, started.elapsed());
            metrics::set_health("repository", true);
            metrics::set_health("projector", true);
        }
        Err(()) => {
            gaps.restore(&pending_gaps);
            for snapshot in &batch.snapshots {
                gaps.record(
                    &snapshot.org_id,
                    snapshot.bucket_at,
                    ProjectionGapReason::FlushFailed,
                    snapshot.measurements.request_count,
                );
            }
            health.flush_failures.fetch_add(1, Ordering::Relaxed);
            health.degraded.store(true, Ordering::Release);
            *health.last_error.write() = Some("repository flush failed".into());
            metrics::record_fact("flush_failed");
            metrics::record_flush(false, started.elapsed());
            metrics::set_health("repository", false);
            tracing::warn!(
                target: "molesignal::app::apm",
                "APM repository flush failed"
            );
        }
    }
    health
        .pending_snapshots
        .store(aggregator.pending_snapshot_count(), Ordering::Relaxed);
}

async fn flush_batch(
    repository: &dyn ApmWriteRepository,
    batch: &crate::app::apm::ApmFlushBatch,
    gaps: &[crate::domain::apm::ProjectionGap],
    max_error_samples: usize,
) -> Result<(), ()> {
    for (org_id, started_at) in &batch.projection_starts {
        repository
            .ensure_projection_started(org_id, *started_at)
            .await
            .map_err(|_| ())?;
    }
    repository
        .upsert_catalog(&batch.services, &batch.versions)
        .await
        .map_err(|_| ())?;
    repository
        .replace_owner_snapshots(&batch.snapshots)
        .await
        .map_err(|_| ())?;
    repository
        .upsert_error_groups(&batch.error_groups, &batch.error_samples, max_error_samples)
        .await
        .map_err(|_| ())?;
    repository
        .record_projection_gaps(gaps)
        .await
        .map_err(|_| ())?;
    for (org_id, bucket_at) in batch.latest_buckets_by_org() {
        repository
            .advance_projection_complete(&org_id, bucket_at)
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}

fn decrement_queue(health: &ApmProjectorHealth, capacity: usize) {
    health.queue_depth.fetch_sub(1, Ordering::Relaxed);
    metrics::set_queue(health.queue_depth.load(Ordering::Relaxed), capacity);
}
