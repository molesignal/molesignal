// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! feature-parity follow-up：SearchJob worker pool。
//!
//! 模型：
//! 1. `claim_next_pending` 走 PG `FOR UPDATE SKIP LOCKED` 多 worker 安全抢占；
//! 2. 命中 → 反序列化 `request_json` → `QueryService::run`；
//! 3. 成功 → 结果以 NDJSON 形态落 object_store，路径 `<org>/search_jobs/<job_id>.ndjson`，
//!    `mark_done(object_key, rows, finished_at)`；
//! 4. 失败 → `mark_failed(error, finished_at)`；
//! 5. cleanup task 每小时一次：扫 `expires_at < now` 的 job → 删 object → 删 row。
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
use tokio::task::JoinHandle;
use tracing::Instrument;

use super::pipeline_exec::{rows_to_objects, transform_and_sink};
use crate::{
    app::query::QueryService,
    domain::{
        ingestion::IngestSink,
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
    /// worker 数量（建议 = querier 节点数 × 1~2）
    pub workers: usize,
    /// 无 pending job 时的轮询间隔
    pub idle_poll_secs: u64,
    /// cleanup 周期
    pub cleanup_interval_secs: u64,
}

impl Default for SearchJobSchedulerConfig {
    fn default() -> Self {
        Self {
            workers: 2,
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
    ingest_sink: Arc<dyn IngestSink>,
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
        ingest_sink: Arc<dyn IngestSink>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
        cfg: SearchJobSchedulerConfig,
    ) -> Self {
        Self {
            repo,
            query,
            object_store,
            scheduled_pipelines,
            ingest_sink,
            connectors,
            dispatcher,
            cfg,
        }
    }

    /// spawn worker pool + cleanup loop；返回所有 handle。
    pub fn spawn(self: Arc<Self>) -> Vec<JoinHandle<()>> {
        let n = self.cfg.workers.max(1);
        let mut handles = Vec::with_capacity(n + 1);
        for i in 0..n {
            let me = self.clone();
            handles.push(tokio::spawn(async move { me.run_worker(i).await }));
        }
        let me = self.clone();
        handles.push(tokio::spawn(async move { me.run_cleanup().await }));
        handles
    }

    async fn run_worker(self: Arc<Self>, worker_id: usize) {
        let idle = Duration::from_secs(self.cfg.idle_poll_secs.max(1));
        loop {
            match self.repo.claim_next_pending().await {
                Ok(Some(job)) => {
                    let job_id = job.id.clone();
                    tracing::info!(worker = worker_id, job_id = %job_id.0, "claimed search job");
                    if let Err(e) = self.process(job).await {
                        tracing::warn!(worker = worker_id, job_id = %job_id.0, error = %e, "search job failed");
                    }
                }
                Ok(None) => tokio::time::sleep(idle).await,
                Err(e) => {
                    tracing::warn!(worker = worker_id, error = %e, "claim_next_pending failed");
                    tokio::time::sleep(idle).await;
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
                    .mark_failed(&job.id, &format!("decode request: {e}"), finished)
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

        let query_result = if pipeline_id.is_some() {
            // Backfill 仍要继续执行 VRL、写入目标流并触发 egress，必须使用原始值；
            // 普通异步搜索则在持久化结果前完成最终返回边界遮掩。
            self.query.run_raw(req).await
        } else {
            self.query.run(req).await
        };
        let result = match query_result {
            Ok(r) => r,
            Err(e) => {
                self.repo
                    .mark_failed(&job.id, &e.to_string(), TimestampMicros::now())
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
        let key = format!("{}/search_jobs/{}.ndjson", job.org_id.0, job.id.0);
        let bytes = encode_ndjson(result);
        let rows = result.rows.len() as i64;
        self.object_store
            .put(
                &Path::from(key.clone()),
                PutPayload::from(Bytes::from(bytes)),
            )
            .await
            .map_err(|e| crate::shared::Error::internal(format!("upload result: {e}")))?;
        self.repo
            .mark_done(&job.id, &key, rows, TimestampMicros::now())
            .await?;
        Ok(())
    }

    /// backfill 端到端：读源结果 → pipeline 的 VRL 步骤链 → 写目标 stream（标准 ingest）→
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
                        &job.id,
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
                .mark_failed(&job.id, &error.to_string(), TimestampMicros::now())
                .await?;
            return Ok(());
        }

        let source_rows = rows_to_objects(&result);
        let vrl = VrlRuntime::new();
        let outcome = match transform_and_sink(
            &vrl,
            self.ingest_sink.as_ref(),
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
                    .mark_failed(&job.id, &e.to_string(), TimestampMicros::now())
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

        let key = format!("{}/search_jobs/{}.ndjson", job.org_id.0, job.id.0);
        let bytes = encode_objects_ndjson(&outcome.transformed);
        self.object_store
            .put(
                &Path::from(key.clone()),
                PutPayload::from(Bytes::from(bytes)),
            )
            .await
            .map_err(|e| crate::shared::Error::internal(format!("upload result: {e}")))?;
        self.repo
            .mark_done(
                &job.id,
                &key,
                outcome.written as i64,
                TimestampMicros::now(),
            )
            .await?;
        Ok(())
    }

    async fn run_cleanup(self: Arc<Self>) {
        let interval = Duration::from_secs(self.cfg.cleanup_interval_secs.max(60));
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = self.cleanup_once().await {
                tracing::warn!(error = %e, "search_jobs cleanup failed");
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
            self.repo.delete(&j.id).await?;
        }
        Ok(())
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
