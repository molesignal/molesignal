// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ScheduledPipelineRunner（pipeline-runs-and-backfill）。
//!
//! 当前 cron 解析：仅支持 `every:Ns` 简化语法（每 N 秒一次），完整 cron 解析留
//! follow-up。alert role 内调 `tick_once`；按 last_run_at 决定是否要触发。
//!
//! 多个 alert 节点按 pipeline id 分担：开跑前先过 [`RunGate`]（归属 + 运行租约），认领成功后
//! 重读 pipeline 再确认仍然到期，因此成员变化时两个节点不会把同一个周期各跑一遍。
//!
//! pipeline-runs-and-backfill：每次 fire 都向 `pipeline_runs` 写一行，
//! `record_start` 进入 → 真实工作 → `record_finish` 退出。RAII guard 保证 panic
//! / error 也能更新 row（当前 stub 没有真正 fail 路径，但保留接口）。

use std::sync::Arc;

use async_trait::async_trait;

use self::repository::{ScheduledPipeline, ScheduledPipelineRepository};
use crate::{
    domain::run_claim::{OpenGate, RunGate},
    infra::persistence::repositories::pipelines::runs::{
        PipelineRun, PipelineRunRepository, PipelineRunState,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "scheduled_pipeline";

mod configuration;
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
    /// 多个 alert 节点分担 pipeline 时的准入；默认每个到期的 pipeline 都跑。
    gate: Arc<dyn RunGate>,
}

impl ScheduledPipelineRunner {
    /// 旧入口：未注入 runs repo 时所有 `record_*` 调用变成 no-op。便于测试。
    pub fn new(repo: Arc<dyn ScheduledPipelineRepository>) -> Self {
        Self {
            repo,
            runs: None,
            executor: None,
            gate: Arc::new(OpenGate),
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
            gate: Arc::new(OpenGate),
        }
    }

    /// 注入真实执行器（bootstrap 实装，持 `QueryService`）。未注入时 tick 仅推进 schedule。
    pub fn with_executor(mut self, executor: Arc<dyn PipelineExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// 与其余 alert 节点分担 pipeline：每个到期的 pipeline 开跑前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
    }

    /// 单次 tick：列所有 enabled，对到期且本节点获准运行的调 `run_pipeline`（当前仅打 metric +
    /// touch_last_run）。命中时同步写一条 `pipeline_runs` 行（state=running → 完成时更新为终态）。
    pub async fn tick_once(&self) -> Result<usize> {
        let pipelines = self.repo.list_enabled_all().await?;
        let now = TimestampMicros::now();
        let mut fired = 0usize;

        for p in pipelines {
            if !is_due(&p, now) {
                continue;
            }

            // 先认领再执行：归属决定谁来跑，运行租约兜住成员变化时的双跑。
            let permit = match self.gate.admit(RUN_KIND, p.id.as_str()).await {
                Ok(Some(permit)) => permit,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(pipeline_id = %p.id.0, error = %e, "pipeline run admission failed");
                    continue;
                }
            };

            // 认领后重读：另一个节点可能刚跑完并推进了 schedule，不能拿列表时的旧快照再跑一遍。
            match self.repo.get_by_id(&p.id).await {
                Ok(fresh) if is_due(&fresh, now) => {
                    self.run_pipeline(&fresh, now).await;
                    fired += 1;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(pipeline_id = %p.id.0, error = %e, "scheduled pipeline reload failed");
                }
            }

            if let Err(e) = permit.release().await {
                tracing::warn!(pipeline_id = %p.id.0, error = %e, "pipeline run claim release failed");
            }
        }

        Ok(fired)
    }

    /// 执行一次已到期的 pipeline：写 running 行 → 执行 → 推进 schedule → 写终态。
    async fn run_pipeline(&self, p: &ScheduledPipeline, now: TimestampMicros) {
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
            Some(exec) => exec.execute(p).await,
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
    }
}

/// 该 pipeline 在 `now` 是否到期：已启用、不是实时 pipeline，且距上次运行已满一个周期。
fn is_due(p: &ScheduledPipeline, now: TimestampMicros) -> bool {
    if !p.enabled || crate::domain::pipeline::realtime::is_realtime(&p.function_steps) {
        return false;
    }

    match parse_every_secs(&p.cron) {
        Some(secs) => match p.last_run_at {
            Some(last) => now.0 - last.0 >= secs * 1_000_000,
            None => true,
        },
        None => false, // 不支持的 cron 语法跳过
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

pub(crate) fn parse_every_secs(cron: &str) -> Option<i64> {
    // 支持 `every:60s` / `every:5m` / `every:1h`
    let s = cron.trim();
    let rest = s.strip_prefix("every:")?;
    let (num, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;

    if n <= 0 {
        return None;
    }

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return None,
    };

    n.checked_mul(multiplier)
        .filter(|seconds| seconds.checked_mul(1_000_000).is_some())
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

    // ───────────── 多个 alert 节点分担 pipeline：运行准入 ─────────────

    use std::{
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use serde_json::json;

    use crate::{
        app::cluster::{PeerRole, ShardedRunGate, WorkOwnership, ownership::test_support::Nodes},
        domain::run_claim::MemoryRunClaims,
    };

    fn pipeline(last_run_at: Option<TimestampMicros>) -> ScheduledPipeline {
        ScheduledPipeline {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "nightly".into(),
            source_stream: "raw".into(),
            target_stream: "clean".into(),
            function_steps: json!([]),
            cron: "every:60s".into(),
            lookback_secs: 300,
            last_run_at,
            enabled: true,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// `list_enabled_all` 返回给定的（可能过期的）快照，`get_by_id` 返回最新的一份。
    struct Pipelines {
        snapshot: Vec<ScheduledPipeline>,
        fresh: Mutex<Vec<ScheduledPipeline>>,
    }

    impl Pipelines {
        fn new(pipelines: Vec<ScheduledPipeline>) -> Arc<Self> {
            Arc::new(Self {
                snapshot: pipelines.clone(),
                fresh: Mutex::new(pipelines),
            })
        }
    }

    #[async_trait]
    impl ScheduledPipelineRepository for Pipelines {
        async fn create(&self, _p: ScheduledPipeline) -> Result<ScheduledPipeline> {
            unimplemented!()
        }
        async fn update(&self, _p: ScheduledPipeline) -> Result<ScheduledPipeline> {
            unimplemented!()
        }
        async fn delete(&self, _org: &Id, _id: &Id) -> Result<()> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: &Id) -> Result<ScheduledPipeline> {
            self.fresh
                .lock()
                .unwrap()
                .iter()
                .find(|p| &p.id == id)
                .cloned()
                .ok_or_else(|| Error::not_found("pipeline"))
        }
        async fn get(&self, _org: &Id, id: &Id) -> Result<ScheduledPipeline> {
            self.get_by_id(id).await
        }
        async fn list(&self, _org: &Id) -> Result<Vec<ScheduledPipeline>> {
            unimplemented!()
        }
        async fn list_enabled_all(&self) -> Result<Vec<ScheduledPipeline>> {
            Ok(self.snapshot.clone())
        }
        async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
            if let Some(p) = self.fresh.lock().unwrap().iter_mut().find(|p| &p.id == id) {
                p.last_run_at = Some(ts);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingExecutor(AtomicUsize);

    #[async_trait]
    impl PipelineExecutor for CountingExecutor {
        async fn execute(&self, _pipeline: &ScheduledPipeline) -> Result<u64> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }
    }

    fn runner(
        repo: Arc<Pipelines>,
        executor: &Arc<CountingExecutor>,
        gate: Arc<dyn RunGate>,
    ) -> ScheduledPipelineRunner {
        ScheduledPipelineRunner::new(repo)
            .with_executor(executor.clone())
            .with_gate(gate)
    }

    fn sharded(
        nodes: &Arc<Nodes>,
        node_id: &str,
        claims: &Arc<MemoryRunClaims>,
    ) -> Arc<dyn RunGate> {
        let ownership = WorkOwnership::new(nodes.clone(), node_id, PeerRole::Alert, Duration::ZERO);
        Arc::new(ShardedRunGate::new(ownership, claims.clone(), node_id))
    }

    #[tokio::test]
    async fn a_due_pipeline_runs_once_and_is_not_repeated_from_a_stale_listing() {
        let repo = Pipelines::new(vec![pipeline(None)]);
        let executor = Arc::new(CountingExecutor::default());
        let claims = Arc::new(MemoryRunClaims::default());
        let gate = sharded(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let runner = runner(repo, &executor, gate);

        assert_eq!(runner.tick_once().await.unwrap(), 1);
        // 快照仍显示「从未运行」，认领后重读发现这个周期已经跑过：不会再跑一遍。
        assert_eq!(runner.tick_once().await.unwrap(), 0);
        assert_eq!(executor.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_pipeline_held_by_another_node_is_not_executed() {
        let due = pipeline(None);
        let repo = Pipelines::new(vec![due.clone()]);
        let executor = Arc::new(CountingExecutor::default());
        let claims = Arc::new(MemoryRunClaims::default());
        // 两个节点各自只看到自己，都认为自己拥有这条 pipeline。
        let theirs = sharded(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let mine = sharded(&Nodes::of(&[("b", PeerRole::Alert)]), "b", &claims);
        let _running_on_a = theirs
            .admit(RUN_KIND, due.id.as_str())
            .await
            .unwrap()
            .expect("a claims the pipeline");
        let runner = runner(repo, &executor, mine);

        assert_eq!(runner.tick_once().await.unwrap(), 0);
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn each_pipeline_is_executed_by_exactly_one_node() {
        let repo = Pipelines::new((0..20).map(|_| pipeline(None)).collect());
        let nodes = Nodes::of(&[("a", PeerRole::Alert), ("b", PeerRole::Alert)]);
        let claims = Arc::new(MemoryRunClaims::default());
        let (exec_a, exec_b) = (
            Arc::new(CountingExecutor::default()),
            Arc::new(CountingExecutor::default()),
        );
        let on_a = runner(repo.clone(), &exec_a, sharded(&nodes, "a", &claims));
        let on_b = runner(repo, &exec_b, sharded(&nodes, "b", &claims));

        let (ran_a, ran_b) = (
            on_a.tick_once().await.unwrap(),
            on_b.tick_once().await.unwrap(),
        );

        assert!(ran_a > 0 && ran_b > 0, "{ran_a} + {ran_b}");
        assert_eq!(ran_a + ran_b, 20);
        assert_eq!(
            exec_a.0.load(Ordering::SeqCst) + exec_b.0.load(Ordering::SeqCst),
            20
        );
    }

    #[test]
    fn disabled_and_realtime_pipelines_are_never_due() {
        let now = TimestampMicros::now();
        let mut disabled = pipeline(None);
        disabled.enabled = false;
        assert!(!is_due(&disabled, now));

        let mut realtime = pipeline(None);
        realtime.function_steps = json!({ "mode": "realtime" });
        assert!(!is_due(&realtime, now));

        let ran_just_now = pipeline(Some(now));
        assert!(!is_due(&ran_just_now, now));
        assert!(is_due(&pipeline(None), now));
    }
}
