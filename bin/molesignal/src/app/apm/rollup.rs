// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use crate::{
    config::ApmSettings,
    domain::apm::{ApmMaintenanceRepository, RollupRequest, RollupStats},
    shared::{Result, time::TimestampMicros},
};

const HOUR_MICROS: i64 = 3_600 * 1_000_000;
const DAY_MICROS: i64 = 24 * HOUR_MICROS;
const DEFAULT_CANDIDATE_LIMIT: usize = 128;

#[derive(Debug, Clone)]
pub struct ApmRollupConfig {
    pub late_grace_micros: i64,
    pub hot_retention_micros: i64,
    pub rollup_retention_micros: i64,
    pub candidate_limit: usize,
}

impl ApmRollupConfig {
    pub fn from_settings(settings: &ApmSettings) -> Self {
        Self {
            late_grace_micros: i64::try_from(settings.late_grace_secs)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000_000),
            hot_retention_micros: i64::from(settings.hot_retention_hours)
                .saturating_mul(HOUR_MICROS),
            rollup_retention_micros: i64::from(settings.rollup_retention_days)
                .saturating_mul(DAY_MICROS),
            candidate_limit: DEFAULT_CANDIDATE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApmRollupRun {
    pub candidates: u64,
    pub source_rows: u64,
    pub rollup_rows: u64,
    pub deleted_hot_rows: u64,
    pub deleted_rollup_rows: u64,
}

pub struct ApmRollupService {
    repository: Arc<dyn ApmMaintenanceRepository>,
    config: ApmRollupConfig,
}

impl ApmRollupService {
    pub fn new(repository: Arc<dyn ApmMaintenanceRepository>, config: ApmRollupConfig) -> Self {
        Self { repository, config }
    }

    /// Rolls only hours whose end is older than the late-data grace. Each
    /// repository call is an independent tenant-scoped transaction so one
    /// heavy tenant cannot expand another tenant's lock scope.
    pub async fn run_once(&self, now: TimestampMicros) -> Result<ApmRollupRun> {
        let grace_cutoff = now.0.saturating_sub(self.config.late_grace_micros);
        let closed_before = grace_cutoff.div_euclid(HOUR_MICROS) * HOUR_MICROS;
        let candidates = self
            .repository
            .rollup_candidates(TimestampMicros(closed_before), self.config.candidate_limit)
            .await?;
        let mut run = ApmRollupRun {
            candidates: candidates.len() as u64,
            ..ApmRollupRun::default()
        };
        for candidate in candidates {
            let stats = self
                .repository
                .rollup_and_retain(&RollupRequest {
                    org_id: candidate.org_id,
                    hour_at: candidate.hour_at,
                    hot_retention_cutoff: TimestampMicros(
                        now.0.saturating_sub(self.config.hot_retention_micros),
                    ),
                    rollup_retention_cutoff: TimestampMicros(
                        now.0.saturating_sub(self.config.rollup_retention_micros),
                    ),
                })
                .await?;
            add_stats(&mut run, stats);
        }
        Ok(run)
    }
}

fn add_stats(run: &mut ApmRollupRun, stats: RollupStats) {
    run.source_rows = run.source_rows.saturating_add(stats.source_rows);
    run.rollup_rows = run.rollup_rows.saturating_add(stats.rollup_rows);
    run.deleted_hot_rows = run.deleted_hot_rows.saturating_add(stats.deleted_hot_rows);
    run.deleted_rollup_rows = run
        .deleted_rollup_rows
        .saturating_add(stats.deleted_rollup_rows);
}
