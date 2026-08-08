// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled-pipeline 执行编排（pipeline-runs-and-backfill）。
//!
//! 端到端链路：**读源 stream → 逐步 VRL 变换 → 写目标 stream（标准 ingest）→ connector egress**。
//! 「读源」一环由调用方（持 [`crate::app::query::QueryService`]，app 层）完成后，把行喂进
//! [`transform_and_sink`]；本模块只承担 infra 侧的「变换 + 写目标 + egress」，因此可脱离 HTTP /
//! 调度时序单测。backfill worker（[`super::search_jobs`]）即调用此核心；未来的 cron runner 可复用。
//!
//! 与 [`crate::infra::pipeline::exec`] 的分工：后者是纯计算（解析 steps + 串行 VRL），本模块
//! 把它接到真实的 `IngestSink` 与 connector egress 上。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::{
    app::query::QueryService,
    domain::{
        ingestion::{IngestBatch, IngestSink, RawEvent},
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    infra::{
        connectors::{ConnectorDispatcher, ConnectorRepository},
        pipeline::{
            PipelineExecutor, ScheduledPipeline, ScheduledPipelineRunner,
            exec::{
                apply_steps, parse_signal_type, parse_sink_connectors, parse_steps,
                validate_pipeline_streams,
            },
        },
        runtime::VrlRuntime,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// 一次执行的产出：统计 + 变换后的事件（供 monitor NDJSON / 落库 / egress 复用）。
#[derive(Debug)]
pub struct PipelineRunOutcome {
    /// 读到的源行数。
    pub scanned: usize,
    /// 成功写入目标 stream 的事件数（= 通过全部 VRL 步骤的行数）。
    pub written: usize,
    /// 变换后的事件（也用于 egress 投递与 monitor 落 NDJSON）。
    pub transformed: Vec<Value>,
    /// 非致命错误（单条事件 VRL 运行失败、单个 connector egress 失败等），不阻断其余。
    pub errors: Vec<String>,
}

/// `QueryResult` 的列式行（`Vec<Vec<Value>>` + 列名）→ 每行一个 JSON 对象（列名→值）。
pub fn rows_to_objects(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::with_capacity(result.columns.len());
            for (i, col) in result.columns.iter().enumerate() {
                obj.insert(col.clone(), row.get(i).cloned().unwrap_or(Value::Null));
            }
            Value::Object(obj)
        })
        .collect()
}

/// 把读到的源行串行应用 `function_steps`（VRL）→ 写 `target_stream`（标准 ingest）→ 按
/// `sink_connectors` 向外部 connector egress。
///
/// 失败语义：任一步 VRL **编译**失败 = pipeline 配置错误 → 整批失败（返 `Err`，调用方 mark_failed）。
/// 单条事件 VRL **运行**失败 → 丢弃该事件、记入 `errors`；单个 connector egress 失败 → 记入
/// `errors`，均不中断其余处理。
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "pipeline.transform_and_sink",
    skip_all,
    fields(
        otel.kind = "internal",
        molesignal.pipeline.input_rows = source_rows.len(),
        molesignal.stream.type = ?target_stream_type
    )
)]
pub async fn transform_and_sink(
    vrl: &VrlRuntime,
    sink: &dyn IngestSink,
    connectors: &dyn ConnectorRepository,
    dispatcher: &dyn ConnectorDispatcher,
    org_id: &Id,
    target_stream: &str,
    target_stream_type: StreamType,
    function_steps: &Value,
    source_rows: Vec<Value>,
) -> Result<PipelineRunOutcome> {
    let scanned = source_rows.len();
    let steps = parse_steps(function_steps);

    // 预编译校验：编译失败 = 配置错误 → 整批失败（区别于下面 apply_steps 里逐条的运行错误）。
    for step in &steps {
        vrl.compile(&step.script)
            .map_err(|e| Error::invalid(format!("step `{}` compile: {e}", step.name)))?;
    }

    let (transformed, mut errors) = apply_steps(vrl, &steps, source_rows);

    // 写目标 stream（标准 ingest 端口）。
    let now = TimestampMicros::now();
    let events: Vec<RawEvent> = transformed.iter().map(|v| to_raw_event(v, now)).collect();
    let written = events.len();
    if written > 0 {
        sink.write(IngestBatch {
            batch_id: Id::new(),
            org_id: org_id.clone(),
            stream: target_stream.to_string(),
            stream_type: target_stream_type,
            events,
            received_at: now,
        })
        .await?;
    }

    // egress 到声明的 sink connectors（best-effort：逐个失败只记录，不阻断写目标的成功）。
    for cid in parse_sink_connectors(function_steps) {
        match connectors.get(org_id, &Id(cid.clone())).await {
            Ok(conn) => {
                if let Err(e) = dispatcher.dispatch(&conn, &transformed).await {
                    errors.push(format!("egress `{cid}`: {e}"));
                }
            }
            Err(e) => errors.push(format!("connector `{cid}` load: {e}")),
        }
    }

    Ok(PipelineRunOutcome {
        scanned,
        written,
        transformed,
        errors,
    })
}

/// 一行变换后的 JSON 对象 → `RawEvent`：从常见时间字段取微秒时间戳，缺失则用 `fallback`。
fn to_raw_event(value: &Value, fallback: TimestampMicros) -> RawEvent {
    let fields = match value {
        Value::Object(map) => map.clone(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("value".to_string(), other.clone());
            m
        }
    };
    let timestamp = fields
        .get("_timestamp")
        .or_else(|| fields.get("timestamp"))
        .and_then(Value::as_i64)
        .map(TimestampMicros)
        .unwrap_or(fallback);
    RawEvent { timestamp, fields }
}

/// [`PipelineExecutor`] 的 bootstrap 实装：读源（`QueryService`，按 `lookback_secs` 窗口
/// 对 source_stream 跑 `SELECT *`）→ 变换 + 写目标 + egress（[`transform_and_sink`]）。
/// cron runner（`ScheduledPipelineRunner`）与 backfill worker 共用同一编排核心。
pub struct BootstrapPipelineExecutor {
    query: Arc<QueryService>,
    sink: Arc<dyn IngestSink>,
    connectors: Arc<dyn ConnectorRepository>,
    dispatcher: Arc<dyn ConnectorDispatcher>,
}

impl BootstrapPipelineExecutor {
    pub fn new(
        query: Arc<QueryService>,
        sink: Arc<dyn IngestSink>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
    ) -> Self {
        Self {
            query,
            sink,
            connectors,
            dispatcher,
        }
    }
}

#[async_trait]
impl PipelineExecutor for BootstrapPipelineExecutor {
    #[tracing::instrument(
        name = "worker.scheduled_pipeline",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "scheduled_pipeline")
    )]
    async fn execute(&self, pipeline: &ScheduledPipeline) -> Result<u64> {
        let now = TimestampMicros::now();
        let lookback_micros = i64::from(pipeline.lookback_secs.max(0)) * 1_000_000;
        let stream_type = parse_signal_type(&pipeline.function_steps);
        validate_pipeline_streams(
            &pipeline.source_stream,
            &pipeline.target_stream,
            stream_type,
        )?;
        let req = QueryRequest {
            org_id: pipeline.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: format!("SELECT * FROM {}", pipeline.source_stream),
            time_range: TimeRange::new(TimestampMicros(now.0 - lookback_micros), now),
            stream: Some(StreamHint {
                name: pipeline.source_stream.clone(),
                stream_type,
            }),
            limit: None,
            federation_clusters: Vec::new(),
        };
        let result = self.query.run_raw(req).await?;
        let source_rows = rows_to_objects(&result);
        let vrl = VrlRuntime::new();
        let outcome = transform_and_sink(
            &vrl,
            self.sink.as_ref(),
            self.connectors.as_ref(),
            self.dispatcher.as_ref(),
            &pipeline.org_id,
            &pipeline.target_stream,
            stream_type,
            &pipeline.function_steps,
            source_rows,
        )
        .await?;
        Ok(outcome.written as u64)
    }
}

/// 起 cron 轮询循环：每 `poll_secs` 调一次 `runner.tick_once`（runner 内按各 pipeline 的
/// cron + last_run_at 判定是否真的 fire）。调用方据角色（alert_manager / standalone）决定是否 spawn。
pub fn spawn_runner(runner: Arc<ScheduledPipelineRunner>, poll_secs: u64) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(poll_secs.max(1)));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = runner.tick_once().await {
                tracing::warn!(error = %e, "scheduled_pipelines tick failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::infra::{connectors::Connector, ingest_sink::MemoryIngestSink};

    /// 只实现 `get`（egress 路径用到的唯一方法）；其余方法测试不触达。
    struct OneConnectorRepo(Connector);

    #[async_trait]
    impl ConnectorRepository for OneConnectorRepo {
        async fn create(&self, _c: Connector) -> Result<Connector> {
            unimplemented!()
        }
        async fn update(&self, _c: Connector) -> Result<Connector> {
            unimplemented!()
        }
        async fn touch_last_run(&self, _id: &Id, _ts: TimestampMicros) -> Result<()> {
            unimplemented!()
        }
        async fn get(&self, _org_id: &Id, _id: &Id) -> Result<Connector> {
            Ok(self.0.clone())
        }
        async fn delete(&self, _org_id: &Id, _id: &Id) -> Result<()> {
            unimplemented!()
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<Connector>> {
            unimplemented!()
        }
        async fn list_enabled(&self) -> Result<Vec<Connector>> {
            unimplemented!()
        }
        async fn find_for_push(&self, _kind: &str, _push_token: &str) -> Result<Option<Connector>> {
            unimplemented!()
        }
    }

    /// 记录所有被 dispatch 的事件，断言 egress 实际发生。
    #[derive(Default)]
    struct CapturingDispatcher {
        seen: Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl ConnectorDispatcher for CapturingDispatcher {
        async fn dispatch(&self, _connector: &Connector, events: &[Value]) -> Result<()> {
            self.seen.lock().unwrap().extend_from_slice(events);
            Ok(())
        }
    }

    fn never_called_repo() -> OneConnectorRepo {
        OneConnectorRepo(sample_connector())
    }

    fn sample_connector() -> Connector {
        Connector {
            id: Id("c1".into()),
            org_id: Id("o1".into()),
            name: "sink".into(),
            kind: "s3".into(),
            config_json: json!({}),
            enabled: true,
            last_run_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[tokio::test]
    async fn transforms_then_writes_to_sink() {
        let vrl = VrlRuntime::new();
        let sink = MemoryIngestSink::new();
        let dispatcher = CapturingDispatcher::default();
        let connectors = never_called_repo();
        let steps = json!({ "steps": [{ "transform_name": "tag", "script": ".count = 1" }] });
        let rows = vec![json!({ "msg": "a" }), json!({ "msg": "b" })];

        let outcome = transform_and_sink(
            &vrl,
            &sink,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "logs_5m",
            StreamType::Logs,
            &steps,
            rows,
        )
        .await
        .unwrap();

        assert_eq!(outcome.scanned, 2);
        assert_eq!(outcome.written, 2);
        assert!(outcome.errors.is_empty());

        let batches = sink.drain();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].stream, "logs_5m");
        assert_eq!(batches[0].events.len(), 2);
        // VRL `.count = 1` 应已写进每个事件。
        assert_eq!(batches[0].events[0].fields.get("count"), Some(&json!(1)));
        // 无 sink_connectors → egress 未触达。
        assert!(dispatcher.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn egresses_to_declared_connectors() {
        let vrl = VrlRuntime::new();
        let sink = MemoryIngestSink::new();
        let dispatcher = CapturingDispatcher::default();
        let connectors = OneConnectorRepo(sample_connector());
        let steps = json!({
            "steps": [{ "transform_name": "noop", "script": ".kept = true" }],
            "sink_connectors": ["c1"],
        });
        let rows = vec![json!({ "msg": "x" })];

        let outcome = transform_and_sink(
            &vrl,
            &sink,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "out",
            StreamType::Logs,
            &steps,
            rows,
        )
        .await
        .unwrap();

        assert_eq!(outcome.written, 1);
        // 目标 stream 写入 + connector egress 都发生，且收到的是变换后的事件。
        assert_eq!(sink.drain()[0].events.len(), 1);
        let seen = dispatcher.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].get("kept"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn compile_error_fails_whole_batch() {
        let vrl = VrlRuntime::new();
        let sink = MemoryIngestSink::new();
        let dispatcher = CapturingDispatcher::default();
        let connectors = never_called_repo();
        let steps = json!({ "steps": [{ "transform_name": "bad", "script": "this ((" }] });

        let err = transform_and_sink(
            &vrl,
            &sink,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "out",
            StreamType::Logs,
            &steps,
            vec![json!({})],
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("compile"));
        // 配置错误 → 不应有任何写入。
        assert!(sink.drain().is_empty());
    }

    #[tokio::test]
    async fn passthrough_when_no_steps() {
        let vrl = VrlRuntime::new();
        let sink = MemoryIngestSink::new();
        let dispatcher = CapturingDispatcher::default();
        let connectors = never_called_repo();

        let outcome = transform_and_sink(
            &vrl,
            &sink,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "out",
            StreamType::Logs,
            &json!({}),
            vec![json!({ "a": 1 }), json!({ "a": 2 })],
        )
        .await
        .unwrap();

        assert_eq!(outcome.written, 2);
    }
}
