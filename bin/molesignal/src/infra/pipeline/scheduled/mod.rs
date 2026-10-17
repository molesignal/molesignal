// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ScheduledPipelineRunner（pipeline-runs-and-backfill）。
//!
//! 当前 cron 解析：仅支持 `every:Ns` 简化语法（每 N 秒一次），完整 cron 解析留
//! follow-up。alert role 内调 `tick_once`；按 last_run_at 与读取位点决定是否要触发。
//!
//! 每次运行读什么由读取位点决定（[`state`]）：每个源从上次读到的地方读到「现在减去等待迟到
//! 数据的延迟」，失败的窗口按退避重试，落后太多的源连续追赶。
//!
//! 多个 alert 节点按 pipeline id 分担：开跑前先过 [`RunGate`]（归属 + 运行租约），认领成功后
//! 重读 pipeline 再确认仍然到期，因此成员变化时两个节点不会把同一个周期各跑一遍。
//!
//! pipeline-runs-and-backfill：每次读了东西的运行都向 `pipeline_runs` 写一行，
//! `record_start` 进入 → 真实工作 → `record_finish` 退出。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;

use self::{
    repository::{ScheduledPipeline, ScheduledPipelineRepository},
    state::{RunState, SourceResult, Timing, Window},
};
use crate::{
    domain::{
        pipeline::{realtime::is_realtime, scheduled::ScheduledConfig},
        run_claim::{OpenGate, RunGate},
    },
    infra::persistence::repositories::pipelines::runs::{
        PipelineRun, PipelineRunFinish, PipelineRunRepository, PipelineRunState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "scheduled_pipeline";
/// A run's error and warning text is cut to this many characters.
const MAX_MESSAGE_CHARS: usize = 4_000;

mod configuration;
pub mod repository;
pub mod state;

/// How many events a run read, and where they ended: each read event is written, dropped on
/// purpose or failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCounts {
    pub scanned: i64,
    pub written: i64,
    pub dropped: i64,
    pub failed: i64,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    pub counts: RunCounts,
    /// What became of each source's window.
    pub sources: BTreeMap<String, SourceResult>,
    /// What went wrong without failing the run.
    pub warnings: Vec<String>,
    /// Why the run is reported failed although every window is done: a stream rejected every
    /// event it was given and there was no dead letter stream to keep them.
    pub failure: Option<String>,
}

impl RunReport {
    /// Every window done and nothing read: a run with nothing to do.
    pub fn done(windows: &[Window]) -> Self {
        Self {
            sources: windows
                .iter()
                .map(|window| (window.source.clone(), SourceResult::Done))
                .collect(),
            ..Self::default()
        }
    }
}

/// 注入式执行器：一次 pipeline run 的「读源窗口 → 函数链 → 写目标 stream → egress」。
/// infra 不依赖 app（`QueryService`），故「读源」由 bootstrap 实装注入。
/// 未注入时 [`ScheduledPipelineRunner::tick_once`] 仅推进位点与 schedule（便于测试）。
#[async_trait]
pub trait PipelineExecutor: Send + Sync {
    /// Reads `windows` of the pipeline's sources and runs the pipeline on what was read. `Err` is
    /// the pipeline's configuration failing before anything was read.
    async fn execute(&self, pipeline: &ScheduledPipeline, windows: &[Window]) -> Result<RunReport>;
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

    /// pipeline-runs-and-backfill：注入 runs repo 后，每次运行写一条 row。
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

    /// 注入真实执行器（bootstrap 实装，持 `QueryService`）。未注入时 tick 仅推进位点与 schedule。
    pub fn with_executor(mut self, executor: Arc<dyn PipelineExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// 与其余 alert 节点分担 pipeline：每个到期的 pipeline 开跑前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
    }

    /// 单次 tick：列所有 enabled，对到期且本节点获准运行的调 `run_pipeline`。返回读了东西的运行数。
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
                    if self.run_pipeline(&fresh, now).await {
                        fired += 1;
                    }
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

    /// 执行一次已到期的 pipeline：按位点定窗口 → 写 running 行 → 执行 → 推进位点 → 写终态。
    /// 没有可读的窗口时只推进 schedule，不写运行行；返回是否真的运行了。
    async fn run_pipeline(&self, p: &ScheduledPipeline, now: TimestampMicros) -> bool {
        tracing::debug!(pipeline_id = %p.id.0, name = %p.name, "scheduled pipeline fired");

        let state = RunState::from_value(&p.run_state);

        let (config, timing) = match schedule(p) {
            Ok(schedule) => schedule,
            Err(e) => {
                // 配置读不了：什么都不读，记一次失败的运行，位点原样留着等配置改好。
                let run_id = self.start_run(p, now, 1).await;
                self.finish_run(p, run_id, now, &state, 1, Err(e)).await;
                return true;
            }
        };

        let windows = state.windows(&config.sources, &timing, now.0);

        if windows.is_empty() {
            if let Err(e) = self.repo.record_run(&p.id, now, &p.run_state).await {
                tracing::warn!(pipeline_id = %p.id.0, error = %e, "scheduled pipeline record_run failed");
            }

            return false;
        }

        let attempt = windows
            .iter()
            .map(|window| window.attempt)
            .max()
            .unwrap_or(1);
        let run_id = self.start_run(p, now, attempt).await;

        let report = match &self.executor {
            Some(exec) => exec.execute(p, &windows).await,
            None => Ok(RunReport::done(&windows)),
        };

        let results = match &report {
            Ok(report) => report.sources.clone(),
            Err(e) => windows
                .iter()
                .map(|window| {
                    let failed = SourceResult::Failed {
                        done: window.done.clone(),
                        error: e.to_string(),
                        permanent: true,
                    };

                    (window.source.clone(), failed)
                })
                .collect(),
        };

        let (next, skipped) = state.after(
            &config.sources,
            &windows,
            &results,
            &timing,
            config.retry,
            now.0,
        );

        let report = report.map(|mut report| {
            report.warnings.extend(skipped);
            report
        });

        self.finish_run(p, run_id, now, &next, attempt, report)
            .await;
        true
    }

    /// 写入 running 行（如果有 runs repo 注入）。
    async fn start_run(
        &self,
        p: &ScheduledPipeline,
        now: TimestampMicros,
        attempt: u32,
    ) -> Option<Id> {
        let runs = self.runs.as_ref()?;
        let run_id = Id::new();

        let row = PipelineRun {
            id: run_id.clone(),
            pipeline_id: p.id.clone(),
            org_id: p.org_id.clone(),
            state: PipelineRunState::Running,
            started_at: now,
            finished_at: None,
            scanned_rows: 0,
            written_rows: 0,
            dropped_rows: 0,
            failed_rows: 0,
            attempt: i32::try_from(attempt).unwrap_or(i32::MAX),
            error: None,
            warning: None,
        };

        match runs.record_start(row).await {
            Ok(()) => Some(run_id),
            Err(e) => {
                tracing::warn!(error = %e, "pipeline_runs record_start failed");
                None
            }
        }
    }

    /// 存下运行后的位点（无论成败都推进 schedule，避免失败 pipeline 每 tick 热重试；重试另按
    /// 位点里的退避时间到期），再把运行行更新为终态。
    async fn finish_run(
        &self,
        p: &ScheduledPipeline,
        run_id: Option<Id>,
        now: TimestampMicros,
        next: &RunState,
        attempt: u32,
        report: Result<RunReport>,
    ) {
        if let Err(e) = self.repo.record_run(&p.id, now, &next.to_value()).await {
            tracing::warn!(pipeline_id = %p.id.0, error = %e, "scheduled pipeline record_run failed");
        }

        // 单个 pipeline 执行失败只记录，不中断本轮其余 pipeline。
        if let Err(e) = &report {
            tracing::warn!(pipeline_id = %p.id.0, error = %e, "scheduled pipeline execution failed");
        }

        if let (Some(runs), Some(run_id)) = (&self.runs, run_id) {
            let finish = finish_of(&report, next, attempt, TimestampMicros::now());

            if let Err(e) = runs.record_finish(&run_id, finish).await {
                tracing::warn!(error = %e, "pipeline_runs record_finish failed");
            }
        }
    }
}

/// What a pipeline reads and when, as saved. An error is a configuration no run can read.
fn schedule(p: &ScheduledPipeline) -> Result<(ScheduledConfig, Timing)> {
    let config = ScheduledConfig::parse(&p.function_steps, &p.source_stream, &p.target_stream)?;

    let every = parse_every_secs(&p.cron)
        .ok_or_else(|| Error::invalid("scheduled pipelines require every:Ns/m/h"))?;

    let timing = Timing::from_secs(every, i64::from(p.lookback_secs), config.delay_secs);
    Ok((config, timing))
}

/// 该 pipeline 在 `now` 是否到期：已启用、不是实时 pipeline，且距上次运行已满一个周期，或者有
/// 窗口的重试到了时候、有源落后到一次读不完。
fn is_due(p: &ScheduledPipeline, now: TimestampMicros) -> bool {
    if !p.enabled || is_realtime(&p.function_steps) {
        return false;
    }

    let Some(every) = parse_every_secs(&p.cron) else {
        return false; // 不支持的 cron 语法跳过
    };

    let regular = match p.last_run_at {
        Some(last) => now.0 - last.0 >= every * 1_000_000,
        None => true,
    };

    regular
        || schedule(p).is_ok_and(|(config, timing)| {
            RunState::from_value(&p.run_state).due_early(&config.sources, &timing, now.0)
        })
}

/// How a run ended, for its row. A run fails when a window failed (a retry may be planned), when a
/// stream took none of its events, or when the pipeline could not run at all.
pub(crate) fn finish_of(
    report: &Result<RunReport>,
    next: &RunState,
    attempt: u32,
    finished_at: TimestampMicros,
) -> PipelineRunFinish {
    let mut finish = PipelineRunFinish {
        state: PipelineRunState::Succeeded,
        finished_at,
        scanned_rows: 0,
        written_rows: 0,
        dropped_rows: 0,
        failed_rows: 0,
        attempt: i32::try_from(attempt).unwrap_or(i32::MAX),
        error: None,
        warning: None,
    };

    let report = match report {
        Ok(report) => report,
        Err(e) => {
            finish.state = if matches!(e, Error::Cancelled(_)) {
                PipelineRunState::Cancelled
            } else {
                PipelineRunState::Failed
            };

            finish.error = Some(clip(e.to_string()));
            return finish;
        }
    };

    finish.scanned_rows = report.counts.scanned;
    finish.written_rows = report.counts.written;
    finish.dropped_rows = report.counts.dropped;
    finish.failed_rows = report.counts.failed;

    let mut errors: Vec<String> = report
        .sources
        .iter()
        .filter_map(|(source, result)| match result {
            SourceResult::Failed { error, .. } => {
                let retry = next
                    .sources
                    .get(source)
                    .and_then(|state| state.retry.as_ref());

                Some(match retry {
                    Some(retry) => format!(
                        "`{source}`: {error} (attempt {} failed, trying again in {} s)",
                        retry.failures,
                        (retry.next_at - finished_at.0).max(0) / 1_000_000
                    ),
                    None => format!("`{source}`: {error}"),
                })
            }
            SourceResult::Done => None,
        })
        .collect();

    errors.extend(report.failure.clone());

    if !errors.is_empty() {
        finish.state = PipelineRunState::Failed;
        finish.error = Some(clip(errors.join("; ")));
    }

    if !report.warnings.is_empty() {
        finish.warning = Some(clip(report.warnings.join("; ")));
    }

    finish
}

fn clip(text: String) -> String {
    match text.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
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
    fn a_run_that_could_not_start_is_failed_or_cancelled() {
        let now = TimestampMicros(0);
        let failed = finish_of(&Err(Error::internal("boom")), &RunState::default(), 1, now);
        assert_eq!(failed.state, PipelineRunState::Failed);
        assert!(failed.error.unwrap().contains("boom"));

        let cancelled = finish_of(
            &Err(Error::cancelled("aborted")),
            &RunState::default(),
            1,
            now,
        );
        assert_eq!(cancelled.state, PipelineRunState::Cancelled);
        assert!(cancelled.error.unwrap().contains("aborted"));
    }

    #[test]
    fn a_run_reports_its_counts_and_fails_when_a_window_failed() {
        let now = TimestampMicros(1_000_000_000);
        let counts = RunCounts {
            scanned: 10,
            written: 6,
            dropped: 3,
            failed: 1,
        };
        let succeeded = RunReport {
            counts,
            sources: [("app".to_owned(), SourceResult::Done)].into(),
            warnings: vec!["connector `c1` took 1 event(s) late".into()],
            failure: None,
        };

        let finish = finish_of(&Ok(succeeded.clone()), &RunState::default(), 2, now);

        assert_eq!(finish.state, PipelineRunState::Succeeded);
        assert_eq!(
            (
                finish.scanned_rows,
                finish.written_rows,
                finish.dropped_rows,
                finish.failed_rows
            ),
            (10, 6, 3, 1)
        );
        assert_eq!(finish.attempt, 2);
        assert_eq!(finish.error, None);
        assert_eq!(
            finish.warning.as_deref(),
            Some("connector `c1` took 1 event(s) late")
        );

        // A window that failed, with its retry planned 30 s on.
        let mut failed = succeeded.clone();
        failed.sources.insert(
            "nginx".into(),
            SourceResult::Failed {
                done: Default::default(),
                error: "read: storage unavailable".into(),
                permanent: false,
            },
        );
        let mut next = RunState::default();
        next.sources.insert(
            "nginx".into(),
            state::SourceState {
                read_until: 0,
                retry: Some(state::Retry {
                    failures: 1,
                    next_at: now.0 + 30_000_000,
                    until: 0,
                    done: Default::default(),
                    error: String::new(),
                }),
                held: None,
            },
        );

        let finish = finish_of(&Ok(failed), &next, 1, now);

        assert_eq!(finish.state, PipelineRunState::Failed);
        assert_eq!(
            finish.error.as_deref(),
            Some("`nginx`: read: storage unavailable (attempt 1 failed, trying again in 30 s)")
        );

        // A stream that took none of the events fails the run although every window is done.
        let mut rejected = succeeded;
        rejected.failure = Some("target stream `out` rejected 2 event(s)".into());

        let finish = finish_of(&Ok(rejected), &RunState::default(), 1, now);

        assert_eq!(finish.state, PipelineRunState::Failed);
        assert_eq!(
            finish.error.as_deref(),
            Some("target stream `out` rejected 2 event(s)")
        );
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
            run_state: json!({}),
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
        async fn record_run(
            &self,
            id: &Id,
            ts: TimestampMicros,
            run_state: &serde_json::Value,
        ) -> Result<()> {
            if let Some(p) = self.fresh.lock().unwrap().iter_mut().find(|p| &p.id == id) {
                p.last_run_at = Some(ts);
                p.run_state = run_state.clone();
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingExecutor(AtomicUsize);

    #[async_trait]
    impl PipelineExecutor for CountingExecutor {
        async fn execute(
            &self,
            _pipeline: &ScheduledPipeline,
            windows: &[Window],
        ) -> Result<RunReport> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(RunReport::done(windows))
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

    /// Fails the first windows it is given, then reads every window.
    struct FlakyExecutor {
        failures_left: AtomicUsize,
        seen: Mutex<Vec<Vec<Window>>>,
    }

    #[async_trait]
    impl PipelineExecutor for FlakyExecutor {
        async fn execute(
            &self,
            _pipeline: &ScheduledPipeline,
            windows: &[Window],
        ) -> Result<RunReport> {
            self.seen.lock().unwrap().push(windows.to_vec());

            if self.failures_left.load(Ordering::SeqCst) == 0 {
                return Ok(RunReport::done(windows));
            }

            self.failures_left.fetch_sub(1, Ordering::SeqCst);

            Ok(RunReport {
                sources: windows
                    .iter()
                    .map(|window| {
                        let failed = SourceResult::Failed {
                            done: Default::default(),
                            error: "read: storage unavailable".into(),
                            permanent: false,
                        };

                        (window.source.clone(), failed)
                    })
                    .collect(),
                ..RunReport::default()
            })
        }
    }

    #[tokio::test]
    async fn a_failed_window_is_read_again_when_its_retry_is_due_and_then_moves_on() {
        let repo = Pipelines::new(vec![pipeline(None)]);
        let executor = Arc::new(FlakyExecutor {
            failures_left: AtomicUsize::new(1),
            seen: Mutex::new(Vec::new()),
        });
        let runner = ScheduledPipelineRunner::new(repo.clone()).with_executor(executor.clone());

        assert_eq!(runner.tick_once().await.unwrap(), 1);

        // The window kept its place and waits for its backoff, which has not passed yet.
        let state = RunState::from_value(&repo.fresh.lock().unwrap()[0].run_state);
        let (failures, next_at) = state.pending_retry().expect("a retry is planned");
        assert_eq!(failures, 1);
        assert!(next_at > TimestampMicros::now().0);
        assert_eq!(runner.tick_once().await.unwrap(), 0);

        // Once it is due, the same window is read again, and done this time.
        {
            let mut fresh = repo.fresh.lock().unwrap();
            let mut due = state.clone();
            due.sources
                .get_mut("raw")
                .unwrap()
                .retry
                .as_mut()
                .unwrap()
                .next_at = 0;
            fresh[0].run_state = due.to_value();
        }

        assert_eq!(runner.tick_once().await.unwrap(), 1);

        let seen = executor.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[1][0].start, seen[1][0].end),
            (seen[0][0].start, seen[0][0].end)
        );
        assert_eq!(seen[1][0].attempt, 2);
        let state = RunState::from_value(&repo.fresh.lock().unwrap()[0].run_state);
        assert_eq!(state.sources["raw"].read_until, seen[0][0].end);
        assert_eq!(state.pending_retry(), None);
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
