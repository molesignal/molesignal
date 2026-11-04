// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Periodic retention for RUM replay sidecar objects and metadata. Replays are kept as long as a
//! stream without a retention of its own: the platform setting, read on every sweep. The comments
//! and the summaries of a session go with its replay.

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::{
        iam::InstanceSettingsRepository,
        rum::{RumReplayCommentRepository, RumSessionSummaryRepository},
    },
    infra::rum::replay::RumReplayWriter,
    shared::{drain::DrainController, time::TimestampMicros},
};

const SWEEP_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const STARTUP_DELAY: Duration = Duration::from_secs(60);
const BATCH_SIZE: usize = 1_000;
const MAX_BATCHES_PER_TICK: usize = 10;

/// What goes with a replay.
pub struct ReplayNotes {
    pub comments: Arc<dyn RumReplayCommentRepository>,
    pub summaries: Arc<dyn RumSessionSummaryRepository>,
}

pub fn spawn(
    writer: Arc<RumReplayWriter>,
    notes: ReplayNotes,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = tokio::time::Instant::now() + STARTUP_DELAY;
        let mut ticker = tokio::time::interval_at(start, SWEEP_INTERVAL);

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            // A guessed retention could delete what should be kept: skip the sweep instead.
            let retention_days = match instance_settings.get().await {
                Ok(settings) => settings.default_retention_days,
                Err(error) => {
                    tracing::warn!(%error, "instance settings unreadable; RUM replay sweep skipped");
                    continue;
                }
            };

            let removed = sweep_once(&writer, &notes, retention_days).await;

            if removed > 0 {
                tracing::info!(segments = removed, "swept expired RUM replay segments");
            }
        }
    })
}

#[tracing::instrument(
    name = "worker.rum_replay_retention",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "rum_replay_retention")
)]
async fn sweep_once(writer: &RumReplayWriter, notes: &ReplayNotes, retention_days: u32) -> usize {
    let retention_micros = i64::from(retention_days.max(1))
        .saturating_mul(86_400)
        .saturating_mul(1_000_000);

    let cutoff = TimestampMicros::now().0.saturating_sub(retention_micros);
    let mut removed = 0_usize;

    for _ in 0..MAX_BATCHES_PER_TICK {
        match writer.sweep_expired(cutoff, BATCH_SIZE).await {
            Ok(count) => {
                removed += count;

                if count < BATCH_SIZE {
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "RUM replay retention sweep failed");
                break;
            }
        }
    }

    let cutoff = TimestampMicros(cutoff);

    if let Err(error) = notes.comments.delete_older_than(cutoff, BATCH_SIZE).await {
        tracing::warn!(error = %error, "RUM replay comment sweep failed");
    }

    if let Err(error) = notes.summaries.delete_older_than(cutoff, BATCH_SIZE).await {
        tracing::warn!(error = %error, "RUM session summary sweep failed");
    }

    removed
}
