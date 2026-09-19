// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Role-aware projector and rollup lifecycle.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
    ApmCandidateProjector, ApmProjectorConfig, ApmProjectorHealthSnapshot, ApmRollupConfig,
    ApmRollupService, BufferedApmProjector, metrics,
};
use crate::{
    config::ApmSettings,
    domain::apm::{ApmMaintenanceRepository, ApmRepository, ApmWriteRepository},
    shared::{Result, time::TimestampMicros},
};

#[derive(Debug, Default)]
struct RollupHealth {
    runs: AtomicU64,
    failures: AtomicU64,
    last_success_at_micros: AtomicU64,
    degraded: AtomicBool,
    last_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmRollupHealthSnapshot {
    pub running: bool,
    pub runs: u64,
    pub failures: u64,
    pub last_success_at: Option<TimestampMicros>,
    pub degraded: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmRuntimeHealthSnapshot {
    pub enabled: bool,
    pub candidate_owner: bool,
    pub projector: Option<ApmProjectorHealthSnapshot>,
    pub rollup: ApmRollupHealthSnapshot,
}

pub struct ApmRuntime {
    projector: Option<Arc<BufferedApmProjector>>,
    rollup_health: Arc<RollupHealth>,
    rollup_cancel: CancellationToken,
    rollup_join: Mutex<Option<JoinHandle<()>>>,
}

impl ApmRuntime {
    pub fn start<R>(
        owner_id: String,
        repository: Arc<R>,
        settings: &ApmSettings,
        run_projector: bool,
        run_rollup: bool,
    ) -> Result<Arc<Self>>
    where
        R: ApmRepository + 'static,
    {
        let projector = if run_projector {
            let writer: Arc<dyn ApmWriteRepository> = repository.clone();
            Some(BufferedApmProjector::start(
                owner_id,
                writer,
                ApmProjectorConfig::from_settings(settings),
            )?)
        } else {
            None
        };
        let rollup_health = Arc::new(RollupHealth::default());
        let rollup_cancel = CancellationToken::new();
        let rollup_join = run_rollup.then(|| {
            let maintenance: Arc<dyn ApmMaintenanceRepository> = repository;
            spawn_rollup(
                maintenance,
                ApmRollupConfig::from_settings(settings),
                rollup_health.clone(),
                rollup_cancel.clone(),
            )
        });
        if run_rollup {
            metrics::set_health("rollup", true);
        }
        Ok(Arc::new(Self {
            projector,
            rollup_health,
            rollup_cancel,
            rollup_join: Mutex::new(rollup_join),
        }))
    }

    pub fn projector(&self) -> Option<Arc<dyn ApmCandidateProjector>> {
        self.projector
            .as_ref()
            .map(|projector| projector.clone() as Arc<dyn ApmCandidateProjector>)
    }

    pub fn health(&self) -> ApmRuntimeHealthSnapshot {
        let last_success = self
            .rollup_health
            .last_success_at_micros
            .load(Ordering::Relaxed);
        let rollup_running = match self.rollup_join.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(_) => true,
        };
        ApmRuntimeHealthSnapshot {
            enabled: true,
            candidate_owner: self.projector.is_some(),
            projector: self.projector.as_ref().map(|value| value.health()),
            rollup: ApmRollupHealthSnapshot {
                running: rollup_running,
                runs: self.rollup_health.runs.load(Ordering::Relaxed),
                failures: self.rollup_health.failures.load(Ordering::Relaxed),
                last_success_at: (last_success != 0)
                    .then(|| TimestampMicros(i64::try_from(last_success).unwrap_or(i64::MAX))),
                degraded: self.rollup_health.degraded.load(Ordering::Acquire),
                last_error: self.rollup_health.last_error.read().clone(),
            },
        }
    }

    /// Trace candidate routing/pipeline must stop before this method so no
    /// producer can enqueue after the projector begins its bounded drain.
    pub async fn shutdown(&self) {
        if let Some(projector) = &self.projector {
            projector.shutdown().await;
        }
        self.rollup_cancel.cancel();
        if let Some(join) = self.rollup_join.lock().await.take() {
            let _ = join.await;
        }
    }
}

fn spawn_rollup(
    repository: Arc<dyn ApmMaintenanceRepository>,
    config: ApmRollupConfig,
    health: Arc<RollupHealth>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let service = ApmRollupService::new(repository, config.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {
                    let now = TimestampMicros::now();
                    match service.run_once(now).await {
                        Ok(run) => {
                            health.runs.fetch_add(1, Ordering::Relaxed);
                            health.degraded.store(false, Ordering::Release);
                            health.last_success_at_micros.store(
                                u64::try_from(now.0).unwrap_or_default(),
                                Ordering::Relaxed,
                            );
                            *health.last_error.write() = None;
                            metrics::record_rollup(
                                true,
                                run.source_rows,
                                run.rollup_rows,
                                run.deleted_hot_rows.saturating_add(run.deleted_rollup_rows),
                            );
                            metrics::set_lag(
                                "rollup",
                                config.late_grace_micros,
                            );
                            metrics::set_health("rollup", true);
                        }
                        Err(_) => {
                            health.failures.fetch_add(1, Ordering::Relaxed);
                            health.degraded.store(true, Ordering::Release);
                            *health.last_error.write() = Some("rollup repository failure".into());
                            metrics::record_rollup(false, 0, 0, 0);
                            metrics::set_health("rollup", false);
                            tracing::warn!(
                                target: "molesignal::app::apm",
                                "APM rollup run failed"
                            );
                        }
                    }
                }
            }
        }
    })
}
