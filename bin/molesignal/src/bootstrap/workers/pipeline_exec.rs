// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled-pipeline 执行编排（pipeline-runs-and-backfill）。
//!
//! 端到端链路：**读源 stream → 逐步 VRL 变换 → 写目标 stream（标准 intake）→ connector egress**。
//! 「读源」一环由调用方（持 [`crate::app::query::QueryService`]，app 层）完成后，把行喂进
//! [`transform_and_sink`]；本模块只承担「变换 + 写目标 + egress」，因此可脱离 HTTP /
//! 调度时序单测。backfill worker（[`super::search_jobs`]）即调用此核心；未来的 cron runner 可复用。
//!
//! 与 [`crate::infra::pipeline::exec`] 的分工：后者是纯计算（解析 steps + 串行 VRL），本模块
//! 把它接到 [`IntakeService`] 与 connector egress 上。写目标走 intake 而不是直接写 WAL，是因为
//! 目标流不一定已经存在、也不一定已有输出里的全部字段：建流、schema 演化与类型校验都在 intake 里。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::{
    app::{intake::IntakeService, query::QueryService},
    domain::{
        intake::{IntakeBatch, IntakeError, RawEvent},
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    infra::{
        connectors::{ConnectorDispatcher, ConnectorRepository},
        pipeline::{
            PipelineExecutor, ScheduledPipeline, ScheduledPipelineRunner,
            exec::{
                parse_signal_type, parse_sink_connectors, processing::process_rows,
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
    /// 目标 stream 接收的事件数（通过全部 VRL 步骤、且没有被目标流拒收的行）。
    pub written: usize,
    /// 变换后的事件（也用于 egress 投递与 monitor 落 NDJSON）。
    pub transformed: Vec<Value>,
    /// 非致命错误（单条事件 VRL 运行失败、目标流拒收事件、单个 connector egress 失败等），
    /// 不阻断其余。
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

/// 把读到的源行串行应用 `function_steps`（VRL）→ 写 `target_stream`（标准 intake）→ 按
/// `sink_connectors` 向外部 connector egress。
///
/// 目标流不存在时按本批事件建流，输出里的新字段照常演化目标流的 schema
/// （见 `IntakeService::intake_pipeline_output`）。
///
/// 失败语义：任一步 VRL **编译**失败 = pipeline 配置错误 → 整批失败（返 `Err`，调用方 mark_failed）。
/// 单条事件 VRL **运行**失败 → 丢弃该事件、记入 `errors`；单条事件被目标流拒收（与已有字段类型
/// 冲突）→ 同样记入 `errors`；单个 connector egress 失败 → 记入 `errors`，均不中断其余处理。
/// 但目标流把收到的事件全部拒收时，这次运行等于什么都没写进去，不能报成功：egress 做完后返
/// `Err`，把拒收原因带给调用方。
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
    intake: &IntakeService,
    connectors: &dyn ConnectorRepository,
    dispatcher: &dyn ConnectorDispatcher,
    org_id: &Id,
    target_stream: &str,
    target_stream_type: StreamType,
    function_steps: &Value,
    source_rows: Vec<Value>,
) -> Result<PipelineRunOutcome> {
    if crate::domain::pipeline::realtime::is_realtime(function_steps) {
        return Err(Error::invalid(
            "realtime pipelines cannot run as scheduled jobs",
        ));
    }

    let scanned = source_rows.len();
    let processed = process_rows(vrl, function_steps, target_stream, source_rows)?;
    let mut errors = processed.errors;
    let mut transformed = Vec::with_capacity(processed.rows.len());
    let mut destinations: BTreeMap<String, Vec<RawEvent>> = BTreeMap::new();
    let now = TimestampMicros::now();

    for (destination, event) in processed.rows {
        destinations
            .entry(destination)
            .or_default()
            .push(to_raw_event(&event, now));

        transformed.push(event);
    }

    let mut written = 0;
    let mut rejected = 0;
    let mut rejections = Vec::new();

    for (destination, events) in destinations {
        let result = intake
            .intake_pipeline_output(IntakeBatch {
                batch_id: Id::new(),
                org_id: org_id.clone(),
                stream: destination.clone(),
                stream_type: target_stream_type,
                events,
                received_at: now,
            })
            .await?;

        written += result.accepted;
        rejected += result.rejected;
        rejections.extend(rejection_summaries(&destination, &result.errors));
    }

    errors.extend(rejections.iter().cloned());

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

    if written == 0 && rejected > 0 {
        return Err(Error::conflict(rejections.join("; ")));
    }

    Ok(PipelineRunOutcome {
        scanned,
        written,
        transformed,
        errors,
    })
}

/// 目标流拒收的事件按原因归并后，最多逐条列出这么多种原因。
const MAX_REJECTION_REASONS: usize = 5;

/// 一个目标流拒收的事件 → 可读的非致命错误。同一种原因只报一条并带上条数：与已有字段类型冲突时
/// 通常整批事件的原因都一样，逐条列出会灌出成千上万行。
fn rejection_summaries(destination: &str, rejected: &[IntakeError]) -> Vec<String> {
    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();

    for error in rejected {
        *by_reason.entry(error.reason.as_str()).or_default() += 1;
    }

    let mut reasons: Vec<_> = by_reason.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let more = reasons.len().saturating_sub(MAX_REJECTION_REASONS);

    let mut lines: Vec<String> = reasons
        .into_iter()
        .take(MAX_REJECTION_REASONS)
        .map(|(reason, count)| {
            format!("target stream `{destination}` rejected {count} event(s): {reason}")
        })
        .collect();

    if more > 0 {
        lines.push(format!(
            "target stream `{destination}` rejected events for {more} more reason(s)"
        ));
    }

    lines
}

/// 一行变换后的 JSON 对象 → `RawEvent`：从常见时间字段取微秒时间戳，缺失则用 `fallback`。
///
/// 两处与 HTTP 写入对齐，因为目标流的 schema 由这些字段推断：
/// - `_timestamp` 是隐式时间列，取出后不再留作数据字段，否则会被当成新字段写进 schema，和隐式列同名；
/// - 值为 null 的字段丢掉。源流 `SELECT *` 会给事件没有的列补 null，这些 null 不携带信息，却会让
///   首次建流时把整列推断成 JSON，也会让非空字段的缺失检查失效。
fn to_raw_event(value: &Value, fallback: TimestampMicros) -> RawEvent {
    let mut fields = match value {
        Value::Object(map) => map.clone(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("value".to_string(), other.clone());
            m
        }
    };

    let system_timestamp = fields.remove("_timestamp");
    fields.retain(|_, value| !value.is_null());

    let timestamp = system_timestamp
        .as_ref()
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
    intake: Arc<IntakeService>,
    connectors: Arc<dyn ConnectorRepository>,
    dispatcher: Arc<dyn ConnectorDispatcher>,
}

impl BootstrapPipelineExecutor {
    pub fn new(
        query: Arc<QueryService>,
        intake: Arc<IntakeService>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
    ) -> Self {
        Self {
            query,
            intake,
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
            self.intake.as_ref(),
            self.connectors.as_ref(),
            self.dispatcher.as_ref(),
            &pipeline.org_id,
            &pipeline.target_stream,
            stream_type,
            &pipeline.function_steps,
            source_rows,
        )
        .await?;

        if !outcome.errors.is_empty() {
            tracing::warn!(
                pipeline_id = %pipeline.id.0,
                scanned = outcome.scanned,
                written = outcome.written,
                errors = ?outcome.errors,
                "scheduled pipeline completed with non-fatal errors"
            );
        }

        Ok(outcome.written as u64)
    }
}

/// 起 cron 轮询循环：每 `poll_secs` 调一次 `runner.tick_once`（runner 内按各 pipeline 的
/// cron + last_run_at 判定是否真的 fire）。调用方据角色（alert / standalone）决定是否 spawn。
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
    use crate::{
        domain::stream::{FieldDef, FieldType, Schema, StreamDefinition, StreamRepository},
        infra::{connectors::Connector, intake_sink::MemoryIntakeSink},
    };

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

    /// 可建流、可演化 schema 的内存流仓储：目标流是否存在、建成什么样，都由测试直接观察。
    #[derive(Default)]
    struct Streams(Mutex<Vec<StreamDefinition>>);

    impl Streams {
        fn find(&self, name: &str) -> Option<StreamDefinition> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|stream| stream.name == name)
                .cloned()
        }
    }

    #[async_trait]
    impl StreamRepository for Streams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            self.0.lock().unwrap().push(def.clone());
            Ok(def)
        }
        async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
            let mut streams = self.0.lock().unwrap();
            let stream = streams
                .iter_mut()
                .find(|stream| &stream.id == id)
                .ok_or_else(|| Error::not_found("stream"))?;
            stream.schema = schema;
            Ok(())
        }
        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|stream| {
                    &stream.org_id == org_id
                        && stream.name == name
                        && stream.stream_type == stream_type
                })
                .cloned()
                .ok_or_else(|| Error::not_found(format!("stream `{name}`")))
        }
        async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|stream| &stream.org_id == org_id)
                .cloned()
                .collect())
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            unimplemented!()
        }
    }

    fn field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    /// 组织 `o1` 下一个已存在、带指定字段的 logs 目标流。
    fn existing_stream(name: &str, fields: Vec<FieldDef>) -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id("o1".into()),
            name: name.into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// 真实的 `IntakeService`（建流、schema 演化、类型校验都在里面），落点是内存 sink。
    struct Fixture {
        intake: IntakeService,
        sink: Arc<MemoryIntakeSink>,
        streams: Arc<Streams>,
    }

    fn fixture(existing: Vec<StreamDefinition>) -> Fixture {
        let sink = Arc::new(MemoryIntakeSink::new());
        let streams = Arc::new(Streams(Mutex::new(existing)));
        Fixture {
            intake: IntakeService::new(sink.clone(), streams.clone()),
            sink,
            streams,
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

    /// 对 `fx` 跑一遍 `transform_and_sink`，目标流类型固定为 logs。
    async fn run(
        fx: &Fixture,
        target: &str,
        steps: &Value,
        rows: Vec<Value>,
    ) -> Result<PipelineRunOutcome> {
        transform_and_sink(
            &VrlRuntime::new(),
            &fx.intake,
            &never_called_repo(),
            &CapturingDispatcher::default(),
            &Id("o1".into()),
            target,
            StreamType::LOGS,
            steps,
            rows,
        )
        .await
    }

    #[tokio::test]
    async fn transforms_then_writes_to_sink() {
        let vrl = VrlRuntime::new();
        let fx = fixture(Vec::new());
        let dispatcher = CapturingDispatcher::default();
        let connectors = never_called_repo();
        let steps = json!({ "steps": [{ "transform_name": "tag", "script": ".count = 1" }] });
        let rows = vec![json!({ "msg": "a" }), json!({ "msg": "b" })];

        let outcome = transform_and_sink(
            &vrl,
            &fx.intake,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "logs_5m",
            StreamType::LOGS,
            &steps,
            rows,
        )
        .await
        .unwrap();

        assert_eq!(outcome.scanned, 2);
        assert_eq!(outcome.written, 2);
        assert!(outcome.errors.is_empty());

        let batches = fx.sink.drain();
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
        let fx = fixture(Vec::new());
        let dispatcher = CapturingDispatcher::default();
        let connectors = OneConnectorRepo(sample_connector());
        let steps = json!({
            "steps": [{ "transform_name": "noop", "script": ".kept = true" }],
            "sink_connectors": ["c1"],
        });
        let rows = vec![json!({ "msg": "x" })];

        let outcome = transform_and_sink(
            &vrl,
            &fx.intake,
            &connectors,
            &dispatcher,
            &Id("o1".into()),
            "out",
            StreamType::LOGS,
            &steps,
            rows,
        )
        .await
        .unwrap();

        assert_eq!(outcome.written, 1);
        // 目标 stream 写入 + connector egress 都发生，且收到的是变换后的事件。
        assert_eq!(fx.sink.drain()[0].events.len(), 1);
        let seen = dispatcher.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].get("kept"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn compile_error_fails_whole_batch() {
        let fx = fixture(Vec::new());
        let steps = json!({ "steps": [{ "transform_name": "bad", "script": "this ((" }] });

        let err = run(&fx, "out", &steps, vec![json!({})]).await.unwrap_err();

        assert!(err.to_string().contains("compile"));
        // 配置错误 → 不应有任何写入，也不该因此建出目标流。
        assert!(fx.sink.drain().is_empty());
        assert!(fx.streams.find("out").is_none());
    }

    #[tokio::test]
    async fn passthrough_when_no_steps() {
        let fx = fixture(Vec::new());

        let outcome = run(
            &fx,
            "out",
            &json!({}),
            vec![json!({ "a": 1 }), json!({ "a": 2 })],
        )
        .await
        .unwrap();

        assert_eq!(outcome.written, 2);
    }

    /// 回归：目标流不存在时运行曾以 `not found: row not found` 失败，要先手工往目标流写一条数据
    /// 才能让定时 pipeline 跑起来。
    #[tokio::test]
    async fn a_missing_target_stream_is_created_from_the_output() {
        let fx = fixture(Vec::new());
        let rows = vec![
            json!({ "_timestamp": 1_700_000_000_000_000_i64, "msg": "a", "extra": null }),
            json!({ "_timestamp": 1_700_000_001_000_000_i64, "msg": "b", "extra": null }),
        ];

        let outcome = run(&fx, "logs_5m", &json!({}), rows).await.unwrap();

        assert_eq!((outcome.scanned, outcome.written), (2, 2));
        let created = fx
            .streams
            .find("logs_5m")
            .expect("the first write creates the target stream");
        assert_eq!(created.stream_type, StreamType::LOGS);
        let fields: Vec<_> = created
            .schema
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(
            fields.contains(&"msg"),
            "schema comes from the output: {fields:?}"
        );
        // `_timestamp` 是隐式时间列，不是数据字段；全 null 的列不携带类型信息。
        assert!(!fields.contains(&"_timestamp"), "{fields:?}");
        assert!(!fields.contains(&"extra"), "{fields:?}");

        let batches = fx.sink.drain();
        assert_eq!(batches.len(), 1);
        let events = &batches[0].events;
        assert_eq!(events[0].timestamp, TimestampMicros(1_700_000_000_000_000));
        assert_eq!(events[1].timestamp, TimestampMicros(1_700_000_001_000_000));
    }

    #[tokio::test]
    async fn new_output_fields_extend_an_existing_target_stream() {
        let fx = fixture(vec![existing_stream(
            "out",
            vec![field("msg", FieldType::Utf8)],
        )]);
        let steps = json!({ "steps": [{ "transform_name": "tag", "script": ".count = 1" }] });

        let outcome = run(&fx, "out", &steps, vec![json!({ "msg": "a" })])
            .await
            .unwrap();

        assert_eq!(outcome.written, 1);
        let schema = fx.streams.find("out").unwrap().schema;
        let count = schema.fields.iter().find(|f| f.name == "count").unwrap();
        assert_eq!(count.data_type, FieldType::Int64);
    }

    /// 与目标流已有字段类型冲突的事件被拒收：只丢这些事件，原因写进非致命错误，其余照常写入。
    #[tokio::test]
    async fn events_that_conflict_with_the_target_schema_are_reported_not_fatal() {
        let fx = fixture(vec![existing_stream(
            "out",
            vec![field("count", FieldType::Int64)],
        )]);
        let steps = json!({ "steps": [{
            "transform_name": "tag",
            "script": "if .n == 1 { .count = 1 } else { .count = \"oops\" }",
        }] });
        let rows = vec![json!({ "n": 1 }), json!({ "n": 2 }), json!({ "n": 3 })];

        let outcome = run(&fx, "out", &steps, rows).await.unwrap();

        assert_eq!((outcome.scanned, outcome.written), (3, 1));
        assert_eq!(
            outcome.errors,
            vec![
                "target stream `out` rejected 2 event(s): \
                 type mismatch on field `count`: expected Int64, got String"
            ]
        );
        assert_eq!(fx.sink.drain()[0].events.len(), 1);
    }

    /// 目标流把事件全部拒收时，运行要失败并带上原因，而不是报成功、只剩日志里一行警告；
    /// 声明了 connector 时 egress 照常做完。
    #[tokio::test]
    async fn a_target_stream_that_rejects_everything_fails_the_run_with_the_reason() {
        let fx = fixture(vec![existing_stream(
            "out",
            vec![field("count", FieldType::Int64)],
        )]);
        let dispatcher = CapturingDispatcher::default();
        let steps = json!({
            "steps": [{ "transform_name": "tag", "script": ".count = \"oops\"" }],
            "sink_connectors": ["c1"],
        });

        let err = transform_and_sink(
            &VrlRuntime::new(),
            &fx.intake,
            &OneConnectorRepo(sample_connector()),
            &dispatcher,
            &Id("o1".into()),
            "out",
            StreamType::LOGS,
            &steps,
            vec![json!({ "n": 1 }), json!({ "n": 2 })],
        )
        .await
        .unwrap_err();

        assert_eq!(err.http_status_code(), 409);
        assert_eq!(
            err.to_string(),
            "conflict: target stream `out` rejected 2 event(s): \
             type mismatch on field `count`: expected Int64, got String"
        );
        assert!(fx.sink.drain().is_empty());
        assert_eq!(dispatcher.seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_reserved_system_stream_is_not_a_valid_target() {
        let fx = fixture(Vec::new());

        let err = run(&fx, "_molesignal", &json!({}), vec![json!({ "a": 1 })])
            .await
            .unwrap_err();

        assert_eq!(err.http_status_code(), 403);
        assert!(fx.sink.drain().is_empty());
        assert!(fx.streams.find("_molesignal").is_none());
    }

    #[test]
    fn source_columns_become_event_time_and_data_fields() {
        let fallback = TimestampMicros(7);

        // `_timestamp` 进事件时间，且不再是数据字段；null 列丢弃。
        let event = to_raw_event(
            &json!({ "_timestamp": 42, "msg": "a", "level": null }),
            fallback,
        );
        assert_eq!(event.timestamp, TimestampMicros(42));
        assert_eq!(event.fields.len(), 1);
        assert_eq!(event.fields.get("msg"), Some(&json!("a")));

        // 没有 `_timestamp` 时看 `timestamp` 字段（它是普通数据字段，保留）；都没有就用兜底时间。
        let event = to_raw_event(&json!({ "timestamp": 9, "msg": "a" }), fallback);
        assert_eq!(event.timestamp, TimestampMicros(9));
        assert_eq!(event.fields.get("timestamp"), Some(&json!(9)));
        assert_eq!(
            to_raw_event(&json!({ "msg": "a" }), fallback).timestamp,
            fallback
        );

        // 非对象的输出整体放进 `value`。
        let event = to_raw_event(&json!(3), fallback);
        assert_eq!(event.fields.get("value"), Some(&json!(3)));
    }

    #[test]
    fn rejections_are_grouped_by_reason_and_capped() {
        fn rejected(reason: &str, count: usize) -> Vec<IntakeError> {
            (0..count)
                .map(|index| IntakeError {
                    index,
                    reason: reason.into(),
                })
                .collect()
        }
        let mut errors = rejected("type mismatch on field `a`", 4);
        errors.extend(rejected("type mismatch on field `b`", 2));
        for letter in ["c", "d", "e", "f", "g"] {
            errors.extend(rejected(&format!("type mismatch on field `{letter}`"), 1));
        }

        let lines = rejection_summaries("out", &errors);

        // 7 种原因：最常见的在前，只列 5 种，其余合并成一行。
        assert_eq!(lines.len(), MAX_REJECTION_REASONS + 1);
        assert_eq!(
            lines[0],
            "target stream `out` rejected 4 event(s): type mismatch on field `a`"
        );
        assert_eq!(
            lines[1],
            "target stream `out` rejected 2 event(s): type mismatch on field `b`"
        );
        assert_eq!(
            lines.last().unwrap(),
            "target stream `out` rejected events for 2 more reason(s)"
        );
        assert!(rejection_summaries("out", &[]).is_empty());
    }
}
