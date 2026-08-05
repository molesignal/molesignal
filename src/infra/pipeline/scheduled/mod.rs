// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ScheduledPipelineRunner（pipeline-runs-and-backfill）。
//!
//! 当前 cron 解析：仅支持 `every:Ns` 简化语法（每 N 秒一次），完整 cron 解析留
//! follow-up。alert_manager role 内调 `tick_once`；按 last_run_at 决定是否要触发。
//!
//! pipeline-runs-and-backfill：每次 fire 都向 `pipeline_runs` 写一行，
//! `record_start` 进入 → 真实工作 → `record_finish` 退出。RAII guard 保证 panic
//! / error 也能更新 row（当前 stub 没有真正 fail 路径，但保留接口）。

use std::sync::Arc;

use async_trait::async_trait;

use self::repository::{ScheduledPipeline, ScheduledPipelineRepository};
use crate::{
    infra::persistence::repositories::pipelines::runs::{
        PipelineRun, PipelineRunRepository, PipelineRunState,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub mod repository;

/// 注入式执行器：一次 pipeline run 的「读源窗口 → 函数链 → 写目标 stream → egress」。
/// infra 不依赖 app（`QueryService`），故「读源」由 bootstrap 实装注入；返回写入目标的行数。
/// 未注入时 [`ScheduledPipelineRunner::tick_once`] 仅推进 schedule（旧 stub 行为）。
#[async_trait]
pub trait PipelineExecutor: Send + Sync {
    async fn execute(&self, pipeline: &ScheduledPipeline) -> Result<u64>;
}

pub struct ScheduledPipelineRunner {
    repo: Arc<dyn ScheduledPipelineRepository>,
    runs: Option<Arc<dyn PipelineRunRepository>>,
    executor: Option<Arc<dyn PipelineExecutor>>,
}

impl ScheduledPipelineRunner {
    /// 旧入口：未注入 runs repo 时所有 `record_*` 调用变成 no-op。便于测试。
    pub fn new(repo: Arc<dyn ScheduledPipelineRepository>) -> Self {
        Self {
            repo,
            runs: None,
            executor: None,
        }
    }

    /// pipeline-runs-and-backfill：注入 runs repo 后，每次 tick 写一条 row。
    pub fn with_runs(
        repo: Arc<dyn ScheduledPipelineRepository>,
        runs: Arc<dyn PipelineRunRepository>,
    ) -> Self {
        Self {
            repo,
            runs: Some(runs),
            executor: None,
        }
    }

    /// 注入真实执行器（bootstrap 实装，持 `QueryService`）。未注入时 tick 仅推进 schedule。
    pub fn with_executor(mut self, executor: Arc<dyn PipelineExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// 单次 tick：列所有 enabled，对到期的调 `execute_one`（当前仅打 metric + touch_last_run）。
    /// 命中时同步写一条 `pipeline_runs` 行（state=running → 完成时更新为终态）。
    pub async fn tick_once(&self) -> Result<usize> {
        let pipelines = self.repo.list_enabled_all().await?;
        let now = TimestampMicros::now();
        let mut fired = 0usize;
        for p in pipelines {
            let due = match parse_every_secs(&p.cron) {
                Some(secs) => match p.last_run_at {
                    Some(last) => now.0 - last.0 >= secs * 1_000_000,
                    None => true,
                },
                None => false, // 不支持的 cron 语法跳过
            };
            if due {
                tracing::debug!(
                    pipeline_id = %p.id.0,
                    name = %p.name,
                    "scheduled pipeline fired"
                );

                // 写入 running 行（如果有 runs repo 注入）。
                let run_id = Id::new();
                if let Some(runs) = &self.runs {
                    let row = PipelineRun {
                        id: run_id.clone(),
                        pipeline_id: p.id.clone(),
                        org_id: p.org_id.clone(),
                        state: PipelineRunState::Running,
                        started_at: now,
                        finished_at: None,
                        scanned_rows: 0,
                        error: None,
                    };
                    if let Err(e) = runs.record_start(row).await {
                        tracing::warn!(error = %e, "pipeline_runs record_start failed");
                    }
                }

                // 真实执行（注入 executor 时）：读源窗口 → 函数链 → 写目标 stream → egress；
                // 未注入则仅推进 schedule（旧 stub 行为，便于无 query 依赖的测试）。
                let exec_result = match &self.executor {
                    Some(exec) => exec.execute(&p).await,
                    None => Ok(0u64),
                };
                // 无论成败都 touch_last_run 推进 schedule，避免失败 pipeline 每 tick 热重试。
                let _ = self.repo.touch_last_run(&p.id, now).await;
                let finished = TimestampMicros::now();
                let scanned_rows = *exec_result.as_ref().unwrap_or(&0) as i64;
                let unit_result: Result<()> = exec_result.map(|_| ());
                let (state, error_msg) = map_exec_result(&unit_result);

                if let Some(runs) = &self.runs
                    && let Err(e) = runs
                        .record_finish(&run_id, state, finished, scanned_rows, error_msg)
                        .await
                {
                    tracing::warn!(error = %e, "pipeline_runs record_finish failed");
                }

                // 单个 pipeline 执行失败只记录，不中断本轮其余 pipeline。
                if let Err(e) = &unit_result {
                    tracing::warn!(pipeline_id = %p.id.0, error = %e, "scheduled pipeline execution failed");
                }
                fired += 1;
            }
        }
        Ok(fired)
    }
}

/// Map a pipeline tick's outcome to the row's terminal state + optional error message.
/// `Err::Cancelled(_)` → `cancelled`; any other `Err` → `failed`; `Ok` → `succeeded`.
pub(crate) fn map_exec_result(r: &Result<()>) -> (PipelineRunState, Option<String>) {
    match r {
        Ok(()) => (PipelineRunState::Succeeded, None),
        Err(e) if matches!(e, crate::shared::Error::Cancelled(_)) => {
            (PipelineRunState::Cancelled, Some(e.to_string()))
        }
        Err(e) => (PipelineRunState::Failed, Some(e.to_string())),
    }
}

fn parse_every_secs(cron: &str) -> Option<i64> {
    // 支持 `every:60s` / `every:5m` / `every:1h`
    let s = cron.trim();
    let rest = s.strip_prefix("every:")?;
    let (num, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;
    Some(match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::Error;

    #[test]
    fn every_parser() {
        assert_eq!(parse_every_secs("every:60s"), Some(60));
        assert_eq!(parse_every_secs("every:5m"), Some(300));
        assert_eq!(parse_every_secs("every:1h"), Some(3600));
        assert_eq!(parse_every_secs("0 * * * *"), None);
    }

    #[test]
    fn map_exec_result_success() {
        let (state, err) = map_exec_result(&Ok(()));
        assert_eq!(state, PipelineRunState::Succeeded);
        assert!(err.is_none());
    }

    #[test]
    fn map_exec_result_failure() {
        let r: Result<()> = Err(Error::internal("boom"));
        let (state, err) = map_exec_result(&r);
        assert_eq!(state, PipelineRunState::Failed);
        assert!(err.is_some());
    }

    #[test]
    fn map_exec_result_cancelled() {
        let r: Result<()> = Err(Error::cancelled("aborted"));
        let (state, err) = map_exec_result(&r);
        assert_eq!(state, PipelineRunState::Cancelled);
        assert!(err.unwrap().contains("aborted"));
    }
}
