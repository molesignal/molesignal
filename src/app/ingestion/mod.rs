// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 写入用例。
//!
//! 输入：`IngestBatch`（org / stream / 一组 `RawEvent`）。
//! 输出：`IngestResult { accepted, rejected, errors }`。
//!
//! 职责（spec 顺序）：
//! 1. 校验目标 stream 存在；
//! 2. **Pipeline 变换**：可选 `PipelineEngine::apply`（VRL function 链）；
//! 3. 类型校验 + 新字段 schema 演化；
//! 4. 把整批喂给 `IngestSink`（infra 实现 WAL + buffer + parquet flush）。
//!
//! 类型冲突 / pipeline 单步错误的事件单条剔除并填到 `IngestResult.errors`。

mod derived;
pub mod pipeline;
mod schema;

use std::sync::Arc;

pub use pipeline::{FunctionExecutor, NoopFunctionExecutor, PipelineEngine};
pub use schema::{check_event_types, infer_schema_extension};

use crate::{
    domain::{
        ingestion::{
            EVENT_ID_FIELD, IngestBatch, IngestError, IngestResult, IngestSink, RawEvent,
            ServiceGraphObserver,
        },
        masking::{Masker, MaskingProvider},
        storage::PhysicalDatasetKind,
        stream::{
            Schema, StreamDefinition, StreamRepository, StreamType, is_reserved_system_stream,
            validate_stream_name,
        },
    },
    shared::{Error, Result, drain::DrainController, ids::Id, time::TimestampMicros},
};

type InternalUsageRecorder = Arc<dyn Fn(Id, TimestampMicros, u64) + Send + Sync + 'static>;

pub struct IngestService {
    sink: Arc<dyn IngestSink>,
    streams: Arc<dyn StreamRepository>,
    /// bootstrap 解析出的不可变 `_sys` 组织。可信 self-telemetry 入口同时校验
    /// 组织与流名，避免 crate 内调用方把平台自身数据写到普通租户。
    system_org_id: Option<Id>,
    /// 可信内部遥测的原始字节计量回调。外部协议入口已按 wire payload 计量；
    /// 这里只补齐不经过协议门禁的内部写入。
    internal_usage_recorder: Option<InternalUsageRecorder>,
    /// 可选 pipeline 引擎；为 None 时直接走 schema 校验 + sink。
    pipeline_engine: Option<Arc<PipelineEngine>>,
    /// 可选脱敏规则源；非空时 pipeline 之后、落盘之前对事件字符串值就地脱敏。
    masking: Option<Arc<dyn MaskingProvider>>,
    /// 可选节点 drain 状态；退役中拒绝新写入（503），让 ingester 把 pending flush 干净后下线。
    drain: Option<Arc<DrainController>>,
    /// 可选 trace 观测器；非空时对 Traces 批旁路派生服务调用边（service graph）。
    service_graph: Option<Arc<dyn ServiceGraphObserver>>,
}

impl IngestService {
    pub fn new(sink: Arc<dyn IngestSink>, streams: Arc<dyn StreamRepository>) -> Self {
        Self {
            sink,
            streams,
            system_org_id: None,
            internal_usage_recorder: None,
            pipeline_engine: None,
            masking: None,
            drain: None,
            service_graph: None,
        }
    }

    pub fn with_pipeline(mut self, engine: Arc<PipelineEngine>) -> Self {
        self.pipeline_engine = Some(engine);
        self
    }

    /// 注入脱敏规则源；非空时写入前对事件做 pattern 驱动脱敏。
    pub fn with_masking(mut self, masking: Arc<dyn MaskingProvider>) -> Self {
        self.masking = Some(masking);
        self
    }

    /// 注入节点 drain 状态；退役中拒绝新写入。
    pub fn with_drain(mut self, drain: Arc<DrainController>) -> Self {
        self.drain = Some(drain);
        self
    }

    /// 注入 trace 观测器；非空时 Traces 批写入后旁路派生服务调用边。
    pub fn with_service_graph(mut self, observer: Arc<dyn ServiceGraphObserver>) -> Self {
        self.service_graph = Some(observer);
        self
    }

    /// 注入 bootstrap 已验证的不可变 `_sys` 组织 ID。
    pub fn with_system_org_id(mut self, system_org_id: Id) -> Self {
        self.system_org_id = Some(system_org_id);
        self
    }

    /// 注入内部遥测计量器。回调必须自行 best-effort 持久化，不能反压摄取。
    pub fn with_internal_usage_recorder(mut self, recorder: InternalUsageRecorder) -> Self {
        self.internal_usage_recorder = Some(recorder);
        self
    }

    /// 公共/用户来源写入。保留系统流在统一应用边界拒绝，因此所有协议适配器都无法
    /// 通过伪造 payload 绕过保护。
    pub async fn ingest(&self, batch: IngestBatch) -> Result<IngestResult> {
        if is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "`_molesignal` is a protected system stream",
            ));
        }
        self.ingest_with_origin(batch, IngestOrigin::External, PhysicalDatasetKind::Raw)
            .await
    }

    /// 可信的服务自身遥测写入入口。不是可反序列化字段，且只在 crate 内开放。
    pub(crate) async fn ingest_self_telemetry(&self, batch: IngestBatch) -> Result<IngestResult> {
        let system_org_id = self.system_org_id.as_ref().ok_or_else(|| {
            Error::internal("self telemetry system organization is not configured")
        })?;
        if &batch.org_id != system_org_id || !is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "self telemetry may only target `_sys/_molesignal`",
            ));
        }
        self.ingest_with_origin(batch, IngestOrigin::SelfTelemetry, PhysicalDatasetKind::Raw)
            .await
    }

    /// 可信的产品内部遥测入口（例如 `_intelligence_model_traces`）。保留
    /// `_molesignal` 的专用入口，防止其他内部调用伪装成服务自身遥测。
    pub(crate) async fn ingest_internal_telemetry(
        &self,
        batch: IngestBatch,
    ) -> Result<IngestResult> {
        if !batch.stream.starts_with('_') || is_reserved_system_stream(&batch.stream) {
            return Err(Error::invalid(
                "internal telemetry must target a protected non-self stream",
            ));
        }
        self.ingest_with_origin(
            batch,
            IngestOrigin::InternalTelemetry,
            PhysicalDatasetKind::Raw,
        )
        .await
    }

    /// 可信应用服务写入独立的派生物理数据集。它复用 schema 校验/演化和 WAL 语义，
    /// 但不会执行面向外部 payload 的 pipeline，也不会重复派生 service graph。
    pub(crate) async fn ingest_derived_dataset(
        &self,
        batch: IngestBatch,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<IngestResult> {
        if dataset_kind == PhysicalDatasetKind::Raw {
            return Err(Error::invalid("derived dataset kind must not be raw"));
        }
        if is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "reserved self telemetry requires the dedicated derived entry point",
            ));
        }
        self.ingest_with_origin(batch, IngestOrigin::InternalTelemetry, dataset_kind)
            .await
    }

    pub(crate) async fn ingest_self_telemetry_dataset(
        &self,
        batch: IngestBatch,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<IngestResult> {
        let system_org_id = self.system_org_id.as_ref().ok_or_else(|| {
            Error::internal("self telemetry system organization is not configured")
        })?;
        if dataset_kind == PhysicalDatasetKind::Raw
            || &batch.org_id != system_org_id
            || !is_reserved_system_stream(&batch.stream)
        {
            return Err(Error::forbidden(
                "derived self telemetry must target `_sys/_molesignal`",
            ));
        }
        self.ingest_with_origin(batch, IngestOrigin::SelfTelemetry, dataset_kind)
            .await
    }

    #[tracing::instrument(
        name = "ingest.batch",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.ingest.origin = ?origin,
            molesignal.ingest.signal = ?batch.stream_type,
            molesignal.ingest.event_count = batch.events.len()
        )
    )]
    async fn ingest_with_origin(
        &self,
        mut batch: IngestBatch,
        origin: IngestOrigin,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<IngestResult> {
        validate_stream_name(&batch.stream)?;
        // 节点退役中：停接新写入（让 pending 数据 flush 干净后安全下线）。
        if let Some(drain) = &self.drain
            && !drain.accepts_writes()
        {
            return Err(Error::unavailable(
                "node is draining; not accepting new writes",
            ));
        }

        // 内部写入没有可复用的 wire body；以进入应用边界时的 RawEvent JSON 作为稳定的
        // 原始字节口径。必须在 pipeline / masking / schema 处理前采样。
        let internal_usage = (origin != IngestOrigin::External)
            .then(|| {
                serde_json::to_vec(&batch.events).ok().map(|payload| {
                    (
                        batch.org_id.clone(),
                        batch.received_at,
                        payload.len() as u64,
                    )
                })
            })
            .flatten();

        // schema-on-write（按需建流）：目标流存在则用；不存在则用本批
        // 推断的 schema 自动建流再写入。撤掉启动期预 seed 后，这是空实例首次接收数据
        // （含 send-test-event 打 `default` 流）不报 "stream not found" 的关键。
        let def = match self
            .streams
            .get(&batch.org_id, &batch.stream, batch.stream_type)
            .await
        {
            Ok(def) => def,
            Err(Error::NotFound(_)) => self.create_stream_on_demand(&batch).await?,
            Err(e) => return Err(e),
        };

        let mut errors: Vec<IngestError> = Vec::new();

        // 1) Pipeline 变换
        if origin == IngestOrigin::External
            && let Some(engine) = &self.pipeline_engine
        {
            let pipeline_errors = engine.apply(&mut batch).await?;
            if !pipeline_errors.is_empty() {
                let bad: std::collections::HashSet<usize> =
                    pipeline_errors.iter().map(|e| e.index).collect();
                let mut idx = 0;
                batch.events.retain(|_| {
                    let keep = !bad.contains(&idx);
                    idx += 1;
                    keep
                });
                errors.extend(pipeline_errors);
            }
        }

        // 1.5) 写入即脱敏（pattern 驱动）：pipeline 产出的字段也会被脱敏。无规则 → no-op。
        if let Some(masking) = &self.masking {
            let masker = masking.ingest_masker(&batch.org_id).await?;
            mask_events(&masker, &mut batch.events);
        }

        // Log-like datasets are queried while new rows continue to arrive.
        // Timestamp alone is not unique, so assign an immutable batch+row id
        // before schema evolution and persistence. RUM streams use Logs too.
        if batch.stream_type == StreamType::Logs {
            assign_log_event_ids(&batch.batch_id, &mut batch.events);
        }

        // 2) 类型冲突先在内存里剔除
        let mut kept: Vec<RawEvent> = Vec::with_capacity(batch.events.len());
        for (idx, ev) in batch.events.drain(..).enumerate() {
            match check_event_types(&def.schema, &ev) {
                Ok(()) => kept.push(ev),
                Err(reason) => errors.push(IngestError { index: idx, reason }),
            }
        }
        let total_kept = kept.len();
        batch.events = kept;

        // 3) schema 演化
        if let Some(new_schema) =
            infer_schema_extension(&def.schema, &batch.events, batch.stream_type)
        {
            if origin == IngestOrigin::External {
                self.streams.update_schema(&def.id, new_schema).await?;
            } else {
                self.streams
                    .update_schema_internal(&def.id, new_schema)
                    .await?;
            }
        }

        // 3.5) 旁路观测 trace：从 span 派生服务间调用边（service graph）。仅 Traces 批、
        // 仅已留存事件；纯内存累计、不阻塞写入；无观测器时零开销。
        if dataset_kind == PhysicalDatasetKind::Raw
            && batch.stream_type == StreamType::Traces
            && total_kept > 0
            && let Some(sg) = &self.service_graph
        {
            sg.observe(&batch.org_id, &batch.events);
        }

        // 原始批次完成全部用户可见变换后再投影内部读模型，保证摘要与权威数据使用
        // 同一份脱敏、类型校验结果。测试/内存 sink 不声明能力时保持原有单批语义。
        let derived =
            if dataset_kind == PhysicalDatasetKind::Raw && self.sink.supports_derived_datasets() {
                derived::project(&batch)
            } else {
                Vec::new()
            };

        // 4) 写入
        let mut result = if total_kept == 0 {
            IngestResult {
                accepted: 0,
                rejected: errors.len(),
                errors: Vec::new(),
            }
        } else {
            self.sink.write_dataset(dataset_kind, batch).await?
        };
        if result.accepted > 0 {
            for (kind, batch) in derived {
                let derived_result = self.sink.write_dataset(kind, batch).await?;
                if derived_result.rejected != 0 {
                    return Err(Error::internal(format!(
                        "physical dataset `{kind}` rejected {} derived rows",
                        derived_result.rejected
                    )));
                }
            }
        }
        result.rejected += errors.len();
        result.errors.extend(errors);
        if result.accepted > 0
            && let Some((org_id, received_at, bytes)) = internal_usage
            && bytes > 0
            && let Some(recorder) = &self.internal_usage_recorder
        {
            recorder(org_id, received_at, bytes);
        }
        Ok(result)
    }

    /// 流不存在时按本批事件推断 schema 建流（schema-on-write）。后续 `infer_schema_extension`
    /// 会接管增量演化，这里只负责让首批数据有个落点。并发下两批同时创建相同
    /// `(org, name, type)` 时，不静默复用竞争者创建的定义，而是把 409 冲突直接返回给
    /// 输掉竞争的写入方，避免一次传输在未确认目标定义的情况下继续写入。
    async fn create_stream_on_demand(&self, batch: &IngestBatch) -> Result<StreamDefinition> {
        let schema =
            infer_schema_extension(&Schema { fields: vec![] }, &batch.events, batch.stream_type)
                .unwrap_or(Schema { fields: vec![] });
        let now = TimestampMicros::now();
        let def = StreamDefinition {
            id: Id::new(),
            org_id: batch.org_id.clone(),
            name: batch.stream.clone(),
            stream_type: batch.stream_type,
            schema,
            retention: None,
            created_at: now,
            updated_at: now,
        };
        match self.streams.create(def.clone()).await {
            Ok(created) => {
                tracing::info!(
                    org_id = %batch.org_id.0,
                    stream = %batch.stream,
                    ?batch.stream_type,
                    "auto-created stream on first ingest"
                );
                Ok(created)
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IngestOrigin {
    External,
    InternalTelemetry,
    SelfTelemetry,
}

/// 对一批事件就地脱敏（masker 为空时跳过，零开销）。
fn mask_events(masker: &Masker, events: &mut [RawEvent]) {
    if masker.is_empty() {
        return;
    }
    for ev in events {
        masker.mask_fields(&mut ev.fields);
    }
}

fn assign_log_event_ids(batch_id: &Id, events: &mut [RawEvent]) {
    for (index, event) in events.iter_mut().enumerate() {
        event.fields.insert(
            EVENT_ID_FIELD.to_string(),
            serde_json::Value::String(format!("{}:{index}", batch_id.as_str())),
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType},
        shared::{time::TimestampMicros, trace::summary::TRACE_SUMMARY_MARKER_FIELD},
    };

    fn ev(map: serde_json::Value) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros::now(),
            fields: map.as_object().unwrap().clone(),
        }
    }

    #[test]
    fn log_event_ids_are_unique_and_override_untrusted_values() {
        let mut events = vec![
            ev(json!({ EVENT_ID_FIELD: "client-value", "message": "one" })),
            ev(json!({ "message": "two" })),
        ];

        assign_log_event_ids(&Id::from_string("batch-a"), &mut events);

        assert_eq!(events[0].fields[EVENT_ID_FIELD], "batch-a:0");
        assert_eq!(events[1].fields[EVENT_ID_FIELD], "batch-a:1");
    }

    #[test]
    fn type_check_accepts_matching() {
        let s = Schema {
            fields: vec![FieldDef {
                name: "msg".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        };
        assert!(check_event_types(&s, &ev(json!({"msg":"x"}))).is_ok());
        assert!(check_event_types(&s, &ev(json!({"msg":1}))).is_err());
    }

    #[test]
    fn extension_picks_up_new_fields() {
        let s = Schema {
            fields: vec![FieldDef {
                name: "msg".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        };
        let events = vec![
            ev(json!({"msg":"a", "lat":3})),
            ev(json!({"msg":"b","ok":true})),
        ];
        let next = infer_schema_extension(&s, &events, StreamType::Logs).expect("extension");
        let names: Vec<&str> = next.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"lat"));
        assert!(names.contains(&"ok"));
    }

    /// traces 流的关联字段与摘要 carrier 自动 exact-indexed；其余字段不索引。
    #[test]
    fn traces_correlation_fields_are_auto_exact_indexed() {
        let events = vec![ev(json!({
            "trace_id": "0af7651916cd43dd8448eb211c80319c",
            "span_id": "b7ad6b7169203331",
            "service.name": "checkout",
            (TRACE_SUMMARY_MARKER_FIELD): "1",
            "http.status_code": 500,
        }))];
        let next = infer_schema_extension(&Schema { fields: vec![] }, &events, StreamType::Traces)
            .expect("extension");
        let by_name = |n: &str| next.fields.iter().find(|f| f.name == n).unwrap().clone();
        for f in [
            "trace_id",
            "span_id",
            "service.name",
            TRACE_SUMMARY_MARKER_FIELD,
        ] {
            let fd = by_name(f);
            assert!(fd.indexed && fd.exact, "{f} 应自动 exact-indexed");
        }
        // 普通字段不受影响。
        let sc = by_name("http.status_code");
        assert!(!sc.indexed && !sc.exact, "普通字段不应被自动索引");

        // 同名字段在 logs 流不触发（策略仅限 traces）。
        let logs = infer_schema_extension(&Schema { fields: vec![] }, &events, StreamType::Logs)
            .expect("extension");
        let tid = logs.fields.iter().find(|f| f.name == "trace_id").unwrap();
        assert!(
            !tid.indexed && !tid.exact,
            "logs 流不应自动 exact-index trace_id"
        );
    }

    #[test]
    fn mask_events_redacts_string_fields_in_place() {
        let masker = Masker::compile([(r"\d{3}-\d{2}-\d{4}".to_string(), "[SSN]".to_string())]);
        let mut events = vec![
            ev(json!({"msg": "ssn 123-45-6789", "code": 200})),
            ev(json!({"msg": "clean line"})),
        ];
        mask_events(&masker, &mut events);
        assert_eq!(events[0].fields["msg"], json!("ssn [SSN]"));
        assert_eq!(events[0].fields["code"], json!(200), "non-string untouched");
        assert_eq!(events[1].fields["msg"], json!("clean line"));
    }

    #[test]
    fn mask_events_empty_masker_is_noop() {
        let masker = Masker::default();
        let mut events = vec![ev(json!({"msg": "123-45-6789"}))];
        mask_events(&masker, &mut events);
        assert_eq!(events[0].fields["msg"], json!("123-45-6789"));
    }

    // drain 拒写：退役中 ingest() 在触碰 sink/streams 之前就返回 503，故 stub 方法 unreachable。
    struct UnreachableSink;
    #[async_trait::async_trait]
    impl crate::domain::ingestion::IngestSink for UnreachableSink {
        async fn write(&self, _: IngestBatch) -> Result<IngestResult> {
            unreachable!("draining must short-circuit before sink")
        }
    }
    struct UnreachableStreams;
    #[async_trait::async_trait]
    impl StreamRepository for UnreachableStreams {
        async fn create(&self, _: StreamDefinition) -> Result<StreamDefinition> {
            unreachable!()
        }
        async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
            unreachable!()
        }
        async fn get(
            &self,
            _: &Id,
            _: &str,
            _: crate::domain::stream::StreamType,
        ) -> Result<StreamDefinition> {
            unreachable!()
        }
        async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
            unreachable!()
        }
        async fn delete(&self, _: &Id) -> Result<()> {
            unreachable!()
        }
    }

    struct ConflictOnCreateStreams;

    #[async_trait::async_trait]
    impl StreamRepository for ConflictOnCreateStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            Err(Error::conflict(format!(
                "stream `{}` with type `{}` already exists",
                def.name,
                def.stream_type.as_str()
            )))
        }

        async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
            unreachable!()
        }

        async fn get(&self, _: &Id, _: &str, _: StreamType) -> Result<StreamDefinition> {
            Err(Error::not_found("stream"))
        }

        async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
            unreachable!()
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn concurrent_auto_create_conflict_is_returned_to_writer() {
        let svc = IngestService::new(Arc::new(UnreachableSink), Arc::new(ConflictOnCreateStreams));
        let err = svc
            .ingest(IngestBatch {
                batch_id: Id::new(),
                org_id: Id::from_string("org-1"),
                stream: "app_logs".into(),
                stream_type: StreamType::Logs,
                events: vec![ev(json!({"message": "first batch"}))],
                received_at: TimestampMicros::now(),
            })
            .await
            .unwrap_err();

        assert_eq!(err.http_status_code(), 409);
        assert!(err.to_string().contains("app_logs"));
        assert!(err.to_string().contains("logs"));
    }

    #[tokio::test]
    async fn ingest_rejected_with_503_when_draining() {
        use crate::shared::drain::DrainController;

        let drain = Arc::new(DrainController::new());
        assert!(drain.begin_drain());
        let svc = IngestService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_drain(drain);
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: "s".into(),
            stream_type: crate::domain::stream::StreamType::Logs,
            events: vec![ev(json!({"msg": "x"}))],
            received_at: TimestampMicros::now(),
        };
        let err = svc.ingest(batch).await.unwrap_err();
        assert_eq!(err.http_status_code(), 503, "draining ingest returns 503");
    }

    #[tokio::test]
    async fn external_ingest_cannot_target_system_stream() {
        let svc = IngestService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams));
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
            stream_type: StreamType::Logs,
            events: vec![ev(json!({"message": "spoof"}))],
            received_at: TimestampMicros::now(),
        };
        let err = svc.ingest(batch).await.unwrap_err();
        assert_eq!(err.http_status_code(), 403);
    }

    struct ExistingSystemStream {
        def: StreamDefinition,
    }

    #[async_trait::async_trait]
    impl StreamRepository for ExistingSystemStream {
        async fn create(&self, _: StreamDefinition) -> Result<StreamDefinition> {
            unreachable!("system stream already exists")
        }

        async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
            Ok(())
        }

        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            assert_eq!(org_id, &self.def.org_id);
            assert_eq!(name, self.def.name);
            assert_eq!(stream_type, self.def.stream_type);
            Ok(self.def.clone())
        }

        async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(vec![self.def.clone()])
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn trusted_self_telemetry_reaches_protected_stream() {
        let org_id = Id::from_string("system-org");
        let now = TimestampMicros::now();
        let streams = Arc::new(ExistingSystemStream {
            def: StreamDefinition {
                id: Id::new(),
                org_id: org_id.clone(),
                name: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::Logs,
                schema: Schema { fields: vec![] },
                retention: Some(crate::domain::stream::Retention { days: 7 }),
                created_at: now,
                updated_at: now,
            },
        });
        let sink = Arc::new(crate::infra::ingest_sink::MemoryIngestSink::new());
        let batches = sink.batches.clone();
        let usage = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_usage = usage.clone();
        let svc = IngestService::new(sink, streams)
            .with_system_org_id(org_id.clone())
            .with_internal_usage_recorder(Arc::new(move |org_id, received_at, bytes| {
                captured_usage
                    .lock()
                    .expect("usage lock")
                    .push((org_id, received_at, bytes));
            }));
        let result = svc
            .ingest_self_telemetry(IngestBatch {
                batch_id: Id::new(),
                org_id: org_id.clone(),
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::Logs,
                events: vec![ev(json!({"message": "self"}))],
                received_at: now,
            })
            .await
            .unwrap();
        assert_eq!(result.accepted, 1);
        assert_eq!(batches.lock().len(), 1);
        let recorded = usage.lock().expect("usage lock");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, org_id);
        assert_eq!(recorded[0].1, now);
        assert!(recorded[0].2 > 0);
    }

    #[tokio::test]
    async fn trusted_self_telemetry_cannot_target_a_tenant_organization() {
        let system_org_id = Id::from_string("system-org");
        let svc = IngestService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_system_org_id(system_org_id);
        let err = svc
            .ingest_self_telemetry(IngestBatch {
                batch_id: Id::new(),
                org_id: Id::from_string("tenant-org"),
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::Logs,
                events: vec![ev(json!({"message": "misrouted"}))],
                received_at: TimestampMicros::now(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.http_status_code(), 403);
        assert!(err.to_string().contains("_sys/_molesignal"));
    }

    #[tokio::test]
    async fn trusted_self_telemetry_still_respects_drain() {
        use crate::shared::drain::DrainController;

        let drain = Arc::new(DrainController::new());
        assert!(drain.begin_drain());
        let system_org_id = Id::from_string("system-org");
        let svc = IngestService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_system_org_id(system_org_id.clone())
            .with_drain(drain);
        let err = svc
            .ingest_self_telemetry(IngestBatch {
                batch_id: Id::new(),
                org_id: system_org_id,
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::Logs,
                events: vec![ev(json!({"message": "late"}))],
                received_at: TimestampMicros::now(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.http_status_code(), 503);
    }
}
