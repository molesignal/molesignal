// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The background of realtime pipelines' metric, quota and throttle steps on every node: every few
//! seconds it writes the samples of the intervals that are over and syncs the limits' counts with
//! the other nodes; once an hour it deletes the counts of days nobody reads any more. A node that
//! stops writes the rest first (`roles::ShutdownWork`).

use std::{sync::Arc, time::Duration};

use tokio::{task::JoinHandle, time::MissedTickBehavior};

use crate::{
    app::intake::{IntakeService, realtime::StepState},
    domain::pipeline::limit::{DAY_MICROS, utc_day},
    shared::time::TimestampMicros,
};

/// How often samples are written and counts synced: how far a quota can see past other nodes'
/// counts, and about what a crash loses of the limits' counts.
const TICK_INTERVAL: Duration = Duration::from_secs(5);
const FORGET_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Days of counts kept, today included.
const KEEP_DAYS: i64 = 8;

/// Runs until the process ends.
pub fn spawn(steps: Arc<StepState>, intake: Arc<IntakeService>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK_INTERVAL);
        let mut forget = tokio::time::interval(FORGET_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        forget.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = tick.tick() => write(&steps, &intake).await,
                _ = forget.tick() => forget_old_days(&steps).await,
            }
        }
    })
}

#[tracing::instrument(
    name = "worker.pipeline_steps",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "pipeline_steps")
)]
async fn write(steps: &StepState, intake: &IntakeService) {
    steps.tick(intake, TimestampMicros::now()).await;
}

async fn forget_old_days(steps: &StepState) {
    let first_kept = utc_day(TimestampMicros::now()) - (KEEP_DAYS - 1) * DAY_MICROS;

    if let Err(error) = steps.ledger().forget_before(first_kept).await {
        tracing::warn!(%error, "old pipeline limit counts not deleted; trying again in an hour");
    }
}
