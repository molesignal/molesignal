// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search Job single-poller scheduler with bounded execution concurrency.
//!
//! 模型：
//! 1. 每个进程只有一个 poller，`claim_next_pending` 走 PG `FOR UPDATE SKIP LOCKED`；
//! 2. PostgreSQL NOTIFY / 进程内 Notify 唤醒空闲 poller，退避 poll 只做丢通知兜底；
//! 3. poller 抢到任务后放入有界 JoinSet 并发执行，不产生重复轮询；
//! 4. 命中 → 反序列化 `request_json` → `QueryService::run`；
//! 5. 成功 → 结果以 NDJSON 形态落 object_store，路径 `<org>/search_jobs/<job_id>.ndjson`，
//!    `mark_done(attempt, object_key, rows, finished_at)`；
//! 6. 失败 → `mark_failed(attempt, error, finished_at)`；
//! 7. cleanup task 周期扫描 `expires_at < now` 的 job → 删 object → 删 row。
//!
//! Parquet 输出 / DataFusion `WriterCommand` 留 follow-up；NDJSON 一是简单，
//! 二是 `result_object_key` 客户端可直接拉走解码。
//!
//! 这一层放在 bootstrap 而不是 app：app crate 不允许依赖 infra（架构约束），
//! 而 worker 强依赖 `SearchJobRepository` 的 infra 实装与 `ObjectStore`。

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use serde_json::Value;
use tokio::task::{JoinHandle, JoinSet};
use tracing::Instrument;

use super::{
    pipeline_exec::{rows_to_objects, transform_and_sink},
    polling::PollingBackoff,
};
use crate::{
    app::query::QueryService,
    domain::{
        intake::IntakeSink,
        query::{QueryRequest, QueryResult},
    },
    infra::{
        connectors::{ConnectorDispatcher, ConnectorRepository},
        persistence::repositories::search::jobs::{SearchJob, SearchJobRepository},
        pipeline::{
            ScheduledPipelineRepository,
            exec::{parse_signal_type, validate_pipeline_streams},
        },
        runtime::VrlRuntime,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct SearchJobSchedulerConfig {
    /// Per-process maximum number of Search Jobs executing concurrently.
    pub max_concurrent_jobs: usize,
    /// Initial fallback poll delay when no enqueue notification is received.
    pub idle_poll_secs: u64,
    /// cleanup 周期
    pub cleanup_interval_secs: u64,
}

impl Default for SearchJobSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 2,
            idle_poll_secs: 2,
            cleanup_interval_secs: 3600,
        }
    }
}

pub struct SearchJobScheduler {
    repo: Arc<dyn SearchJobRepository>,
    query: Arc<QueryService>,
    object_store: Arc<dyn ObjectStore>,
    /// backfill 任务（request_json 带 `pipeline_id`）执行编排需要的依赖：取 pipeline 定义、
    /// 写目标 stream、egress connector。
    scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    intake_sink: Arc<dyn IntakeSink>,
    connectors: Arc<dyn ConnectorRepository>,
    dispatcher: Arc<dyn ConnectorDispatcher>,
    cfg: SearchJobSchedulerConfig,
}

impl SearchJobScheduler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo: Arc<dyn SearchJobRepository>,
        query: Arc<QueryService>,
        object_store: Arc<dyn ObjectStore>,
        scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
        intake_sink: Arc<dyn IntakeSink>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
        cfg: SearchJobSchedulerConfig,
    ) -> Self {
        Self {
            repo,
            query,
            object_store,
            scheduled_pipelines,
            intake_sink,
            connectors,
            dispatcher,
            cfg,
        }
    }

    /// Spawn one claim poller plus one cleanup loop.
    pub fn spawn(self: Arc<Self>) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::with_capacity(2);
        let me = self.clone();
        handles.push(tokio::spawn(async move { me.run_poller().await }));
        let me = self.clone();
        handles.push(tokio::spawn(async move { me.run_cleanup().await }));
        handles
    }

    async fn run_poller(self: Arc<Self>) {
        let max_concurrent = self.cfg.max_concurrent_jobs.max(1);
        let base_idle = Duration::from_secs(self.cfg.idle_poll_secs.max(1));
        let max_idle = base_idle.saturating_mul(16).min(Duration::from_secs(60));
        let mut backoff = PollingBackoff::new(base_idle, max_idle, "search-jobs-poller");
        let mut running = JoinSet::new();
        tracing::info!(max_concurrent, "search job scheduler started");

        loop {
            while running.len() < max_concurrent {
                match self.repo.claim_next_pending().await {
                    Ok(Some(job)) => {
                        backoff.reset();
                        let job_id = job.id.clone();
                        let scheduler = self.clone();
                        tracing::info!(
                            job_id = %job_id.0,
                            active = running.len() + 1,
                            max_concurrent,
                            "claimed search job"
                        );
                        running.spawn(async move {
                            if let Err(error) = scheduler.process(job).await {
                                tracing::warn!(
                                    job_id = %job_id.0,
                                    %error,
                                    "search job failed"
                                );
                            }
                        });
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "claim_next_pending failed");
                        break;
                    }
                }
            }

            if running.len() >= max_concurrent {
                observe_job_completion(running.join_next().await);
                continue;
            }

            let delay = backoff.next_delay();
            if running.is_empty() {
                let _ = self.repo.wait_for_pending(delay).await;
            } else {
                tokio::select! {
                    _ = self.repo.wait_for_pending(delay) => {}
                    completed = running.join_next() => {
                        observe_job_completion(completed);
                    }
                }
            }
        }
    }

    async fn process(&self, job: SearchJob) -> Result<()> {
        let (context, span) = crate::shared::trace_context::linked_execution_root(
            job.trace_link.as_ref(),
            "search_job",
        );
        crate::shared::trace_context::with_current_trace_context(
            context,
            self.process_inner(job).instrument(span),
        )
        .await
    }

    async fn process_inner(&self, job: SearchJob) -> Result<()> {
        let req: QueryRequest = match serde_json::from_value(job.request_json.clone()) {
            Ok(r) => r,
            Err(e) => {
                let finished = TimestampMicros::now();
                self.repo
                    .mark_failed(
                        &job.org_id,
                        &job.id,
                        job.attempt,
                        &format!("decode request: {e}"),
                        finished,
                    )
                    .await?;
                return Ok(());
            }
        };

        // backfill 任务（submit_backfill 提交）在 request_json 里带 `pipeline_id`：读源后
        // 还要跑「VRL 变换 → 写目标 stream → connector egress」，而非仅把结果落 NDJSON。
        let pipeline_id = job
            .request_json
            .get("pipeline_id")
            .and_then(Value::as_str)
            .map(str::to_string);

        let query_result = self
            .query
            .run_search_job(
                req,
                job.user_id.clone(),
                &job.role_key,
                job.id.clone(),
                job.attempt,
                pipeline_id.is_some(),
            )
            .await;
        let result = match query_result {
            Ok(r) => r,
            Err(e) => {
                self.repo
                    .mark_failed(
                        &job.org_id,
                        &job.id,
                        job.attempt,
                        &e.to_string(),
                        TimestampMicros::now(),
                    )
                    .await?;
                return Ok(());
            }
        };

        match pipeline_id {
            Some(pid) => self.run_backfill(&job, &pid, result).await,
            None => self.store_result(&job, &result).await,
        }
    }

    /// 普通 search-job：结果以 NDJSON 落 object_store + mark_done。
    async fn store_result(&self, job: &SearchJob, result: &QueryResult) -> Result<()> {
        let key = format!(
            "{}/search_jobs/{}-{}.ndjson",
            job.org_id.0, job.id.0, job.attempt
        );
        let bytes = encode_ndjson(result);
        let rows = result.rows.len() as i64;
        self.object_store
            .put(
                &Path::from(key.clone()),
                PutPayload::from(Bytes::from(bytes)),
            )
            .await
            .map_err(|e| crate::shared::Error::internal(format!("upload result: {e}")))?;
        let committed = self
            .repo
            .mark_done(
                &job.org_id,
                &job.id,
                job.attempt,
                &key,
                rows,
                TimestampMicros::now(),
            )
            .await?;
        if !committed {
            let _ = self.object_store.delete(&Path::from(key)).await;
        }
        Ok(())
    }

    /// backfill 端到端：读源结果 → pipeline 的 VRL 步骤链 → 写目标 stream（标准 intake）→
    /// egress。变换后的产出同样落 NDJSON 供 monitor 拉取；`mark_done` 记写入目标的行数。
    async fn run_backfill(
        &self,
        job: &SearchJob,
        pipeline_id: &str,
        result: QueryResult,
    ) -> Result<()> {
        let pipeline = match self
            .scheduled_pipelines
            .get(&job.org_id, &Id(pipeline_id.to_string()))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                self.repo
                    .mark_failed(
                        &job.org_id,
                        &job.id,
                        job.attempt,
                        &format!("load pipeline: {e}"),
                        TimestampMicros::now(),
                    )
                    .await?;
                return Ok(());
            }
        };

        let stream_type = parse_signal_type(&pipeline.function_steps);
        if let Err(error) = validate_pipeline_streams(
            &pipeline.source_stream,
            &pipeline.target_stream,
            stream_type,
        ) {
            self.repo
                .mark_failed(
                    &job.org_id,
                    &job.id,
                    job.attempt,
                    &error.to_string(),
                    TimestampMicros::now(),
                )
                .await?;
            return Ok(());
        }

        let source_rows = rows_to_objects(&result);
        let vrl = VrlRuntime::new();
        let outcome = match transform_and_sink(
            &vrl,
            self.intake_sink.as_ref(),
            self.connectors.as_ref(),
            self.dispatcher.as_ref(),
            &job.org_id,
            &pipeline.target_stream,
            stream_type,
            &pipeline.function_steps,
            source_rows,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                self.repo
                    .mark_failed(
                        &job.org_id,
                        &job.id,
                        job.attempt,
                        &e.to_string(),
                        TimestampMicros::now(),
                    )
                    .await?;
                return Ok(());
            }
        };

        if !outcome.errors.is_empty() {
            tracing::warn!(
                job_id = %job.id.0,
                pipeline_id,
                scanned = outcome.scanned,
                written = outcome.written,
                errors = ?outcome.errors,
                "backfill completed with non-fatal errors"
            );
        }

        let key = format!(
            "{}/search_jobs/{}-{}.ndjson",
            job.org_id.0, job.id.0, job.attempt
        );
        let bytes = encode_objects_ndjson(&outcome.transformed);
        self.object_store
            .put(
                &Path::from(key.clone()),
                PutPayload::from(Bytes::from(bytes)),
            )
            .await
            .map_err(|e| crate::shared::Error::internal(format!("upload result: {e}")))?;
        let committed = self
            .repo
            .mark_done(
                &job.org_id,
                &job.id,
                job.attempt,
                &key,
                outcome.written as i64,
                TimestampMicros::now(),
            )
            .await?;
        if !committed {
            let _ = self.object_store.delete(&Path::from(key)).await;
        }
        Ok(())
    }

    async fn run_cleanup(self: Arc<Self>) {
        let interval = Duration::from_secs(self.cfg.cleanup_interval_secs.max(60));
        let max_delay = interval.saturating_mul(8);
        let mut backoff = PollingBackoff::new(interval, max_delay, "search-jobs-cleanup");
        loop {
            tokio::time::sleep(backoff.next_delay()).await;
            match self.cleanup_once().await {
                Ok(()) => backoff.reset(),
                Err(error) => {
                    tracing::warn!(%error, "search_jobs cleanup failed");
                }
            }
        }
    }

    #[tracing::instrument(
        name = "worker.search_job_cleanup",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "search_job_cleanup")
    )]
    async fn cleanup_once(&self) -> Result<()> {
        let now = TimestampMicros::now();
        let expired = self.repo.list_expired(now, 1000).await?;
        for j in expired {
            if let Some(key) = j.result_object_key.as_deref() {
                let _ = self.object_store.delete(&Path::from(key.to_string())).await;
            }
            self.repo.delete(&j.org_id, &j.id).await?;
        }
        Ok(())
    }
}

fn observe_job_completion(completed: Option<std::result::Result<(), tokio::task::JoinError>>) {
    if let Some(Err(error)) = completed {
        tracing::error!(%error, "search job execution task terminated unexpectedly");
    }
}

fn encode_ndjson(result: &QueryResult) -> Vec<u8> {
    let mut out = Vec::with_capacity(result.rows.len() * 128);
    for row in &result.rows {
        let mut obj = serde_json::Map::with_capacity(result.columns.len());
        for (i, col) in result.columns.iter().enumerate() {
            obj.insert(
                col.clone(),
                row.get(i).cloned().unwrap_or(serde_json::Value::Null),
            );
        }
        if let Ok(line) = serde_json::to_string(&serde_json::Value::Object(obj)) {
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
    }
    out
}

/// 变换后的事件（已是 JSON 对象）→ NDJSON（每行一个对象）。backfill 产出落库复用。
fn encode_objects_ndjson(events: &[Value]) -> Vec<u8> {
    let mut out = Vec::with_capacity(events.len() * 128);
    for ev in events {
        if let Ok(line) = serde_json::to_string(ev) {
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_ndjson_emits_rows() {
        let r = QueryResult {
            columns: vec!["a".into(), "b".into()],
            rows: vec![
                vec![serde_json::json!(1), serde_json::json!("x")],
                vec![serde_json::json!(2), serde_json::json!("y")],
            ],
            scanned_rows: 2,
            took_ms: 5,
            federation: None,
        };
        let s = String::from_utf8(encode_ndjson(&r)).unwrap();
        assert_eq!(s, "{\"a\":1,\"b\":\"x\"}\n{\"a\":2,\"b\":\"y\"}\n");
    }
}
