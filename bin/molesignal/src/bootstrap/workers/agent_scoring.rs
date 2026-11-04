// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 质量打分的 worker：做排着的打分，做 Prompt 晋级检查，清理做完的打分记录。
//!
//! 提供完整 HTTP 的节点各起一份，靠数据库协调：打分与检查都用 SKIP LOCKED 领走、带着租约，节点
//! 没了由别的节点接着做。检查要重新回答与评好几个样本，放在单独的循环里，不挡打分。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    api::{
        AppState,
        http::routes::agent::quality::{process_due, process_next_check},
    },
    shared::time::TimestampMicros,
};

/// 多久看一次排着的打分。
const TICK: Duration = Duration::from_secs(5);
/// 一次最多接着领这么多批：一批是满的说明后面还有。
const BATCHES_PER_TICK: usize = 5;
/// 一批领满是这么多个（与 `quality::process` 的批大小一致）。
const FULL_BATCH: usize = 10;
/// 多久看一次排着的检查。
const CHECK_TICK: Duration = Duration::from_secs(10);
/// 多久清理一次做完的打分记录，留多久。
const PURGE_EVERY: Duration = Duration::from_secs(60 * 60);
const KEEP_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;

pub struct ScoringWorker {
    state: AppState,
}

impl ScoringWorker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        let worker = Arc::new(self);
        let checks = worker.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CHECK_TICK).await;
                checks.checks().await;
            }
        });

        tokio::spawn(async move {
            let mut purged_at = tokio::time::Instant::now();

            loop {
                tokio::time::sleep(TICK).await;

                let purge = purged_at.elapsed() >= PURGE_EVERY;

                if purge {
                    purged_at = tokio::time::Instant::now();
                }

                worker.tick(purge).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_scoring",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_scoring")
    )]
    async fn tick(&self, purge: bool) {
        for _ in 0..BATCHES_PER_TICK {
            match process_due(&self.state).await {
                Ok(claimed) if claimed >= FULL_BATCH => {}
                Ok(_) => break,
                Err(error) => {
                    tracing::warn!(%error, "score jobs could not be claimed");
                    break;
                }
            }
        }

        if purge {
            let before = TimestampMicros(TimestampMicros::now().0 - KEEP_MICROS);

            match self.state.agent.scoring.purge_jobs(before).await {
                Ok(0) => {}
                Ok(purged) => tracing::debug!(purged, "finished score jobs purged"),
                Err(error) => tracing::warn!(%error, "finished score jobs could not be purged"),
            }
        }
    }

    #[tracing::instrument(
        name = "worker.agent_prompt_checks",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_prompt_checks")
    )]
    async fn checks(&self) {
        loop {
            match process_next_check(&self.state).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    tracing::warn!(%error, "a prompt check could not be run");
                    break;
                }
            }
        }
    }
}
