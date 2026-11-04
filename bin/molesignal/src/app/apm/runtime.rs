// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Role-aware projector and rollup lifecycle.
//!
//! The rollup is a singleton job: every alert node starts the loop, but only the alert primary
//! (see [`WorkOwnership::is_primary`]) runs it.

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
    ApmCandidateProjector, ApmLimitsHandle, ApmProjectorConfig, ApmProjectorHealthSnapshot,
    ApmRollupConfig, ApmRollupService, BufferedApmProjector, ErrorRuleBook, OrgSettingsBook,
    metrics, spawn_refresh, spawn_settings_refresh,
};
use crate::{
    app::cluster::WorkOwnership,
    config::ApmSettings,
    domain::{
        apm::{ApmCatalogRepository, ApmMaintenanceRepository, ApmRepository, ApmWriteRepository},
        iam::InstanceSettingsRepository,
    },
    shared::{Result, time::TimestampMicros},
};

/// How long limits saved on another node take at most to apply here.
const LIMITS_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct RollupHealth {
    runs: AtomicU64,
    failures: AtomicU64,
    last_success_at_micros: AtomicU64,
    degraded: AtomicBool,
    last_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApmRollupHealthSnapshot {
    pub running: bool,
    pub runs: u64,
    pub failures: u64,
    pub last_success_at: Option<TimestampMicros>,
    pub degraded: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    rules_join: Mutex<Option<JoinHandle<()>>>,
    settings_join: Mutex<Option<JoinHandle<()>>>,
    limits_join: Mutex<Option<JoinHandle<()>>>,
}

impl ApmRuntime {
    /// `catalog` keeps the error rules and settings the projector reads,
    /// `instance_settings` the limits it keeps to.
    pub fn start<R>(
        owner_id: String,
        repository: Arc<R>,
        catalog: Arc<dyn ApmCatalogRepository>,
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        settings: &ApmSettings,
        run_projector: bool,
        rollup: Option<Arc<WorkOwnership>>,
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
        let runs_rollup = rollup.is_some();
        let rules_join = projector.as_ref().map(|projector| {
            spawn_refresh(
                projector.error_rules(),
                catalog.clone(),
                rollup_cancel.clone(),
            )
        });
        let settings_join = projector.as_ref().map(|projector| {
            spawn_settings_refresh(projector.org_settings(), catalog, rollup_cancel.clone())
        });
        let limits_join = projector.as_ref().map(|projector| {
            spawn_limits_refresh(projector.limits(), instance_settings, rollup_cancel.clone())
        });

        let rollup_join = rollup.map(|ownership| {
            let maintenance: Arc<dyn ApmMaintenanceRepository> = repository;

            spawn_rollup(
                maintenance,
                ApmRollupConfig::from_settings(settings),
                rollup_health.clone(),
                rollup_cancel.clone(),
                ownership,
            )
        });

        if runs_rollup {
            metrics::set_health("rollup", true);
        }

        Ok(Arc::new(Self {
            projector,
            rollup_health,
            rollup_cancel,
            rollup_join: Mutex::new(rollup_join),
            rules_join: Mutex::new(rules_join),
            settings_join: Mutex::new(settings_join),
            limits_join: Mutex::new(limits_join),
        }))
    }

    /// The organizations' settings this node's projector applies, when it runs one.
    pub fn org_settings(&self) -> Option<Arc<OrgSettingsBook>> {
        self.projector
            .as_ref()
            .map(|projector| projector.org_settings())
    }

    /// The limits this node's projector keeps to, when it runs one.
    pub fn limits(&self) -> Option<ApmLimitsHandle> {
        self.projector.as_ref().map(|projector| projector.limits())
    }

    /// The error rules this node's projector applies, when it runs one.
    pub fn error_rules(&self) -> Option<Arc<ErrorRuleBook>> {
        self.projector
            .as_ref()
            .map(|projector| projector.error_rules())
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

        for joins in [&self.rules_join, &self.settings_join, &self.limits_join] {
            if let Some(join) = joins.lock().await.take() {
                let _ = join.await;
            }
        }
    }
}

/// Reads the platform's limits into `limits` now and every [`LIMITS_EVERY`]
/// until `cancel`; a failed read keeps the limits held.
fn spawn_limits_refresh(
    limits: ApmLimitsHandle,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(LIMITS_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => match instance_settings.get().await {
                    Ok(settings) => *limits.write() = settings.apm_limits,
                    Err(error) => tracing::warn!(
                        target: "molesignal::app::apm",
                        error = %error,
                        "APM limits could not be read; keeping the limits held"
                    ),
                },
            }
        }
    })
}

fn spawn_rollup(
    repository: Arc<dyn ApmMaintenanceRepository>,
    config: ApmRollupConfig,
    health: Arc<RollupHealth>,
    cancel: CancellationToken,
    ownership: Arc<WorkOwnership>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let service = ApmRollupService::new(repository, config.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {
                    // 单点任务：非主节点跳过这一轮。
                    if !ownership.is_primary().await {
                        continue;
                    }
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
