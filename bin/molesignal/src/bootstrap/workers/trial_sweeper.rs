// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 试用状态机 sweeper：周期遍历 `active` 试用，推进 state（active→expired/converted）
//! 与通知阶段（none→expiring_soon→last_day→expired，单调前进）。
//!
//! `converted` 判定依据该 org 是否有 `active` 订阅（marketplace_subscriptions，含 Stripe）。
//! 通知当前以结构化日志事件呈现；真实投递（邮件给 org owner）作为后续接入点。
//!
//! 放在 bootstrap：worker 依赖 infra repo 实装，app crate 不能依赖 infra。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::billing::{evaluate_trial_state, trial_days_remaining, trial_notify_stage},
    infra::persistence::repositories::{
        marketplace::MarketplaceRepository, trials::TrialRepository,
    },
    shared::{Result, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct TrialSweeperConfig {
    pub interval_secs: u64,
}

impl Default for TrialSweeperConfig {
    fn default() -> Self {
        Self { interval_secs: 300 }
    }
}

pub struct TrialSweeper {
    trials: Arc<dyn TrialRepository>,
    marketplace: Arc<dyn MarketplaceRepository>,
    cfg: TrialSweeperConfig,
}

impl TrialSweeper {
    pub fn new(
        trials: Arc<dyn TrialRepository>,
        marketplace: Arc<dyn MarketplaceRepository>,
        cfg: TrialSweeperConfig,
    ) -> Self {
        Self {
            trials,
            marketplace,
            cfg,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(self.cfg.interval_secs));
            loop {
                tick.tick().await;
                if let Err(e) = self.sweep_once().await {
                    tracing::warn!(error = %e, "trial sweep failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.trial_sweeper",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "trial_sweeper")
    )]
    async fn sweep_once(&self) -> Result<()> {
        let now = TimestampMicros::now();
        let active = self.trials.list_active().await?;
        for trial in active {
            let subs = self.marketplace.list(&trial.org_id).await?;
            let has_active_sub = subs.iter().any(|s| s.state == "active");
            let new_state = evaluate_trial_state(trial.ends_at, now, has_active_sub);
            // 通知阶段单调前进：取已记录阶段与当前应到阶段的较大者。
            let target_stage = trial_notify_stage(trial.ends_at, now);
            let new_stage = trial.notified_stage.max(target_stage);

            if new_state == trial.state && new_stage == trial.notified_stage {
                continue;
            }
            self.trials
                .update_progress(&trial.org_id, new_state, new_stage, now)
                .await?;
            if new_stage > trial.notified_stage {
                // 通知事件（当前结构化日志；真实邮件投递为后续接入点）。
                tracing::info!(
                    org_id = %trial.org_id.0,
                    stage = new_stage.as_str(),
                    state = new_state.as_str(),
                    days_left = trial_days_remaining(trial.ends_at, now),
                    "trial notification stage advanced"
                );
            }
        }
        Ok(())
    }
}
