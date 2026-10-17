// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;

use super::{delivery::connector_key, streams::to_raw_event, *};
use crate::{
    domain::{
        intake::{IntakeBatch, IntakeResult, IntakeSink},
        stream::{FieldDef, FieldType, Schema, StreamDefinition, StreamRepository},
    },
    infra::{connectors::Connector, intake_sink::MemoryIntakeSink},
};

/// 只实现 `get`（egress 路径用到的唯一方法）；其余方法测试不触达。
struct Connectors(Vec<Connector>);

#[async_trait]
impl ConnectorRepository for Connectors {
    async fn create(&self, _c: Connector) -> Result<Connector> {
        unimplemented!()
    }
    async fn update(&self, _c: Connector) -> Result<Connector> {
        unimplemented!()
    }
    async fn touch_last_run(&self, _id: &Id, _ts: TimestampMicros) -> Result<()> {
        unimplemented!()
    }
    async fn get(&self, _org_id: &Id, id: &Id) -> Result<Connector> {
        self.0
            .iter()
            .find(|connector| &connector.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("connector `{id}`")))
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

/// 记录被 dispatch 的事件；前 `failures` 次投递失败。
#[derive(Default)]
struct Dispatcher {
    seen: Mutex<Vec<Value>>,
    attempts: AtomicUsize,
    failures: usize,
}

#[async_trait]
impl ConnectorDispatcher for Dispatcher {
    async fn dispatch(&self, _connector: &Connector, events: &[Value]) -> Result<()> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) < self.failures {
            return Err(Error::unavailable("bucket unreachable"));
        }

        self.seen.lock().unwrap().extend_from_slice(events);
        Ok(())
    }
}

/// 内存 sink；写进 `failing` 里的流时报存储不可用。
#[derive(Default)]
struct Sink {
    memory: MemoryIntakeSink,
    failing: Mutex<BTreeSet<String>>,
}

#[async_trait]
impl IntakeSink for Sink {
    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        if self.failing.lock().unwrap().contains(&batch.stream) {
            return Err(Error::unavailable("storage unavailable"));
        }

        self.memory.write(batch).await
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
                &stream.org_id == org_id && stream.name == name && stream.stream_type == stream_type
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

/// 组织 `o1` 下一个已存在、带指定字段的 logs 流。
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

fn connector(id: &str) -> Connector {
    Connector {
        id: Id(id.into()),
        org_id: Id("o1".into()),
        name: format!("{id}-sink"),
        kind: "s3".into(),
        config_json: json!({}),
        enabled: true,
        last_run_at: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

/// 真实的 `IntakeService`（建流、schema 演化、类型校验都在里面），落点是内存 sink。
struct Fixture {
    output: PipelineOutput,
    sink: Arc<Sink>,
    streams: Arc<Streams>,
    dispatcher: Arc<Dispatcher>,
}

fn fixture(existing: Vec<StreamDefinition>) -> Fixture {
    with_dispatcher(existing, Dispatcher::default())
}

fn with_dispatcher(existing: Vec<StreamDefinition>, dispatcher: Dispatcher) -> Fixture {
    let sink = Arc::new(Sink::default());
    let streams = Arc::new(Streams(Mutex::new(existing)));
    let dispatcher = Arc::new(dispatcher);
    let mut output = PipelineOutput::new(
        Arc::new(IntakeService::new(sink.clone(), streams.clone())),
        Arc::new(Connectors(vec![connector("c1")])),
        dispatcher.clone(),
    );
    output.delivery_backoff = Duration::ZERO;

    Fixture {
        output,
        sink,
        streams,
        dispatcher,
    }
}

impl Fixture {
    fn written(&self) -> Vec<IntakeBatch> {
        self.sink.memory.drain()
    }
}

fn rows(source: &str, rows: Vec<Value>) -> SourceRows {
    SourceRows {
        source: source.into(),
        rows,
        done: BTreeSet::new(),
    }
}

/// 对 `fx` 跑一遍 pipeline：源 `app`、目标 `out`（logs），`steps` 里另可声明其余配置。
async fn run_with(
    fx: &Fixture,
    steps: &Value,
    input: Vec<SourceRows>,
) -> Result<PipelineRunOutcome> {
    let config = ScheduledConfig::parse(steps, "app", "out")?;
    let spec = PipelineSpec {
        org_id: &Id("o1".into()),
        name: "nightly",
        stream_type: StreamType::LOGS,
        function_steps: steps,
        config: &config,
    };

    fx.output.run(&VrlRuntime::new(), &spec, input).await
}

async fn run(fx: &Fixture, steps: &Value, events: Vec<Value>) -> Result<PipelineRunOutcome> {
    run_with(fx, steps, vec![rows("app", events)]).await
}

fn counts(scanned: i64, written: i64, dropped: i64, failed: i64) -> RunCounts {
    RunCounts {
        scanned,
        written,
        dropped,
        failed,
    }
}

#[tokio::test]
async fn transforms_then_writes_to_the_target() {
    let fx = fixture(Vec::new());
    let steps = json!({ "steps": [{ "transform_name": "tag", "script": ".count = 1" }] });

    let outcome = run(
        &fx,
        &steps,
        vec![json!({ "msg": "a" }), json!({ "msg": "b" })],
    )
    .await
    .unwrap();

    assert_eq!(outcome.counts, counts(2, 2, 0, 0));
    assert!(outcome.errors.is_empty());
    assert!(outcome.failure.is_none());
    let batches = fx.written();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].stream, "out");
    assert_eq!(batches[0].events.len(), 2);
    // VRL `.count = 1` 应已写进每个事件。
    assert_eq!(batches[0].events[0].fields.get("count"), Some(&json!(1)));
    // 无 sink_connectors → egress 未触达。
    assert!(fx.dispatcher.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn egresses_to_declared_connectors() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "steps": [{ "transform_name": "noop", "script": ".kept = true" }],
        "sink_connectors": ["c1"],
    });

    let outcome = run(&fx, &steps, vec![json!({ "msg": "x" })]).await.unwrap();

    assert_eq!(outcome.counts, counts(1, 1, 0, 0));
    // 目标 stream 写入 + connector egress 都发生，且收到的是变换后的事件。
    assert_eq!(fx.written()[0].events.len(), 1);
    let seen = fx.dispatcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].get("kept"), Some(&json!(true)));
    assert!(outcome.sources[0].done.contains(&connector_key("c1")));
}

#[tokio::test]
async fn compile_error_fails_whole_batch() {
    let fx = fixture(Vec::new());
    let steps = json!({ "steps": [{ "transform_name": "bad", "script": "this ((" }] });

    let err = run(&fx, &steps, vec![json!({})]).await.unwrap_err();

    assert!(err.to_string().contains("compile"));
    // 配置错误 → 不应有任何写入，也不该因此建出目标流。
    assert!(fx.written().is_empty());
    assert!(fx.streams.find("out").is_none());
}

#[tokio::test]
async fn passthrough_when_no_steps() {
    let fx = fixture(Vec::new());

    let outcome = run(&fx, &json!({}), vec![json!({ "a": 1 }), json!({ "a": 2 })])
        .await
        .unwrap();

    assert_eq!(outcome.counts, counts(2, 2, 0, 0));
}

/// 回归：目标流不存在时运行曾以 `not found: row not found` 失败，要先手工往目标流写一条数据
/// 才能让定时 pipeline 跑起来。
#[tokio::test]
async fn a_missing_target_stream_is_created_from_the_output() {
    let fx = fixture(Vec::new());
    let events = vec![
        json!({ "_timestamp": 1_700_000_000_000_000_i64, "msg": "a", "extra": null }),
        json!({ "_timestamp": 1_700_000_001_000_000_i64, "msg": "b", "extra": null }),
    ];

    let outcome = run(&fx, &json!({}), events).await.unwrap();

    assert_eq!(outcome.counts, counts(2, 2, 0, 0));
    let created = fx
        .streams
        .find("out")
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

    let batches = fx.written();
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

    let outcome = run(&fx, &steps, vec![json!({ "msg": "a" })]).await.unwrap();

    assert_eq!(outcome.counts.written, 1);
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
    let events = vec![json!({ "n": 1 }), json!({ "n": 2 }), json!({ "n": 3 })];

    let outcome = run(&fx, &steps, events).await.unwrap();

    assert_eq!(outcome.counts, counts(3, 1, 0, 2));
    assert_eq!(
        outcome.errors,
        vec![
            "stream `out` rejected 2 event(s): \
             type mismatch on field `count`: expected Int64, got String"
        ]
    );
    assert!(outcome.failure.is_none());
    assert_eq!(fx.written()[0].events.len(), 1);
}

/// 目标流把事件全部拒收时，运行要报失败并带上原因，而不是报成功、只剩日志里一行警告；
/// 声明了 connector 时 egress 照常做完。
#[tokio::test]
async fn a_target_stream_that_rejects_everything_fails_the_run_with_the_reason() {
    let fx = fixture(vec![existing_stream(
        "out",
        vec![field("count", FieldType::Int64)],
    )]);
    let steps = json!({
        "steps": [{ "transform_name": "tag", "script": ".count = \"oops\"" }],
        "sink_connectors": ["c1"],
    });

    let outcome = run(&fx, &steps, vec![json!({ "n": 1 }), json!({ "n": 2 })])
        .await
        .unwrap();

    assert_eq!(
        outcome.failure.as_deref(),
        Some(
            "stream `out` rejected 2 event(s): \
             type mismatch on field `count`: expected Int64, got String"
        )
    );
    // Rejected, not a failed write: there is nothing to try again.
    assert!(outcome.sources[0].failure.is_none());
    assert!(fx.written().is_empty());
    assert_eq!(fx.dispatcher.seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn every_stream_takes_every_event_and_a_route_moves_only_the_first() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "sinks": ["out", "archive", "audit"],
        "steps": [{
            "kind": "builtin",
            "operation": "route",
            "routing": { "kind": "field", "field": "appname", "prefix": "logs_", "fallback": "out" },
        }],
    });
    let events = vec![
        json!({ "appname": "orders" }),
        json!({ "appname": "payments" }),
        json!({ "msg": "no app" }),
    ];

    let outcome = run(&fx, &steps, events).await.unwrap();

    assert_eq!(outcome.counts, counts(3, 3, 0, 0));
    let batches = fx.written();
    let sizes: BTreeMap<&str, usize> = batches
        .iter()
        .map(|batch| (batch.stream.as_str(), batch.events.len()))
        .collect();
    assert_eq!(
        sizes,
        BTreeMap::from([
            ("archive", 3),
            ("audit", 3),
            ("logs_orders", 1),
            ("logs_payments", 1),
            ("out", 1),
        ])
    );
    for stream in ["out", "archive", "audit", "logs_orders"] {
        assert!(outcome.sources[0].done.contains(stream), "{stream}");
    }
}

/// An event routed to a copy stream is written there once.
#[tokio::test]
async fn an_event_routed_into_a_copy_is_written_there_once() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "sinks": ["out", "archive"],
        "steps": [{ "kind": "builtin", "operation": "route", "target": "archive" }],
    });

    run(&fx, &steps, vec![json!({ "a": 1 })]).await.unwrap();

    let batches = fx.written();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].stream, "archive");
    assert_eq!(batches[0].events.len(), 1);
}

/// Routing by a field can name a source only at run time; that event fails, the rest go on.
#[tokio::test]
async fn an_event_routed_into_a_source_fails_alone() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "steps": [{
            "kind": "builtin",
            "operation": "route",
            "routing": { "kind": "field", "field": "appname", "fallback": "out" },
        }],
    });

    let outcome = run(
        &fx,
        &steps,
        vec![json!({ "appname": "app" }), json!({ "appname": "orders" })],
    )
    .await
    .unwrap();

    assert_eq!(outcome.counts, counts(2, 1, 0, 1));
    assert_eq!(
        outcome.errors,
        vec!["1 event(s) failed: routed to `app`, a stream the pipeline reads"]
    );
    assert_eq!(fx.written()[0].stream, "orders");
}

#[tokio::test]
async fn aborted_and_duplicate_events_are_dropped_not_failed() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "dedupe_field": "trace.id",
        "steps": [{ "transform_name": "filter", "script": "if .level == \"debug\" { abort }" }],
    });
    let events = vec![
        json!({ "level": "debug", "trace": { "id": "t0" } }),
        json!({ "level": "info", "trace": { "id": "t1" } }),
        json!({ "level": "info", "trace": { "id": "t1" } }),
        json!({ "level": "info", "trace": { "id": "t2" } }),
        json!({ "level": "info" }),
        json!({ "level": "info" }),
    ];

    let outcome = run(&fx, &steps, events).await.unwrap();

    assert_eq!(outcome.counts, counts(6, 4, 2, 0));
    assert!(outcome.errors.is_empty());
    assert_eq!(fx.written()[0].events.len(), 4);
}

#[tokio::test]
async fn several_sources_are_processed_together_and_steps_see_where_events_came_from() {
    let fx = fixture(Vec::new());
    let steps = json!({
        "sources": ["app", "nginx"],
        "dedupe_field": "id",
        "steps": [{ "transform_name": "tag", "script": ".from = %source_stream" }],
    });

    let outcome = run_with(
        &fx,
        &steps,
        vec![
            rows("app", vec![json!({ "id": 1 }), json!({ "id": 2 })]),
            // The same event read from a second source is written once.
            rows("nginx", vec![json!({ "id": 2 }), json!({ "id": 3 })]),
        ],
    )
    .await
    .unwrap();

    assert_eq!(outcome.counts, counts(4, 3, 1, 0));
    let froms: Vec<_> = fx.written()[0]
        .events
        .iter()
        .map(|event| event.fields["from"].clone())
        .collect();
    assert_eq!(froms, vec![json!("app"), json!("app"), json!("nginx")]);
}

/// A delivery is tried again within the run before it gives up.
#[tokio::test]
async fn a_failed_delivery_is_tried_again() {
    let fx = with_dispatcher(
        Vec::new(),
        Dispatcher {
            failures: 2,
            ..Dispatcher::default()
        },
    );
    let steps = json!({ "sink_connectors": ["c1"], "retry": { "delivery_retries": 2 } });

    let outcome = run(&fx, &steps, vec![json!({ "a": 1 })]).await.unwrap();

    assert_eq!(outcome.counts, counts(1, 1, 0, 0));
    assert_eq!(fx.dispatcher.attempts.load(Ordering::SeqCst), 3);
    assert_eq!(fx.dispatcher.seen.lock().unwrap().len(), 1);
}

/// What a step failed on and what a stream rejected go to the dead letter stream with what
/// happened to them; the run itself goes through.
#[tokio::test]
async fn failures_become_dead_letters_with_their_reason() {
    let fx = fixture(vec![existing_stream(
        "out",
        vec![field("n", FieldType::Int64)],
    )]);
    let steps = json!({
        "dead_letter_stream": "failed_events",
        "steps": [{ "transform_name": "parse", "script": ". = parse_json!(.message)" }],
    });
    let events = vec![
        json!({ "message": "not json" }),
        json!({ "message": "{\"n\":\"text\"}" }),
        json!({ "message": "{\"n\":1}" }),
    ];

    let outcome = run(&fx, &steps, events).await.unwrap();

    assert_eq!(outcome.counts, counts(3, 1, 0, 2));
    assert!(outcome.failure.is_none());

    let batches = fx.written();
    let letters = batches
        .iter()
        .find(|batch| batch.stream == "failed_events")
        .expect("dead letters were written");
    assert_eq!(letters.stream_type, StreamType::LOGS);
    assert_eq!(letters.events.len(), 2);

    let failed = &letters.events[0].fields;
    // A step's failure keeps the event as it was read.
    assert_eq!(failed["message"], "not json");
    assert_eq!(failed["dead_letter_pipeline"], "nightly");
    assert_eq!(failed["dead_letter_stage"], "transform");
    assert_eq!(failed["dead_letter_source"], "app");
    assert!(
        failed["dead_letter_reason"]
            .as_str()
            .unwrap()
            .starts_with("step `parse` run:")
    );
    assert!(!failed.contains_key("dead_letter_destination"));

    let rejected = &letters.events[1].fields;
    assert_eq!(rejected["n"], "text");
    assert_eq!(rejected["dead_letter_stage"], "write");
    assert_eq!(rejected["dead_letter_destination"], "out");
    assert_eq!(
        rejected["dead_letter_reason"],
        "type mismatch on field `n`: expected Int64, got String"
    );
}

/// A stream that rejects everything does not fail the run when its events were kept as dead letters.
#[tokio::test]
async fn dead_letters_keep_a_stream_that_rejected_everything_from_failing_the_run() {
    let fx = fixture(vec![existing_stream(
        "out",
        vec![field("n", FieldType::Int64)],
    )]);
    let steps = json!({ "dead_letter_stream": "failed_events" });

    let outcome = run(&fx, &steps, vec![json!({ "n": "text" })])
        .await
        .unwrap();

    assert_eq!(outcome.counts, counts(1, 0, 0, 1));
    assert!(outcome.failure.is_none());
    assert!(!outcome.errors.is_empty());
}

/// What a connector never took, after its retries, goes to the dead letter stream.
#[tokio::test]
async fn undelivered_events_become_dead_letters() {
    let fx = with_dispatcher(
        Vec::new(),
        Dispatcher {
            failures: usize::MAX,
            ..Dispatcher::default()
        },
    );
    let steps = json!({
        "sink_connectors": ["c1"],
        "retry": { "delivery_retries": 1 },
        "dead_letter_stream": "failed_events",
    });

    let outcome = run(&fx, &steps, vec![json!({ "a": 1 }), json!({ "a": 2 })])
        .await
        .unwrap();

    assert_eq!(outcome.counts, counts(2, 0, 0, 2));
    assert_eq!(fx.dispatcher.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        outcome.errors,
        vec![
            "connector `c1-sink` did not take 2 event(s) after 2 attempt(s): \
             unavailable: bucket unreachable"
        ]
    );
    // Not tried again by a later run: a retried window would deliver the same events twice.
    assert!(outcome.sources[0].done.contains(&connector_key("c1")));

    let batches = fx.written();
    let letters = batches
        .iter()
        .find(|batch| batch.stream == "failed_events")
        .unwrap();
    assert_eq!(letters.events.len(), 2);
    assert_eq!(letters.events[0].fields["dead_letter_stage"], "deliver");
    assert_eq!(
        letters.events[0].fields["dead_letter_destination"],
        "c1-sink"
    );
    assert_eq!(letters.events[1].fields["a"], 2);
}

/// Writing to one stream failing leaves the others written; the failure is kept on the source,
/// with the streams that did take its rows, for a retry to leave out.
#[tokio::test]
async fn a_failed_write_is_kept_on_its_source_and_the_other_streams_are_written() {
    let fx = fixture(Vec::new());
    fx.sink.failing.lock().unwrap().insert("archive".into());
    let steps = json!({ "sinks": ["out", "archive"], "sink_connectors": ["c1"] });

    let outcome = run(&fx, &steps, vec![json!({ "a": 1 })]).await.unwrap();

    assert_eq!(outcome.counts, counts(1, 0, 0, 1));
    let source = &outcome.sources[0];
    assert_eq!(
        source.failure,
        Some(WriteFailure {
            message: "write to `archive`: unavailable: storage unavailable".into(),
            permanent: false,
        })
    );
    assert!(source.done.contains("out"));
    assert!(!source.done.contains("archive"));
    assert!(source.done.contains(&connector_key("c1")));
    assert_eq!(fx.written()[0].stream, "out");
}

/// A retried window goes only where it did not go the first time.
#[tokio::test]
async fn a_retried_window_leaves_out_what_already_took_it() {
    let fx = fixture(Vec::new());
    let steps = json!({ "sinks": ["out", "archive"], "sink_connectors": ["c1"] });
    let mut retried = rows("app", vec![json!({ "a": 1 })]);
    retried.done = BTreeSet::from(["out".to_owned(), connector_key("c1")]);

    let outcome = run_with(&fx, &steps, vec![retried]).await.unwrap();

    assert_eq!(outcome.counts, counts(1, 1, 0, 0));
    let batches = fx.written();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].stream, "archive");
    assert!(fx.dispatcher.seen.lock().unwrap().is_empty());
    assert!(outcome.sources[0].failure.is_none());
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
fn reasons_are_grouped_and_capped() {
    let mut reasons = vec!["type mismatch on field `a`"; 4];
    reasons.extend(vec!["type mismatch on field `b`"; 2]);
    let more: Vec<String> = ["c", "d", "e", "f", "g"]
        .iter()
        .map(|letter| format!("type mismatch on field `{letter}`"))
        .collect();
    reasons.extend(more.iter().map(String::as_str));

    let lines = summaries(reasons.into_iter(), |count, reason| {
        format!("stream `out` rejected {count} event(s): {reason}")
    });

    // 7 种原因：最常见的在前，只列 5 种，其余合并成一行。
    assert_eq!(lines.len(), MAX_REASONS + 1);
    assert_eq!(
        lines[0],
        "stream `out` rejected 4 event(s): type mismatch on field `a`"
    );
    assert_eq!(
        lines[1],
        "stream `out` rejected 2 event(s): type mismatch on field `b`"
    );
    assert_eq!(
        lines.last().unwrap(),
        "stream `out` rejected 2 event(s): 2 more reason(s)"
    );
    assert!(summaries(std::iter::empty(), |_, reason| reason.to_owned()).is_empty());
}
