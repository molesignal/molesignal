// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;

use super::*;
use crate::{
    domain::{
        pipeline::realtime::{OutputKind, RealtimeStep, Routing},
        stream::{Schema, StreamDefinition, StreamRepository, StreamType},
    },
    shared::time::TimestampMicros,
};

#[derive(Default)]
struct Streams(Mutex<Vec<StreamDefinition>>);

#[async_trait]
impl StreamRepository for Streams {
    async fn get(&self, org: &Id, name: &str, kind: StreamType) -> Result<StreamDefinition> {
        self.0
            .lock()
            .iter()
            .find(|d| &d.org_id == org && d.name == name && d.stream_type == kind)
            .cloned()
            .ok_or_else(|| Error::not_found("stream"))
    }

    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        self.0.lock().push(def.clone());
        Ok(def)
    }

    async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
        self.0
            .lock()
            .iter_mut()
            .find(|s| &s.id == id)
            .unwrap()
            .schema = schema;

        Ok(())
    }

    async fn list(&self, org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self
            .0
            .lock()
            .iter()
            .filter(|d| &d.org_id == org)
            .cloned()
            .collect())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        unreachable!()
    }
}

#[derive(Default)]
struct Sink {
    writes: Mutex<Vec<(IntakeBatch, Option<u64>)>>,
    fail_stream: Option<String>,
}

#[async_trait]
impl crate::domain::intake::IntakeSink for Sink {
    async fn write(&self, _: IntakeBatch) -> Result<IntakeResult> {
        unreachable!()
    }

    async fn write_metered_dataset(
        &self,
        _: crate::domain::storage::DatasetTypeId,
        batch: IntakeBatch,
        size: Option<u64>,
    ) -> Result<IntakeResult> {
        if self.fail_stream.as_ref() == Some(&batch.stream) {
            return Err(Error::unavailable("storage unavailable"));
        }

        let result = IntakeResult {
            accepted: batch.events.len(),
            rejected: 0,
            errors: vec![],
        };

        self.writes.lock().push((batch, size));
        Ok(result)
    }
}

struct Repo(RealtimePipeline);

#[async_trait]
impl RealtimePipelineRepository for Repo {
    async fn for_source(
        &self,
        org: &Id,
        signal: StreamType,
        source: &str,
    ) -> Result<Option<RealtimePipeline>> {
        // A routed destination must never be dispatched back through this entry point.
        assert_eq!(source, "default");
        Ok((&self.0.org_id == org && self.0.signal == signal).then(|| self.0.clone()))
    }
}

fn pipeline(retain: bool) -> RealtimePipeline {
    RealtimePipeline {
        id: Id("pipeline".into()),
        org_id: Id("org".into()),
        signal: StreamType::LOGS,
        target: "unused".into(),
        updated_at: TimestampMicros(1),
        routing: Routing {
            kind: OutputKind::Field,
            field: "appname".into(),
            prefix: String::new(),
            fallback: "default".into(),
            retain_source: retain,
        },
        steps: vec![RealtimeStep {
            transform_name: "enrich".into(),
            script: ".processed = true".into(),
            ..Default::default()
        }],
    }
}

fn batch(values: Vec<Value>) -> IntakeBatch {
    IntakeBatch {
        batch_id: Id("batch".into()),
        org_id: Id("org".into()),
        stream: "default".into(),
        stream_type: StreamType::LOGS,
        events: values
            .into_iter()
            .map(|v| RawEvent {
                timestamp: TimestampMicros(1),
                fields: v.as_object().unwrap().clone(),
            })
            .collect(),
        received_at: TimestampMicros(2),
    }
}

fn service(pipeline: RealtimePipeline, sink: Arc<Sink>) -> IntakeService {
    IntakeService::new(sink, Arc::new(Streams::default())).with_realtime(RealtimeEngine {
        repository: Arc::new(Repo(pipeline)),
        executor: Arc::new(crate::infra::runtime::vrl::executor::VrlFunctionExecutor::new()),
        steps: step_state(),
    })
}

fn step_state() -> Arc<StepState> {
    Arc::new(StepState::new(Arc::new(
        crate::domain::pipeline::limit::MemoryLimitUsageLedger::default(),
    )))
}

#[tokio::test]
async fn routes_mixed_batch_and_meters_original_bytes_per_stream() {
    let sink = Arc::new(Sink::default());
    let svc = service(pipeline(false), sink.clone());
    let input = batch(vec![
        json!({"appname":"orders","message":"one"}),
        json!({"appname":"payments"}),
        json!({"message":"no app"}),
        json!({"appname":"../bad"}),
    ]);
    let sizes: Vec<_> = input
        .events
        .iter()
        .map(|e| event_bytes(e).unwrap())
        .collect();
    let result = svc.intake(input).await.unwrap();
    assert_eq!((result.accepted, result.rejected), (4, 0));
    let writes = sink.writes.lock();
    assert_eq!(writes.len(), 3);
    let output = |name: &str| writes.iter().find(|(b, _)| b.stream == name).unwrap();
    assert_eq!(output("orders").1, Some(sizes[0]));
    assert_eq!(output("payments").1, Some(sizes[1]));
    assert_eq!(output("default").1, Some(sizes[2] + sizes[3]));
    assert_eq!(output("orders").0.events[0].fields["processed"], true);
}

#[tokio::test]
async fn retains_original_and_counts_each_input_once_without_self_duplicates() {
    let sink = Arc::new(Sink::default());
    let svc = service(pipeline(true), sink.clone());
    let result = svc
        .intake(batch(vec![
            json!({"appname":"orders"}),
            json!({"appname":"default"}),
            json!({"message":"fallback"}),
        ]))
        .await
        .unwrap();
    assert_eq!((result.accepted, result.rejected), (3, 0));
    let writes = sink.writes.lock();
    let original = &writes
        .iter()
        .find(|(b, _)| b.stream == "default")
        .unwrap()
        .0;
    assert_eq!(original.events.len(), 3);
    assert!(!original.events[0].fields.contains_key("processed"));
    assert_eq!(original.events[1].fields["processed"], true);
}

#[tokio::test]
async fn storage_failure_does_not_silently_fall_back() {
    let sink = Arc::new(Sink {
        fail_stream: Some("orders".into()),
        ..Default::default()
    });
    let svc = service(pipeline(false), sink.clone());
    assert!(
        svc.intake(batch(vec![json!({"appname":"orders"})]))
            .await
            .is_err()
    );
    assert!(sink.writes.lock().is_empty());
}

#[tokio::test]
async fn failed_transforms_report_original_indices_and_do_not_copy() {
    let sink = Arc::new(Sink::default());
    let mut p = pipeline(true);
    p.steps[0].script = ". = parse_json!(.message)".into();
    let svc = service(p, sink.clone());
    let result = svc
        .intake(batch(vec![
            json!({"message":"{\"appname\":\"orders\"}"}),
            json!({"message":"invalid json"}),
        ]))
        .await
        .unwrap();
    assert_eq!((result.accepted, result.rejected), (1, 1));
    assert_eq!(result.errors[0].index, 1);
    assert_eq!(
        sink.writes
            .lock()
            .iter()
            .map(|(b, _)| b.events.len())
            .sum::<usize>(),
        2
    );
}

#[tokio::test]
async fn another_organization_does_not_run_the_route() {
    let sink = Arc::new(Sink::default());
    let svc = service(pipeline(false), sink.clone());
    let mut input = batch(vec![json!({"appname":"orders"})]);
    input.org_id = Id("other".into());
    assert_eq!(svc.intake(input).await.unwrap().accepted, 1);
    assert_eq!(sink.writes.lock()[0].0.stream, "default");
    assert!(
        !sink.writes.lock()[0].0.events[0]
            .fields
            .contains_key("processed")
    );
}

#[tokio::test]
async fn builtin_uses_values_at_its_position_and_keeps_original_copy() {
    let sink = Arc::new(Sink::default());
    let mut p = pipeline(true);
    p.steps = crate::domain::pipeline::processing::parse_processing_steps(&json!({"steps":[
        {"transformName":"before","script":".appname = \"orders\""},
        {"transformName":"route","kind":"builtin","operation":"route","routing":{"kind":"field","field":"appname","prefix":"logs_","fallback":"default"}},
        {"transformName":"after","script":".appname = \"changed\""}
    ]})).unwrap();
    let result = service(p, sink.clone())
        .intake(batch(vec![json!({"message":"original"})]))
        .await
        .unwrap();
    assert_eq!((result.accepted, result.rejected), (1, 0));
    let writes = sink.writes.lock();
    assert_eq!(writes.len(), 2);
    let routed = writes
        .iter()
        .find(|(b, _)| b.stream == "logs_orders")
        .unwrap();
    assert_eq!(routed.0.events[0].fields["appname"], "changed");
    let mut payload = routed.0.events[0].fields.clone();
    payload.remove(crate::domain::intake::EVENT_ID_FIELD);
    assert_eq!(
        Value::Object(payload),
        json!({"message":"original","appname":"changed"})
    );
    let original = writes.iter().find(|(b, _)| b.stream == "default").unwrap();
    assert!(!original.0.events[0].fields.contains_key("appname"));
}

#[tokio::test]
async fn an_aborted_event_is_accepted_but_not_routed_and_the_source_copy_stays() {
    let sink = Arc::new(Sink::default());
    let mut p = pipeline(false);
    p.steps[0].script = "if .level == \"debug\" { abort }\n.processed = true".into();
    let svc = service(p, sink.clone());

    let result = svc
        .intake(batch(vec![
            json!({"appname":"orders","level":"debug"}),
            json!({"appname":"orders","level":"info"}),
        ]))
        .await
        .unwrap();

    // Dropping on purpose is not a rejection.
    assert_eq!((result.accepted, result.rejected), (2, 0));
    {
        let writes = sink.writes.lock();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0.stream, "orders");
        assert_eq!(writes[0].0.events.len(), 1);
        assert_eq!(writes[0].0.events[0].fields["level"], "info");
    }

    // Retaining the source keeps the event as it arrived, dropped or not.
    let sink = Arc::new(Sink::default());
    let mut p = pipeline(true);
    p.steps[0].script = "abort".into();

    let result = service(p, sink.clone())
        .intake(batch(vec![json!({"appname":"orders"})]))
        .await
        .unwrap();

    assert_eq!((result.accepted, result.rejected), (1, 0));
    let writes = sink.writes.lock();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0.stream, "default");
    assert!(!writes[0].0.events[0].fields.contains_key("processed"));
}

/// A pipeline that changes `signal` data in place, with these VRL steps.
fn in_place(signal: StreamType, scripts: &[&str]) -> RealtimePipeline {
    RealtimePipeline {
        signal,
        target: "default".into(),
        routing: Routing::default(),
        steps: scripts
            .iter()
            .map(|script| RealtimeStep {
                transform_name: "tidy".into(),
                script: (*script).into(),
                ..Default::default()
            })
            .collect(),
        ..pipeline(false)
    }
}

fn of(signal: StreamType, values: Vec<Value>) -> IntakeBatch {
    IntakeBatch {
        stream_type: signal,
        ..batch(values)
    }
}

#[tokio::test]
async fn a_metric_keeps_its_name_and_value_while_its_labels_change() {
    let sink = Arc::new(Sink::default());
    let svc = service(
        in_place(
            StreamType::METRICS,
            &[
                "del(.pod)\n.env = \"prod\"",
                "if .metric_name == \"up\" && .value == 0 { .value = 1 }",
            ],
        ),
        sink.clone(),
    );

    let result = svc
        .intake(of(
            StreamType::METRICS,
            vec![
                json!({"metric_name":"requests","value":3,"pod":"web-7f9c"}),
                json!({"metric_name":"up","value":0,"pod":"web-1d2e"}),
            ],
        ))
        .await
        .unwrap();

    assert_eq!((result.accepted, result.rejected), (1, 1));
    assert_eq!(result.errors[0].index, 1);
    assert!(
        result.errors[0]
            .reason
            .contains("changed `value`, which a pipeline on metrics keeps"),
        "{}",
        result.errors[0].reason
    );

    let writes = sink.writes.lock();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0.stream, "default");
    assert_eq!(writes[0].0.stream_type, StreamType::METRICS);

    let fields = &writes[0].0.events[0].fields;
    assert_eq!(fields["env"], "prod");
    assert!(!fields.contains_key("pod"));
}

#[tokio::test]
async fn spans_are_changed_where_they_are_written_and_report_their_places() {
    let sink = Arc::new(Sink::default());
    let svc = service(
        in_place(
            StreamType::TRACES,
            &[
                "if .name == \"GET /health\" { abort }\ndel(.\"db.statement\")",
                "if .name == \"bad\" { .trace_id = \"other\" }",
            ],
        ),
        sink.clone(),
    );

    let spans = vec![
        json!({"trace_id":"t1","span_id":"a","name":"GET /health"}),
        json!({
            "trace_id":"t1","span_id":"b","name":"SELECT users",
            "db.statement":"SELECT * FROM users WHERE id = 42",
            "attributes":{"db.statement":"SELECT * FROM users WHERE id = 42","db.system":"postgresql"},
        }),
        json!({"trace_id":"t1","span_id":"c","name":"bad"}),
    ];

    // The public entry leaves spans alone: they are changed where they are written.
    let result = svc
        .intake(of(StreamType::TRACES, spans.clone()))
        .await
        .unwrap();
    assert_eq!((result.accepted, result.rejected), (3, 0));
    assert_eq!(sink.writes.lock()[0].0.events.len(), 3);
    sink.writes.lock().clear();

    let result = svc
        .intake_spans(of(StreamType::TRACES, spans))
        .await
        .unwrap();

    // The dropped span is accepted; the failing one is reported where it was in the request.
    assert_eq!((result.accepted, result.rejected), (2, 1));
    assert_eq!(result.errors[0].index, 2);
    assert!(result.errors[0].reason.contains("changed `trace_id`"));

    let writes = sink.writes.lock();
    let written = &writes[0].0.events;
    assert_eq!(written.len(), 1);
    assert!(!written[0].fields.contains_key("db.statement"));
    assert_eq!(
        written[0].fields["attributes"],
        json!({"db.system":"postgresql"})
    );
}

#[tokio::test]
async fn a_rum_view_changes_in_its_custom_parts_and_is_never_dropped() {
    struct Rum(RealtimePipeline);

    #[async_trait]
    impl RealtimePipelineRepository for Rum {
        async fn for_source(
            &self,
            _: &Id,
            signal: StreamType,
            source: &str,
        ) -> Result<Option<RealtimePipeline>> {
            Ok((signal == StreamType::LOGS && source == self.0.target).then(|| self.0.clone()))
        }
    }

    let sink = Arc::new(Sink::default());

    let pipeline = RealtimePipeline {
        target: "rum_views".into(),
        ..in_place(
            StreamType::LOGS,
            &[
                "if .\"view.name\" == \"bot\" { abort }\n.context.team = \"checkout\"",
                "if .\"view.name\" == \"rename\" { .\"view.name\" = \"other\" }",
            ],
        )
    };

    let svc = IntakeService::new(sink.clone(), Arc::new(Streams::default())).with_realtime(
        RealtimeEngine {
            repository: Arc::new(Rum(pipeline)),
            executor: Arc::new(crate::infra::runtime::vrl::executor::VrlFunctionExecutor::new()),
            steps: step_state(),
        },
    );

    let mut input = batch(vec![
        json!({"view.name":"cart","context":{"plan":"pro"}}),
        json!({"view.name":"bot","context":{}}),
        json!({"view.name":"rename","context":{}}),
    ]);
    input.stream = "rum_views".into();

    let result = svc.intake(input).await.unwrap();

    // A view the steps would drop fails, and so does one whose contract field they would change.
    assert_eq!((result.accepted, result.rejected), (1, 2));
    assert!(
        result.errors[0]
            .reason
            .contains("dropped a row of rum_views")
    );
    assert!(result.errors[1].reason.contains("changed `view.name`"));

    let writes = sink.writes.lock();
    assert_eq!(writes[0].0.stream, "rum_views");
    assert_eq!(
        writes[0].0.events[0].fields["context"],
        json!({"plan":"pro","team":"checkout"})
    );
}

fn service_with(
    pipeline: RealtimePipeline,
    sink: Arc<Sink>,
    steps: Arc<StepState>,
) -> IntakeService {
    IntakeService::new(sink, Arc::new(Streams::default())).with_realtime(RealtimeEngine {
        repository: Arc::new(Repo(pipeline)),
        executor: Arc::new(crate::infra::runtime::vrl::executor::VrlFunctionExecutor::new()),
        steps,
    })
}

/// A logs pipeline made of these steps, as a saved one is read.
fn counting(steps: Value) -> RealtimePipeline {
    RealtimePipeline {
        target: "default".into(),
        routing: Routing::default(),
        steps: crate::domain::pipeline::processing::parse_processing_steps(
            &json!({ "steps": steps }),
        )
        .unwrap(),
        ..pipeline(false)
    }
}

/// A batch of events that happened now: metrics leave out events far from the clock.
fn recent(signal: StreamType, values: Vec<Value>) -> IntakeBatch {
    let mut batch = of(signal, values);

    for event in &mut batch.events {
        event.timestamp = TimestampMicros::now();
    }

    batch
}

fn written<'a>(writes: &'a [(IntakeBatch, Option<u64>)], stream: &str) -> &'a IntakeBatch {
    &writes
        .iter()
        .find(|(batch, _)| batch.stream == stream)
        .unwrap()
        .0
}

#[tokio::test]
async fn a_metric_step_counts_what_it_picks_and_the_events_go_on() {
    let sink = Arc::new(Sink::default());
    let steps = step_state();
    let svc = service_with(
        counting(json!([
            {
                "transformName": "errors",
                "kind": "generate_metric",
                "conditions": [{ "field": "level", "test": "equals", "value": "error" }],
                "metric": {
                    "name": "app_errors", "type": "count", "stream": "app_metrics",
                    "groupBy": ["service"],
                },
            },
            { "transformName": "drop-debug", "script": "if .level == \"debug\" { abort }" },
        ])),
        sink.clone(),
        steps.clone(),
    );

    let result = svc
        .intake(recent(
            StreamType::LOGS,
            vec![
                json!({ "level": "error", "service": "cart" }),
                json!({ "level": "error", "service": "cart" }),
                json!({ "level": "info", "service": "cart" }),
                json!({ "level": "debug", "service": "cart" }),
            ],
        ))
        .await
        .unwrap();

    // Counting first, then dropping what is not kept: the events themselves are not changed.
    assert_eq!((result.accepted, result.rejected), (4, 0));
    let logs = written(&sink.writes.lock(), "default").events.clone();
    assert_eq!(logs.len(), 3);
    assert_eq!(logs[0].fields["level"], "error");
    assert!(!logs[0].fields.contains_key("metric_name"));

    steps
        .tick(
            &svc,
            TimestampMicros(TimestampMicros::now().0 + 10 * 60_000_000),
        )
        .await;

    let writes = sink.writes.lock();
    let metrics = written(&writes, "app_metrics");

    assert_eq!(metrics.stream_type, StreamType::METRICS);
    assert_eq!(metrics.events.len(), 1);
    assert_eq!(metrics.events[0].fields["metric_name"], "app_errors");
    assert_eq!(metrics.events[0].fields["service"], "cart");
    assert_eq!(metrics.events[0].fields["value"], json!(2.0));
}

#[tokio::test]
async fn past_a_quota_events_leave_for_another_stream_as_they_are_or_are_dropped() {
    for (action, cold) in [("route", 2), ("drop", 0), ("pass", 0)] {
        let sink = Arc::new(Sink::default());
        let svc = service(
            counting(json!([
                { "transformName": "seen", "script": ".seen = true" },
                {
                    "transformName": "daily",
                    "kind": "quota",
                    "conditions": [{ "field": "service", "test": "equals", "value": "cart" }],
                    "quota": { "unit": "events", "limit": 2, "action": action, "target": "cold" },
                },
                { "transformName": "after", "script": ".after = true" },
            ])),
            sink.clone(),
        );

        let mut events = vec![json!({ "service": "cart" }); 4];
        events.push(json!({ "service": "pay" }));

        let result = svc.intake(recent(StreamType::LOGS, events)).await.unwrap();
        assert_eq!((result.accepted, result.rejected), (5, 0), "{action}");

        let writes = sink.writes.lock();
        let kept = written(&writes, "default").events.clone();

        // Two within the quota and the one it does not pick; all four with `pass`.
        assert_eq!(kept.len(), if action == "pass" { 5 } else { 3 }, "{action}");
        assert!(kept.iter().all(|event| event.fields["after"] == true));

        if cold > 0 {
            let routed = &written(&writes, "cold").events;

            assert_eq!(routed.len(), cold);
            assert_eq!(routed[0].fields["seen"], true);
            assert!(!routed[0].fields.contains_key("after"));
        } else {
            assert!(writes.iter().all(|(batch, _)| batch.stream != "cold"));
        }
    }
}

#[tokio::test]
async fn a_throttle_lets_each_group_through_up_to_its_limit() {
    let sink = Arc::new(Sink::default());
    let svc = service(
        counting(json!([{
            "transformName": "per-host",
            "kind": "throttle",
            "throttle": { "limit": 2, "windowSecs": 3600, "groupBy": ["host"] },
        }])),
        sink.clone(),
    );

    let events = ["a", "a", "a", "b", "a", "b"]
        .into_iter()
        .map(|host| json!({ "host": host }))
        .collect();

    let result = svc.intake(recent(StreamType::LOGS, events)).await.unwrap();
    assert_eq!((result.accepted, result.rejected), (6, 0));

    let writes = sink.writes.lock();
    let hosts: Vec<_> = written(&writes, "default")
        .events
        .iter()
        .map(|event| event.fields["host"].clone())
        .collect();

    assert_eq!(hosts, vec![json!("a"), json!("a"), json!("b"), json!("b")]);
}

#[tokio::test]
async fn spans_make_metrics_where_they_are_written() {
    let sink = Arc::new(Sink::default());
    let steps = step_state();
    let pipeline = RealtimePipeline {
        signal: StreamType::TRACES,
        ..counting(json!([{
            "transformName": "latency",
            "kind": "generate_metric",
            "metric": {
                "name": "span_duration_ns", "type": "histogram", "field": "duration_ns",
                "buckets": [1000, 1000000], "stream": "span_metrics", "intervalSecs": 10,
            },
        }]))
    };
    let svc = service_with(pipeline, sink.clone(), steps.clone());

    let result = svc
        .intake_spans(recent(
            StreamType::TRACES,
            vec![
                json!({ "trace_id": "t", "span_id": "a", "duration_ns": 500 }),
                json!({ "trace_id": "t", "span_id": "b", "duration_ns": 50000 }),
            ],
        ))
        .await
        .unwrap();

    assert_eq!((result.accepted, result.rejected), (2, 0));

    steps
        .tick(&svc, TimestampMicros(TimestampMicros::now().0 + 60_000_000))
        .await;

    let writes = sink.writes.lock();
    let samples = &written(&writes, "span_metrics").events;
    let count = samples
        .iter()
        .find(|sample| sample.fields["metric_name"] == "span_duration_ns_count")
        .unwrap();

    assert_eq!(count.fields["value"], json!(2.0));
    assert_eq!(samples.len(), 5, "two bounds, +Inf, count and sum");
}
