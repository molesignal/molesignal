// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时日志巡检的后台循环。
//!
//! 设计：
//! - 每 15 秒一拍，依次做三件事：了结卡住的执行（节点中途退出、排队后没人执行）→ 领走排队的
//!   手动触发 → 开始到期的定时执行。
//! - 多个 alert 节点按巡检 id 分担：开始前先过 [`RunGate`]（归属 + 运行租约）。同一条巡检同一
//!   时刻至多一条执行由数据库的唯一索引兜底，成员变化的窗口里也不会跑两遍。
//! - 一次巡检要来回调用模型和工具，可能持续几分钟：每次执行在自己的任务里跑，不占住这一拍；
//!   单节点同时执行的条数有上限（`[log_inspection].concurrent_runs`），到期而没轮到的下一拍再看。
//! - 每小时清理一次过了保留期的执行记录。
//!
//! 一次执行的全过程在 `app::log_inspection::InspectionRunner`；调模型和调工具的实现在
//! [`agent`] 与 `dispatcher`。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};

use crate::{
    app::log_inspection::InspectionRunner,
    domain::{
        log_inspection::{LogInspection, LogInspectionRun},
        run_claim::{OpenGate, RunGate, RunPermit},
    },
    shared::{Result, time::TimestampMicros},
};

pub mod agent;
mod dispatcher;

const TICK: Duration = Duration::from_secs(15);

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "log_inspection";

/// 过期执行记录的清理间隔。
const PURGE_EVERY: Duration = Duration::from_secs(3600);

/// 一拍里最多看多少条到期的巡检、多少条排队的手动触发。
const BATCH: i64 = 200;

pub struct LogInspectionWorker {
    runner: Arc<InspectionRunner>,
    /// 多个 alert 节点分担巡检时的准入；默认每条到期的巡检都由本节点执行。
    gate: Arc<dyn RunGate>,
    /// 本节点同时执行的巡检数。
    slots: Arc<Semaphore>,
    purged_at: Mutex<Option<Instant>>,
}

impl LogInspectionWorker {
    pub fn new(runner: Arc<InspectionRunner>) -> Self {
        let slots = Arc::new(Semaphore::new(runner.limits().concurrent_runs));

        Self {
            runner,
            gate: Arc::new(OpenGate),
            slots,
            purged_at: Mutex::new(None),
        }
    }

    /// 与其余 alert 节点分担巡检：每条巡检开始前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
    }

    pub fn spawn(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.tick().await;

            loop {
                ticker.tick().await;

                if let Err(error) = self.tick_once(TimestampMicros::now()).await {
                    tracing::warn!(%error, "log_inspection tick failed");
                }
            }
        })
    }

    /// 一拍。返回这一拍里开始的执行，各自在自己的任务里跑。
    #[tracing::instrument(
        name = "worker.log_inspection",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "log_inspection")
    )]
    pub async fn tick_once(&self, now: TimestampMicros) -> Result<Vec<JoinHandle<()>>> {
        self.settle_stale(now).await;
        self.purge_when_due(now).await;

        let mut started = Vec::new();

        // 手动触发在前：有人正等着看结果。
        for queued in self.runner.queued(BATCH).await? {
            let Some(slot) = self.free_slot() else {
                return Ok(started);
            };

            let Some(permit) = self.admit(queued.inspection_id.as_str()).await else {
                continue;
            };

            match self.runner.begin_queued(&queued, now).await {
                Ok(Some((inspection, run))) => {
                    started.push(self.run(inspection, run, permit, slot));
                }
                Ok(None) => release(permit, queued.inspection_id.as_str()).await,
                Err(error) => {
                    tracing::warn!(run_id = %queued.id.0, %error, "queued log inspection run could not start");
                    release(permit, queued.inspection_id.as_str()).await;
                }
            }
        }

        for due in self.runner.due(now, BATCH).await? {
            let Some(slot) = self.free_slot() else {
                break;
            };

            // 先认领再开始：归属决定谁来执行，运行租约兜住成员变化时两个节点同时开始。
            let Some(permit) = self.admit(due.id.as_str()).await else {
                continue;
            };

            // 认领后由执行器重读：别的节点可能刚执行完并推进了下次执行时间。
            match self.runner.begin_scheduled(&due.id, now).await {
                Ok(Some((inspection, run))) => {
                    started.push(self.run(inspection, run, permit, slot));
                }
                Ok(None) => release(permit, due.id.as_str()).await,
                Err(error) => {
                    tracing::warn!(inspection_id = %due.id.0, %error, "log inspection run could not start");
                    release(permit, due.id.as_str()).await;
                }
            }
        }

        Ok(started)
    }

    /// 卡住的执行逐条了结。任何节点都可以做：了结是原子的，后到的那个什么都不会改。
    async fn settle_stale(&self, now: TimestampMicros) {
        let stale = match self.runner.stale(now).await {
            Ok(stale) => stale,
            Err(error) => {
                tracing::warn!(%error, "stale log inspection runs could not be listed");
                return;
            }
        };

        for run in stale {
            if let Err(error) = self.runner.abandon(&run).await {
                tracing::warn!(run_id = %run.id.0, %error, "stale log inspection run could not be settled");
            }
        }
    }

    async fn purge_when_due(&self, now: TimestampMicros) {
        let due = self
            .purged_at
            .lock()
            .is_none_or(|at| at.elapsed() >= PURGE_EVERY);

        if !due {
            return;
        }

        *self.purged_at.lock() = Some(Instant::now());

        match self.runner.purge_expired(now).await {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "expired log inspection runs removed"),
            Err(error) => {
                tracing::warn!(%error, "expired log inspection runs could not be removed")
            }
        }
    }

    fn free_slot(&self) -> Option<OwnedSemaphorePermit> {
        self.slots.clone().try_acquire_owned().ok()
    }

    async fn admit(&self, inspection_id: &str) -> Option<RunPermit> {
        match self.gate.admit(RUN_KIND, inspection_id).await {
            Ok(permit) => permit,
            Err(error) => {
                tracing::warn!(inspection_id, %error, "log inspection run admission failed");
                None
            }
        }
    }

    /// 在自己的任务里执行，结束后交还运行租约和并发名额。
    fn run(
        &self,
        inspection: LogInspection,
        run: LogInspectionRun,
        permit: RunPermit,
        slot: OwnedSemaphorePermit,
    ) -> JoinHandle<()> {
        let runner = self.runner.clone();

        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            let outcome = runner.execute(&inspection, &run).await;

            tracing::info!(
                inspection_id = %inspection.id.0,
                run_id = %run.id.0,
                status = outcome.status.as_str(),
                level = outcome.level.map(|level| level.as_str()),
                notifications = outcome.notifications,
                "log inspection run finished"
            );

            release(permit, inspection.id.as_str()).await;
            drop(slot);
        })
    }
}

async fn release(permit: RunPermit, inspection_id: &str) {
    if let Err(error) = permit.release().await {
        tracing::warn!(inspection_id, %error, "log inspection run claim release failed");
    }
}

#[cfg(test)]
mod tests;
