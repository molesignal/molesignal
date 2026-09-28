// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;

use super::*;
use crate::{
    domain::{
        pipeline::realtime::{OutputKind, RealtimeStep, Routing},
        stream::{Schema, StreamDefinition, StreamRepository},
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
    async fn for_source(&self, org: &Id, source: &str) -> Result<Option<RealtimePipeline>> {
        // A routed destination must never be dispatched back through this entry point.
        assert_eq!(source, "default");
        Ok((&self.0.org_id == org).then(|| self.0.clone()))
    }
}
fn pipeline(retain: bool) -> RealtimePipeline {
    RealtimePipeline {
        id: Id("pipeline".into()),
        org_id: Id("org".into()),
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
    })
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
        {"transform_name":"before","script":".appname = \"orders\""},
        {"transform_name":"route","kind":"builtin","operation":"route","routing":{"kind":"field","field":"appname","prefix":"logs_","fallback":"default"}},
        {"transform_name":"after","script":".appname = \"changed\""}
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
