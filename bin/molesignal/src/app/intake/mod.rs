// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 写入用例。
//!
//! 输入：`IntakeBatch`（org / stream / 一组 `RawEvent`）。
//! 输出：`IntakeResult { accepted, rejected, errors }`。
//!
//! 职责（spec 顺序）：
//! 1. 校验目标 stream 存在；日志流、指标流挂着实时流水线时整批交给它（见 [`realtime`]；链路在
//!    写 span 的地方处理）；
//! 2. 写入即脱敏；
//! 3. 类型校验 + 新字段 schema 演化；
//! 4. 把整批喂给 `IntakeSink`（infra 实现 WAL + buffer + parquet flush）。
//!
//! 类型冲突的事件单条剔除并填到 `IntakeResult.errors`。

mod forward;
mod metrics;
pub mod realtime;
mod schema;
pub mod usage;

use std::sync::Arc;

pub use forward::IntakeForwarder;
pub use metrics::MetricRecorder;
pub use schema::{apply_index_rules, check_event_types, infer_schema_extension};

pub use crate::domain::function::FunctionExecutor;
use crate::{
    app::alerting::realtime::RealtimeAlerts,
    domain::{
        intake::{
            EVENT_ID_FIELD, IntakeBatch, IntakeError, IntakeResult, IntakeSink, RawEvent,
            ServiceGraphObserver,
        },
        masking::{Masker, MaskingProvider},
        metrics::MetricDirectory,
        rum::{RumEventWriter, RumIssueTracker, RumSessionMerger, event::RumStream},
        storage::DatasetTypeId,
        stream::{
            Schema, StreamDefinition, StreamRepository, StreamType, is_reserved_system_stream,
            refuse_platform_stream, validate_stream_name,
        },
    },
    shared::{Error, Result, drain::DrainController, ids::Id, time::TimestampMicros},
};

type InternalUsageRecorder = Arc<dyn Fn(&Id, usage::IntakeReceipt, u64) + Send + Sync + 'static>;

pub struct IntakeService {
    sink: Arc<dyn IntakeSink>,
    streams: Arc<dyn StreamRepository>,
    /// bootstrap 解析出的不可变 `_sys` 组织。可信 self-telemetry 入口同时校验
    /// 组织与流名，避免 crate 内调用方把平台自身数据写到普通租户。
    system_org_id: Option<Id>,
    /// 可信内部遥测的原始字节计量回调。外部协议入口已按 wire payload 计量；
    /// 这里只补齐不经过协议门禁的内部写入。
    internal_usage_recorder: Option<InternalUsageRecorder>,
    realtime: Option<realtime::RealtimeEngine>,
    /// 可选脱敏规则源；非空时落盘之前对事件字符串值就地脱敏。
    masking: Option<Arc<dyn MaskingProvider>>,
    /// 可选节点 drain 状态；退役中拒绝新写入（503），让 intake 把 pending flush 干净后下线。
    drain: Option<Arc<DrainController>>,
    /// 可选 trace 观测器；非空时对 Traces 批旁路派生服务调用边（service graph）。
    service_graph: Option<Arc<dyn ServiceGraphObserver>>,
    /// 可选实时告警匹配器；非空时写入成功的记录逐条对 `real_time` 规则求值。
    realtime_alerts: Option<Arc<RealtimeAlerts>>,
    /// 没有 intake 角色的节点：公共入口收到的批次转发给 intake 节点处理。
    forwarder: Option<Arc<dyn IntakeForwarder>>,
    /// 可选指标目录登记器；非空时指标批写入成功后登记流里出现的指标。
    metric_recorder: Option<MetricRecorder>,
    /// 把写入 `rum_sessions` 的 view 补丁合并成会话行。没有它的节点把补丁原样当作会话行。
    rum_sessions: Option<Arc<dyn RumSessionMerger>>,
    /// 把写入 `rum_errors` 的错误归到 issue：开新 issue、记录再现、判回归。没有它的节点照原样写。
    rum_issues: Option<Arc<dyn RumIssueTracker>>,
}

impl IntakeService {
    pub fn new(sink: Arc<dyn IntakeSink>, streams: Arc<dyn StreamRepository>) -> Self {
        Self {
            sink,
            streams,
            system_org_id: None,
            internal_usage_recorder: None,
            realtime: None,
            masking: None,
            drain: None,
            service_graph: None,
            realtime_alerts: None,
            forwarder: None,
            metric_recorder: None,
            rum_sessions: None,
            rum_issues: None,
        }
    }

    /// 注入 RUM 会话合并器：`rum_sessions` 流收到的 view 补丁经它变成会话行。
    pub fn with_rum_sessions(mut self, merger: Arc<dyn RumSessionMerger>) -> Self {
        self.rum_sessions = Some(merger);
        self
    }

    /// 注入 RUM issue 追踪器：`rum_errors` 流收到的错误行经它归到 issue。
    pub fn with_rum_issues(mut self, tracker: Arc<dyn RumIssueTracker>) -> Self {
        self.rum_issues = Some(tracker);
        self
    }

    /// 注入指标目录；指标批写入成功后登记流里出现的指标。
    pub fn with_metric_directory(mut self, directory: Arc<dyn MetricDirectory>) -> Self {
        self.metric_recorder = Some(MetricRecorder::new(directory));
        self
    }

    /// 注入转发器：公共入口收到的批次交给 intake 节点，本节点不再处理。
    pub fn with_forwarder(mut self, forwarder: Arc<dyn IntakeForwarder>) -> Self {
        self.forwarder = Some(forwarder);
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

    /// 注入实时告警匹配器；写入成功的外部来源记录会对 `real_time` 规则求值。
    pub fn with_realtime_alerts(mut self, alerts: Arc<RealtimeAlerts>) -> Self {
        self.realtime_alerts = Some(alerts);
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
    pub async fn intake(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        if is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "`_molesignal` is a protected system stream",
            ));
        }

        refuse_platform_stream(&batch.stream)?;

        validate_stream_name(&batch.stream)?;

        if self
            .drain
            .as_ref()
            .is_some_and(|drain| !drain.accepts_writes())
        {
            return Err(Error::unavailable(
                "node is draining; not accepting new writes",
            ));
        }

        if let Some(forwarder) = &self.forwarder {
            return forwarder.forward(batch).await;
        }

        // Spans are changed where they are written, after tail sampling (`realtime::spans`), so that
        // a trace's summary follows its changed spans; a batch forwarded here is never changed twice.
        if (batch.stream_type == StreamType::LOGS
            || batch.stream_type == StreamType::METRICS
            || batch.stream_type == StreamType::PROFILES)
            && let Some(engine) = &self.realtime
            && let Some(pipeline) = engine
                .repository
                .for_source(&batch.org_id, batch.stream_type, &batch.stream)
                .await?
        {
            return self.route_realtime(batch, pipeline, engine).await;
        }

        let dataset_type = self.sink.primary_dataset_type(batch.stream_type)?;
        self.intake_with_origin(batch, IntakeOrigin::External, dataset_type)
            .await
    }

    /// 定时 pipeline 与 backfill 写目标流的入口。输出本身就是 pipeline 的产物，和实时 pipeline
    /// 路由出来的批次一样不再套目标流上挂的 pipeline（不递归）；其余与标准写入一致：目标流不存在时
    /// 按本批建流，新字段演化 schema，写入前脱敏，并对实时告警规则求值。保留系统流同样拒绝。
    ///
    /// 数据写进本节点自己的 WAL，不转发给 intake 节点：这类结果由产生它的节点负责落盘，
    /// 不依赖 intake 节点是否在线。
    pub(crate) async fn intake_pipeline_output(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        if is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "`_molesignal` is a protected system stream",
            ));
        }

        refuse_platform_stream(&batch.stream)?;

        let dataset_type = self.sink.primary_dataset_type(batch.stream_type)?;
        self.intake_prepared(batch, IntakeOrigin::RoutedExternal, dataset_type, None)
            .await
    }

    /// 可信的服务自身遥测写入入口。不是可反序列化字段，且只在 crate 内开放。
    pub(crate) async fn intake_self_telemetry(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        let system_org_id = self.system_org_id.as_ref().ok_or_else(|| {
            Error::internal("self telemetry system organization is not configured")
        })?;

        if &batch.org_id != system_org_id || !is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "self telemetry may only target `_sys/_molesignal`",
            ));
        }

        let dataset_type = self.sink.primary_dataset_type(batch.stream_type)?;
        self.intake_with_origin(batch, IntakeOrigin::SelfTelemetry, dataset_type)
            .await
    }

    /// 可信的产品内部遥测入口（例如 `_agent_model_traces`）。保留
    /// `_molesignal` 的专用入口，防止其他内部调用伪装成服务自身遥测。
    pub(crate) async fn intake_internal_telemetry(
        &self,
        batch: IntakeBatch,
    ) -> Result<IntakeResult> {
        if !batch.stream.starts_with('_') || is_reserved_system_stream(&batch.stream) {
            return Err(Error::invalid(
                "internal telemetry must target a protected non-self stream",
            ));
        }

        let dataset_type = self.sink.primary_dataset_type(batch.stream_type)?;
        self.intake_with_origin(batch, IntakeOrigin::InternalTelemetry, dataset_type)
            .await
    }

    /// 可信应用服务写入独立的派生物理数据集。它复用 schema 校验/演化和 WAL 语义，
    /// 但不会执行面向外部 payload 的 pipeline，也不会重复派生 service graph。
    pub(crate) async fn intake_derived_dataset(
        &self,
        batch: IntakeBatch,
        dataset_type: DatasetTypeId,
    ) -> Result<IntakeResult> {
        if dataset_type == self.sink.primary_dataset_type(batch.stream_type)? {
            return Err(Error::invalid("derived dataset type must not be primary"));
        }

        if is_reserved_system_stream(&batch.stream) {
            return Err(Error::forbidden(
                "reserved self telemetry requires the dedicated derived entry point",
            ));
        }

        self.intake_with_origin(batch, IntakeOrigin::InternalTelemetry, dataset_type)
            .await
    }

    pub(crate) async fn intake_self_telemetry_dataset(
        &self,
        batch: IntakeBatch,
        dataset_type: DatasetTypeId,
    ) -> Result<IntakeResult> {
        let system_org_id = self.system_org_id.as_ref().ok_or_else(|| {
            Error::internal("self telemetry system organization is not configured")
        })?;

        if dataset_type == self.sink.primary_dataset_type(batch.stream_type)?
            || &batch.org_id != system_org_id
            || !is_reserved_system_stream(&batch.stream)
        {
            return Err(Error::forbidden(
                "derived self telemetry must target `_sys/_molesignal`",
            ));
        }

        self.intake_with_origin(batch, IntakeOrigin::SelfTelemetry, dataset_type)
            .await
    }

    #[tracing::instrument(
        name = "intake.batch",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.intake.origin = ?origin,
            molesignal.intake.signal = ?batch.stream_type,
            molesignal.intake.event_count = batch.events.len()
        )
    )]
    async fn intake_with_origin(
        &self,
        batch: IntakeBatch,
        origin: IntakeOrigin,
        dataset_type: DatasetTypeId,
    ) -> Result<IntakeResult> {
        self.intake_prepared(batch, origin, dataset_type, None)
            .await
    }

    async fn intake_prepared(
        &self,
        mut batch: IntakeBatch,
        origin: IntakeOrigin,
        dataset_type: DatasetTypeId,
        sizes: Option<Vec<u64>>,
    ) -> Result<IntakeResult> {
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
        let internal_usage = (matches!(
            origin,
            IntakeOrigin::InternalTelemetry | IntakeOrigin::SelfTelemetry
        ))
        .then(|| {
            serde_json::to_vec(&batch.events).ok().map(|payload| {
                (
                    batch.org_id.clone(),
                    usage::IntakeReceipt::internal(&batch, payload.len() as u64),
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

        // `rum_sessions` 收到的是 view 补丁：先合并成会话行，之后的 pipeline、脱敏、类型校验
        // 与落盘都只见到会话行。会话行是服务端派生的，不重复计量。
        let derives_sessions = batch.stream_type == StreamType::LOGS
            && batch.stream == RumStream::Sessions.name()
            && self.rum_sessions.is_some();

        let sizes = if derives_sessions && let Some(merger) = &self.rum_sessions {
            let patches = std::mem::take(&mut batch.events);

            batch.events = merger.merge(&batch.org_id, patches).await?;
            None
        } else {
            sizes
        };

        let event_sizes = match sizes {
            Some(sizes) => sizes,
            None => batch
                .events
                .iter()
                .map(crate::domain::intake::usage::event_bytes)
                .collect::<Result<Vec<_>>>()?,
        };

        let mut errors: Vec<IntakeError> = Vec::new();

        // 1) 写入即脱敏（pattern 驱动）：实时流水线产出的字段也会被脱敏。无规则 → no-op。
        if let Some(masking) = &self.masking {
            let masker = masking.intake_masker(&batch.org_id).await?;
            mask_events(&masker, &mut batch.events);
        }

        // `rum_errors` 的行已带着自己的 issue id；这里是它所属组织的错误都经过的地方，由追踪器
        // 开新 issue、记录再现、判回归，并把超出应用每小时上限的错误归入「其余」issue。放在脱敏之后：
        // issue 里留下的消息与落盘的是同一份文字。
        if batch.stream_type == StreamType::LOGS
            && batch.stream == RumStream::Errors.name()
            && let Some(tracker) = &self.rum_issues
        {
            tracker.track(&batch.org_id, &mut batch.events).await;
        }

        // Log-like datasets are queried while new rows continue to arrive.
        // Timestamp alone is not unique, so assign an immutable batch+row id
        // before schema evolution and persistence. RUM streams use Logs too.
        if batch.stream_type == StreamType::LOGS {
            assign_log_event_ids(&batch.batch_id, &mut batch.events);
        }

        // 指标流装一个来源的全部指标：每个样本都要带上自己的 `metric_name`。
        if batch.stream_type == StreamType::METRICS {
            metrics::name_samples(&batch.stream, &mut batch.events);
        }

        // 2) 类型冲突先在内存里剔除
        let mut collected_bytes = 0_u64;
        let mut kept: Vec<RawEvent> = Vec::with_capacity(batch.events.len());

        for (idx, ev) in batch.events.drain(..).enumerate() {
            match check_event_types(&def.schema, &ev) {
                Ok(()) => {
                    collected_bytes = collected_bytes.saturating_add(event_sizes[idx]);
                    kept.push(ev);
                }
                Err(reason) => errors.push(IntakeError { index: idx, reason }),
            }
        }

        let total_kept = kept.len();
        batch.events = kept;

        // 3) schema 演化：新字段按数据流的索引规则定索引。
        if let Some(mut new_schema) =
            infer_schema_extension(&def.schema, &batch.events, batch.stream_type)
        {
            match self.streams.get_settings(&def.id).await {
                Ok(settings) => {
                    apply_index_rules(&mut new_schema, def.schema.fields.len(), &settings);
                }
                Err(error) => tracing::warn!(
                    stream = %def.name,
                    %error,
                    "stream settings unreadable; new fields keep the default indexes"
                ),
            }

            if matches!(
                origin,
                IntakeOrigin::External | IntakeOrigin::RoutedExternal
            ) {
                self.streams.update_schema(&def.id, new_schema).await?;
            } else {
                self.streams
                    .update_schema_internal(&def.id, new_schema)
                    .await?;
            }
        }

        // 3.5) 旁路观测 trace：从 span 派生服务间调用边（service graph）。仅 Traces 批、
        // 仅已留存事件；纯内存累计、不阻塞写入；无观测器时零开销。路由出来的批次（定时 pipeline
        // 的输出）是已经观测过的 span 的副本，再观测会让调用边重复计数。
        let is_primary_dataset =
            dataset_type == self.sink.primary_dataset_type(batch.stream_type)?;

        if is_primary_dataset
            && batch.stream_type == StreamType::TRACES
            && origin != IntakeOrigin::RoutedExternal
            && total_kept > 0
            && let Some(sg) = &self.service_graph
        {
            sg.observe(&batch.org_id, &batch.events);
        }

        // 3.6) 实时告警：对将要写入的记录求值，写入成功后再计入规则窗口。只看外部来源写入的
        // 主数据集；内部遥测与派生数据集不属于规则面向的数据。
        let realtime_hits = if is_primary_dataset
            && total_kept > 0
            && matches!(
                origin,
                IntakeOrigin::External | IntakeOrigin::RoutedExternal
            ) {
            self.realtime_alerts.as_ref().and_then(|alerts| {
                alerts.observe(
                    &batch.org_id,
                    &batch.stream,
                    batch.stream_type,
                    &batch.events,
                )
            })
        } else {
            None
        };

        let pending_metrics = self
            .metric_recorder
            .as_ref()
            .filter(|_| is_primary_dataset && batch.stream_type == StreamType::METRICS)
            .and_then(|recorder| recorder.pending(&def, &batch.events));

        // 4) 写入
        let mut result = if total_kept == 0 {
            IntakeResult {
                accepted: 0,
                rejected: 0,
                errors: Vec::new(),
            }
        } else {
            self.sink
                .write_metered_dataset(
                    dataset_type,
                    batch,
                    (is_primary_dataset && !derives_sessions).then_some(collected_bytes),
                )
                .await?
        };

        if result.accepted > 0
            && let (Some(hits), Some(alerts)) = (realtime_hits, &self.realtime_alerts)
        {
            alerts.record(hits, &result.errors, TimestampMicros::now());
        }

        if result.accepted > 0
            && let Some(pending) = pending_metrics
        {
            pending.record().await;
        }

        result.rejected += errors.len();
        result.errors.extend(errors);

        if result.accepted > 0
            && let Some((org_id, receipt)) = internal_usage
            && let Some(recorder) = &self.internal_usage_recorder
        {
            recorder(&org_id, receipt, result.accepted as u64);
        }

        Ok(result)
    }

    /// 流不存在时按本批事件推断 schema 建流（schema-on-write）。后续 `infer_schema_extension`
    /// 会接管增量演化，这里只负责让首批数据有个落点。并发下两批同时创建相同
    /// `(org, name, type)` 时，不静默复用竞争者创建的定义，而是把 409 冲突直接返回给
    /// 输掉竞争的写入方，避免一次传输在未确认目标定义的情况下继续写入。
    async fn create_stream_on_demand(&self, batch: &IntakeBatch) -> Result<StreamDefinition> {
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
                    "auto-created stream on first intake"
                );
                Ok(created)
            }
            Err(error) => Err(error),
        }
    }
}

/// RUM 入口把按流分好的行批交给公共写入路径：与其它协议适配器一样走转发、pipeline、脱敏、
/// schema 校验与落盘。
#[async_trait::async_trait]
impl RumEventWriter for IntakeService {
    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        self.intake(batch).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntakeOrigin {
    External,
    RoutedExternal,
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

    #[derive(Default)]
    struct RecordingSink {
        batches: Arc<parking_lot::Mutex<Vec<IntakeBatch>>>,
        metered: parking_lot::Mutex<Vec<Option<u64>>>,
    }

    #[async_trait::async_trait]
    impl IntakeSink for RecordingSink {
        async fn write_metered_dataset(
            &self,
            _: DatasetTypeId,
            batch: IntakeBatch,
            bytes: Option<u64>,
        ) -> Result<IntakeResult> {
            self.metered.lock().push(bytes);
            self.write(batch).await
        }
        async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
            let accepted = batch.events.len();
            self.batches.lock().push(batch);
            Ok(IntakeResult {
                accepted,
                rejected: 0,
                errors: Vec::new(),
            })
        }
    }

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
                index_type: None,
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
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        };
        let events = vec![
            ev(json!({"msg":"a", "lat":3})),
            ev(json!({"msg":"b","ok":true})),
        ];
        let next = infer_schema_extension(&s, &events, StreamType::LOGS).expect("extension");
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
        let next = infer_schema_extension(&Schema { fields: vec![] }, &events, StreamType::TRACES)
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
        let logs = infer_schema_extension(&Schema { fields: vec![] }, &events, StreamType::LOGS)
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

    // drain 拒写：退役中 intake() 在触碰 sink/streams 之前就返回 503，故 stub 方法 unreachable。
    struct UnreachableSink;
    #[async_trait::async_trait]
    impl crate::domain::intake::IntakeSink for UnreachableSink {
        async fn write(&self, _: IntakeBatch) -> Result<IntakeResult> {
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
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(ConflictOnCreateStreams));
        let err = svc
            .intake(IntakeBatch {
                batch_id: Id::new(),
                org_id: Id::from_string("org-1"),
                stream: "app_logs".into(),
                stream_type: StreamType::LOGS,
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
    async fn intake_rejected_with_503_when_draining() {
        use crate::shared::drain::DrainController;

        let drain = Arc::new(DrainController::new());
        assert!(drain.begin_drain());
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_drain(drain);
        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: "s".into(),
            stream_type: crate::domain::stream::StreamType::LOGS,
            events: vec![ev(json!({"msg": "x"}))],
            received_at: TimestampMicros::now(),
        };
        let err = svc.intake(batch).await.unwrap_err();
        assert_eq!(err.http_status_code(), 503, "draining intake returns 503");
    }

    #[tokio::test]
    async fn external_intake_cannot_target_system_stream() {
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams));
        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
            stream_type: StreamType::LOGS,
            events: vec![ev(json!({"message": "spoof"}))],
            received_at: TimestampMicros::now(),
        };
        let err = svc.intake(batch).await.unwrap_err();
        assert_eq!(err.http_status_code(), 403);
    }

    /// 可建流、可演化 schema 的内存流仓储。
    #[derive(Default)]
    struct MemStreams(parking_lot::Mutex<Vec<StreamDefinition>>);

    #[async_trait::async_trait]
    impl StreamRepository for MemStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            self.0.lock().push(def.clone());
            Ok(def)
        }
        async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
            self.0
                .lock()
                .iter_mut()
                .find(|stream| &stream.id == id)
                .ok_or_else(|| Error::not_found("stream"))?
                .schema = schema;
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
                .iter()
                .find(|stream| {
                    &stream.org_id == org_id
                        && stream.name == name
                        && stream.stream_type == stream_type
                })
                .cloned()
                .ok_or_else(|| Error::not_found(format!("stream `{name}`")))
        }
        async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(self.0.lock().clone())
        }
        async fn delete(&self, _: &Id) -> Result<()> {
            unreachable!()
        }
    }

    /// 没有 intake 角色的节点装配的转发器：公共入口的批次要转走。
    struct NeverForward;

    #[async_trait::async_trait]
    impl IntakeForwarder for NeverForward {
        async fn forward(&self, _: IntakeBatch) -> Result<IntakeResult> {
            unreachable!("pipeline output is written on the node that produced it")
        }
    }

    fn pipeline_output(stream: &str, events: Vec<RawEvent>) -> IntakeBatch {
        IntakeBatch {
            batch_id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: stream.into(),
            stream_type: StreamType::LOGS,
            events,
            received_at: TimestampMicros::now(),
        }
    }

    #[tokio::test]
    async fn pipeline_output_creates_the_missing_stream_and_stays_on_this_node() {
        let streams = Arc::new(MemStreams::default());
        let sink = Arc::new(RecordingSink::default());
        let svc = IntakeService::new(sink.clone(), streams.clone())
            .with_forwarder(Arc::new(NeverForward));

        let result = svc
            .intake_pipeline_output(pipeline_output(
                "rollup",
                vec![ev(json!({ "message": "a", "n": 1 }))],
            ))
            .await
            .unwrap();

        assert_eq!((result.accepted, result.rejected), (1, 0));
        let created = streams.0.lock()[0].clone();
        assert_eq!(created.name, "rollup");
        let fields: Vec<_> = created
            .schema
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(
            fields.contains(&"message") && fields.contains(&"n"),
            "{fields:?}"
        );
        // 与标准写入一致：logs 事件带上不可变的批次+行号 id。
        let batches = sink.batches.lock();
        assert_eq!(batches.len(), 1);
        assert!(batches[0].events[0].fields.contains_key(EVENT_ID_FIELD));
    }

    #[derive(Default)]
    struct CountingObserver(std::sync::atomic::AtomicUsize);

    impl ServiceGraphObserver for CountingObserver {
        fn observe(&self, _: &Id, events: &[RawEvent]) {
            self.0
                .fetch_add(events.len(), std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// 定时 pipeline 的输出是已观测 span 的副本：写进目标 traces 流不能让服务调用边重复计数。
    #[tokio::test]
    async fn pipeline_output_is_not_observed_for_the_service_graph_again() {
        let observer = Arc::new(CountingObserver::default());
        let svc = IntakeService::new(
            Arc::new(RecordingSink::default()),
            Arc::new(MemStreams::default()),
        )
        .with_service_graph(observer.clone());
        let span = || ev(json!({ "trace_id": "t", "span_id": "s", "service.name": "api" }));
        let traces = |events| IntakeBatch {
            stream_type: StreamType::TRACES,
            ..pipeline_output("spans", events)
        };

        svc.intake(traces(vec![span()])).await.unwrap();
        let observed = observer.0.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(observed, 1, "ordinary intake still feeds the service graph");

        svc.intake_pipeline_output(traces(vec![span(), span()]))
            .await
            .unwrap();
        assert_eq!(
            observer.0.load(std::sync::atomic::Ordering::SeqCst),
            observed
        );
    }

    #[tokio::test]
    async fn pipeline_output_cannot_target_the_system_stream() {
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams));

        let err = svc
            .intake_pipeline_output(pipeline_output(
                crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM,
                vec![ev(json!({ "message": "spoof" }))],
            ))
            .await
            .unwrap_err();

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
    async fn meters_only_accepted_events_before_generated_fields() {
        let org_id = Id::new();
        let now = TimestampMicros::now();
        let streams = Arc::new(ExistingSystemStream {
            def: StreamDefinition {
                id: Id::new(),
                org_id: org_id.clone(),
                name: "metering".into(),
                stream_type: StreamType::LOGS,
                schema: Schema {
                    fields: vec![FieldDef {
                        name: "message".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        index_type: None,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    }],
                },
                retention: None,
                created_at: now,
                updated_at: now,
            },
        });
        let sink = Arc::new(RecordingSink::default());
        let accepted = ev(json!({"message":"中文\n测试"}));
        let expected = crate::domain::intake::usage::event_bytes(&accepted).unwrap();
        let svc = IntakeService::new(sink.clone(), streams);
        let result = svc
            .intake(IntakeBatch {
                batch_id: Id::new(),
                org_id,
                stream: "metering".into(),
                stream_type: StreamType::LOGS,
                events: vec![accepted, ev(json!({"message":42}))],
                received_at: now,
            })
            .await
            .unwrap();
        assert_eq!((result.accepted, result.rejected), (1, 1));
        assert_eq!(*sink.metered.lock(), vec![Some(expected)]);
        assert!(
            sink.batches.lock()[0].events[0]
                .fields
                .contains_key(EVENT_ID_FIELD)
        );
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
                stream_type: StreamType::LOGS,
                schema: Schema { fields: vec![] },
                retention: Some(crate::domain::stream::Retention { days: 7 }),
                created_at: now,
                updated_at: now,
            },
        });
        let sink = Arc::new(RecordingSink::default());
        let batches = sink.batches.clone();
        let usage = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_usage = usage.clone();
        let svc = IntakeService::new(sink, streams)
            .with_system_org_id(org_id.clone())
            .with_internal_usage_recorder(Arc::new(move |org_id, receipt, accepted| {
                captured_usage.lock().expect("usage lock").push((
                    org_id.clone(),
                    receipt,
                    accepted,
                ));
            }));
        let result = svc
            .intake_self_telemetry(IntakeBatch {
                batch_id: Id::new(),
                org_id: org_id.clone(),
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::LOGS,
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
        assert_eq!(
            recorded[0].1.credential.kind,
            crate::domain::intake::usage::source::CredentialKind::Internal
        );
        assert_eq!(
            recorded[0].1.stream,
            crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM
        );
        assert!(recorded[0].1.bytes > 0);
        assert_eq!(recorded[0].2, 1);
    }

    #[tokio::test]
    async fn trusted_self_telemetry_cannot_target_a_tenant_organization() {
        let system_org_id = Id::from_string("system-org");
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_system_org_id(system_org_id);
        let err = svc
            .intake_self_telemetry(IntakeBatch {
                batch_id: Id::new(),
                org_id: Id::from_string("tenant-org"),
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::LOGS,
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
        let svc = IntakeService::new(Arc::new(UnreachableSink), Arc::new(UnreachableStreams))
            .with_system_org_id(system_org_id.clone())
            .with_drain(drain);
        let err = svc
            .intake_self_telemetry(IntakeBatch {
                batch_id: Id::new(),
                org_id: system_org_id,
                stream: crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: StreamType::LOGS,
                events: vec![ev(json!({"message": "late"}))],
                received_at: TimestampMicros::now(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.http_status_code(), 503);
    }
}
