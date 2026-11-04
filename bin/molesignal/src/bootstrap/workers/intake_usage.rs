// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The background of intake usage: every node writes what it measured and reads the limit it counts
//! services by; the compactor primary deletes rows past their retention; the alert nodes look at
//! the budgets of the organizations they own.

use std::{sync::Arc, time::Duration};

use tokio::{task::JoinHandle, time::MissedTickBehavior};

use crate::{
    app::{
        cluster::WorkOwnership, intake::usage::IntakeUsageMeter, notify::IntakeBudgetEventProducer,
    },
    domain::{
        iam::{InstanceSettingsRepository, OrganizationRepository},
        intake::usage::source::IntakeUsageRepository,
    },
    shared::{Result, drain::DrainController, time::TimestampMicros},
};

/// How often a node writes what it measured: the most a crash can lose.
const FLUSH_INTERVAL: Duration = Duration::from_secs(15);
/// How often a node reads the platform settings again.
const SETTINGS_INTERVAL: Duration = Duration::from_secs(30);
/// How often the budgets of an organization are looked at.
const BUDGET_INTERVAL: Duration = Duration::from_secs(60);
/// Two rounds of the retention sweep.
const SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const SWEEP_STARTUP_DELAY: Duration = Duration::from_secs(5 * 60);
const SWEEP_NOT_PRIMARY_RECHECK: Duration = Duration::from_secs(60);
const SWEEP_RETRY_AFTER_ERROR: Duration = Duration::from_secs(60 * 60);
const SWEEP_BATCH_SIZE: i64 = 5_000;
const SWEEP_BATCH_PAUSE: Duration = Duration::from_millis(100);

/// Writes what this node measures. Runs until the process ends: the shutdown writes once more for
/// what came in after the last round.
pub fn spawn_meter(
    meter: Arc<IntakeUsageMeter>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut flush = tokio::time::interval(FLUSH_INTERVAL);
        let mut settings = tokio::time::interval(SETTINGS_INTERVAL);
        flush.set_missed_tick_behavior(MissedTickBehavior::Delay);
        settings.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = flush.tick() => write(&meter).await,
                _ = settings.tick() => match instance_settings.get().await {
                    Ok(settings) => meter.set_max_services_per_org(
                        settings.intake_usage_max_services_per_org as usize,
                    ),
                    Err(error) => {
                        tracing::warn!(%error, "intake usage settings unreadable; keeping the current limit");
                    }
                },
            }
        }
    })
}

#[tracing::instrument(
    name = "worker.intake_usage",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "intake_usage")
)]
async fn write(meter: &IntakeUsageMeter) {
    if let Err(error) = meter.flush().await {
        tracing::warn!(%error, "intake usage write failed; keeping the rows for the next one");
    }
}

/// Deletes the hourly rows past the retention of the platform settings, on the compactor primary.
pub fn spawn_retention(
    usage: Arc<dyn IntakeUsageRepository>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(SWEEP_STARTUP_DELAY).await;

        while !drain.is_draining() {
            let delay = if !ownership.is_primary().await {
                SWEEP_NOT_PRIMARY_RECHECK
            } else {
                match sweep_once(usage.as_ref(), instance_settings.as_ref(), &drain).await {
                    Ok(deleted) => {
                        if deleted > 0 {
                            tracing::info!(deleted, "expired intake usage rows deleted");
                        }

                        SWEEP_INTERVAL
                    }
                    Err(error) => {
                        tracing::warn!(%error, "intake usage retention sweep failed");
                        SWEEP_RETRY_AFTER_ERROR
                    }
                }
            };

            tokio::time::sleep(delay).await;
        }
    })
}

#[tracing::instrument(
    name = "worker.intake_usage_retention",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "intake_usage_retention")
)]
async fn sweep_once(
    usage: &dyn IntakeUsageRepository,
    instance_settings: &dyn InstanceSettingsRepository,
    drain: &DrainController,
) -> Result<u64> {
    let settings = instance_settings.get().await?;

    let Some(before) = settings.intake_usage_retention_cutoff(TimestampMicros::now()) else {
        return Ok(0);
    };

    let mut deleted = 0;

    loop {
        let batch = usage.delete_before(before.0, SWEEP_BATCH_SIZE).await?;
        deleted += batch;

        if batch < SWEEP_BATCH_SIZE as u64 || drain.is_draining() {
            return Ok(deleted);
        }

        tokio::time::sleep(SWEEP_BATCH_PAUSE).await;
    }
}

/// Looks at the budgets of the organizations this alert node owns.
pub fn spawn_budget_events(
    producer: Arc<IntakeBudgetEventProducer>,
    orgs: Arc<dyn OrganizationRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(BUDGET_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while !drain.is_draining() {
            ticker.tick().await;
            look_at_budgets(&producer, orgs.as_ref(), &ownership).await;
        }
    })
}

#[tracing::instrument(
    name = "worker.intake_budgets",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "intake_budgets")
)]
async fn look_at_budgets(
    producer: &IntakeBudgetEventProducer,
    orgs: &dyn OrganizationRepository,
    ownership: &WorkOwnership,
) {
    let orgs = match orgs.list().await {
        Ok(orgs) => orgs,
        Err(error) => {
            tracing::warn!(%error, "intake budgets could not list the organizations");
            return;
        }
    };

    let view = ownership.view().await;

    for org in orgs {
        if !view.owns(&format!("intake_budget:{}", org.id)) {
            continue;
        }

        if let Err(error) = producer.tick(&org.id, TimestampMicros::now()).await {
            tracing::warn!(org = %org.id, %error, "intake budget look failed");
        }
    }
}
