// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Periodic retention for RUM replay sidecar objects and metadata.

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    infra::rum::replay::RumReplayWriter,
    shared::{drain::DrainController, time::TimestampMicros},
};

const SWEEP_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const STARTUP_DELAY: Duration = Duration::from_secs(60);
const BATCH_SIZE: usize = 1_000;
const MAX_BATCHES_PER_TICK: usize = 10;

pub fn spawn(
    writer: Arc<RumReplayWriter>,
    retention_days: u32,
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
            let removed = sweep_once(&writer, retention_days).await;
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
async fn sweep_once(writer: &RumReplayWriter, retention_days: u32) -> usize {
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
    removed
}
