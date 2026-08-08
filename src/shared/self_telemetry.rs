// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! tracing callback 与异步 self-ingest worker 之间的无阻塞桥。

use std::{
    cell::Cell,
    collections::BTreeMap,
    fmt,
    future::Future,
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use prometheus::{IntCounterVec, IntGaugeVec};
use serde_json::{Map, Number, Value};
use tokio::sync::mpsc;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use crate::{
    domain::{
        ingestion::RawEvent,
        metrics::{METRIC_KIND_FIELD, METRIC_NAME_FIELD},
    },
    shared::{
        ids::Id as MoleId,
        metrics::{register_int_counter_vec, register_int_gauge_vec},
        time::TimestampMicros,
        trace_context, trace_metrics,
        trace_normalization::{CanonicalEvent, FinishedSpan, finished_span_to_event},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTelemetrySignal {
    Metrics,
    Traces,
    Profiles,
}

impl SelfTelemetrySignal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Metrics => "metrics",
            Self::Traces => "traces",
            Self::Profiles => "profiles",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceIdentity {
    service_name: String,
    service_version: String,
    deployment_environment: String,
    service_instance_id: String,
    service_role: String,
    node_id: Arc<RwLock<String>>,
}

impl ResourceIdentity {
    pub fn new(
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        deployment_environment: impl Into<String>,
        service_role: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: service_version.into(),
            deployment_environment: deployment_environment.into(),
            service_instance_id: MoleId::new().0,
            service_role: service_role.into(),
            node_id: Arc::new(RwLock::new(node_id.into())),
        }
    }

    pub fn set_node_id(&self, node_id: impl Into<String>) {
        *self.node_id.write().expect("resource node id lock") = node_id.into();
    }

    pub fn service_instance_id(&self) -> &str {
        &self.service_instance_id
    }

    pub fn labels(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("service.namespace".into(), "molesignal".into()),
            ("service.name".into(), self.service_name.clone()),
            ("service.version".into(), self.service_version.clone()),
            (
                "deployment.environment.name".into(),
                self.deployment_environment.clone(),
            ),
            (
                "service.instance.id".into(),
                self.service_instance_id.clone(),
            ),
            ("service.role".into(), self.service_role.clone()),
            ("telemetry.sdk.language".into(), "rust".into()),
            ("telemetry.sdk.name".into(), "molesignal".into()),
            ("telemetry.sdk.version".into(), self.service_version.clone()),
            ("process.runtime.name".into(), "rust".into()),
            (
                "node.id".into(),
                self.node_id.read().expect("resource node id lock").clone(),
            ),
        ])
    }

    pub fn enrich(&self, fields: &mut Map<String, Value>) {
        fields.insert(
            "service.namespace".into(),
            Value::String("molesignal".into()),
        );
        fields.insert(
            "service.name".into(),
            Value::String(self.service_name.clone()),
        );
        fields.insert(
            "service.version".into(),
            Value::String(self.service_version.clone()),
        );
        fields.insert(
            "deployment.environment.name".into(),
            Value::String(self.deployment_environment.clone()),
        );
        fields.insert(
            "service.instance.id".into(),
            Value::String(self.service_instance_id.clone()),
        );
        fields.insert(
            "service.role".into(),
            Value::String(self.service_role.clone()),
        );
        fields.insert(
            "telemetry.sdk.language".into(),
            Value::String("rust".into()),
        );
        fields.insert(
            "telemetry.sdk.name".into(),
            Value::String("molesignal".into()),
        );
        fields.insert(
            "telemetry.sdk.version".into(),
            Value::String(self.service_version.clone()),
        );
        fields.insert("process.runtime.name".into(), Value::String("rust".into()));
        fields.insert(
            "node.id".into(),
            Value::String(self.node_id.read().expect("resource node id lock").clone()),
        );
    }
}

#[derive(Debug, Clone)]
pub struct SelfTelemetryInit {
    pub queue_capacity: usize,
    pub traces_enabled: bool,
    pub resource: ResourceIdentity,
}

struct SignalQueue {
    sender: mpsc::Sender<RawEvent>,
    receiver: Mutex<Option<mpsc::Receiver<RawEvent>>>,
    depth: AtomicUsize,
}

impl SignalQueue {
    fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            depth: AtomicUsize::new(0),
        }
    }
}

pub struct SelfTelemetryHub {
    traces: Option<SignalQueue>,
    accepting: AtomicBool,
    resource: ResourceIdentity,
}

impl SelfTelemetryHub {
    pub fn new(init: SelfTelemetryInit) -> Arc<Self> {
        let traces = init
            .traces_enabled
            .then(|| SignalQueue::new(init.queue_capacity));
        let hub = Arc::new(Self {
            traces,
            accepting: AtomicBool::new(true),
            resource: init.resource,
        });
        if hub.queue(SelfTelemetrySignal::Traces).is_some() {
            health()
                .queue_capacity
                .with_label_values(&[SelfTelemetrySignal::Traces.as_str()])
                .set(init.queue_capacity as i64);
        }
        hub
    }

    pub fn resource(&self) -> &ResourceIdentity {
        &self.resource
    }

    pub fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    pub fn is_accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    pub fn try_send(&self, signal: SelfTelemetrySignal, event: RawEvent) -> bool {
        if !self.is_accepting() || is_suppressed() {
            return false;
        }
        let Some(queue) = self.queue(signal) else {
            return false;
        };
        match queue.sender.try_send(event) {
            Ok(()) => {
                let depth = queue.depth.fetch_add(1, Ordering::Relaxed) + 1;
                health()
                    .accepted
                    .with_label_values(&[signal.as_str()])
                    .inc();
                health()
                    .queue_depth
                    .with_label_values(&[signal.as_str()])
                    .set(depth as i64);
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                record_drop(signal, "queue_full", 1);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                record_drop(signal, "queue_closed", 1);
                false
            }
        }
    }

    pub fn take_receiver(&self, signal: SelfTelemetrySignal) -> Option<mpsc::Receiver<RawEvent>> {
        self.queue(signal)?
            .receiver
            .lock()
            .expect("self telemetry receiver lock")
            .take()
    }

    pub fn record_dequeued(&self, signal: SelfTelemetrySignal, count: usize) {
        let Some(queue) = self.queue(signal) else {
            return;
        };
        let remaining = queue
            .depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
                Some(previous.saturating_sub(count))
            })
            .unwrap_or_default()
            .saturating_sub(count);
        health()
            .queue_depth
            .with_label_values(&[signal.as_str()])
            .set(remaining as i64);
    }

    pub fn pending_depth(&self, signal: SelfTelemetrySignal) -> usize {
        self.queue(signal)
            .map(|queue| queue.depth.load(Ordering::Relaxed))
            .unwrap_or_default()
    }

    fn queue(&self, signal: SelfTelemetrySignal) -> Option<&SignalQueue> {
        match signal {
            SelfTelemetrySignal::Traces => self.traces.as_ref(),
            SelfTelemetrySignal::Metrics | SelfTelemetrySignal::Profiles => None,
        }
    }
}

struct SelfTelemetryHealth {
    accepted: IntCounterVec,
    dropped: IntCounterVec,
    batches: IntCounterVec,
    retries: IntCounterVec,
    queue_depth: IntGaugeVec,
    queue_capacity: IntGaugeVec,
    last_success_unixtime: IntGaugeVec,
    profile_available: IntGaugeVec,
}

fn health() -> &'static SelfTelemetryHealth {
    static HEALTH: OnceLock<SelfTelemetryHealth> = OnceLock::new();
    HEALTH.get_or_init(|| SelfTelemetryHealth {
        accepted: register_int_counter_vec(
            "self_telemetry_accepted_total",
            "Self telemetry records accepted into a local queue.",
            &["signal"],
        ),
        dropped: register_int_counter_vec(
            "self_telemetry_dropped_total",
            "Self telemetry records dropped before durable ingestion.",
            &["signal", "reason"],
        ),
        batches: register_int_counter_vec(
            "self_telemetry_batches_total",
            "Self telemetry ingest batch attempts by outcome.",
            &["signal", "outcome"],
        ),
        retries: register_int_counter_vec(
            "self_telemetry_retries_total",
            "Self telemetry delivery retries.",
            &["signal", "reason"],
        ),
        queue_depth: register_int_gauge_vec(
            "self_telemetry_queue_depth",
            "Current self telemetry queue depth.",
            &["signal"],
        ),
        queue_capacity: register_int_gauge_vec(
            "self_telemetry_queue_capacity",
            "Configured self telemetry queue capacity.",
            &["signal"],
        ),
        last_success_unixtime: register_int_gauge_vec(
            "self_telemetry_last_success_unixtime",
            "Unix timestamp of the last successful self telemetry batch.",
            &["signal"],
        ),
        profile_available: register_int_gauge_vec(
            "self_telemetry_profile_available",
            "Whether a self profile kind is available on this build.",
            &["kind"],
        ),
    })
}

pub fn record_drop(signal: SelfTelemetrySignal, reason: &'static str, count: u64) {
    health()
        .dropped
        .with_label_values(&[signal.as_str(), reason])
        .inc_by(count);
}

pub fn record_accepted(signal: SelfTelemetrySignal, count: u64) {
    health()
        .accepted
        .with_label_values(&[signal.as_str()])
        .inc_by(count);
}

pub fn record_batch(signal: SelfTelemetrySignal, success: bool) {
    health()
        .batches
        .with_label_values(&[signal.as_str(), if success { "success" } else { "error" }])
        .inc();
    if success {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        health()
            .last_success_unixtime
            .with_label_values(&[signal.as_str()])
            .set(now);
    }
}

pub fn record_retry(signal: SelfTelemetrySignal, reason: &'static str) {
    health()
        .retries
        .with_label_values(&[signal.as_str(), reason])
        .inc();
}

pub fn set_profile_available(kind: &'static str, available: bool) {
    health()
        .profile_available
        .with_label_values(&[kind])
        .set(i64::from(available));
}

tokio::task_local! {
    static TASK_SUPPRESSED: ();
}

thread_local! {
    static THREAD_SUPPRESSED: Cell<u32> = const { Cell::new(0) };
}

pub async fn with_suppression<F: Future>(future: F) -> F::Output {
    TASK_SUPPRESSED.scope((), future).await
}

pub fn is_suppressed() -> bool {
    TASK_SUPPRESSED.try_with(|_| ()).is_ok()
        || THREAD_SUPPRESSED.with(|suppressed| suppressed.get() > 0)
}

pub struct ThreadSuppressionGuard;

pub fn enter_thread_suppression() -> ThreadSuppressionGuard {
    THREAD_SUPPRESSED.with(|suppressed| suppressed.set(suppressed.get().saturating_add(1)));
    ThreadSuppressionGuard
}

impl Drop for ThreadSuppressionGuard {
    fn drop(&mut self) {
        THREAD_SUPPRESSED.with(|suppressed| suppressed.set(suppressed.get().saturating_sub(1)));
    }
}

pub struct SelfTelemetryLayer {
    hub: Arc<SelfTelemetryHub>,
}

impl SelfTelemetryLayer {
    pub fn new(hub: Arc<SelfTelemetryHub>) -> Self {
        Self { hub }
    }

    pub fn traces(hub: Arc<SelfTelemetryHub>) -> Self {
        Self::new(hub)
    }
}

#[derive(Debug)]
struct SpanData {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    name: String,
    target: String,
    execution_role: String,
    started_at: TimestampMicros,
    started: Instant,
    attributes: Map<String, Value>,
    events: Vec<CanonicalEvent>,
}

fn normalize_execution_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "router" => Some("router"),
        "querier" => Some("querier"),
        "ingester" => Some("ingester"),
        "compactor" => Some("compactor"),
        "alert_manager" => Some("alert_manager"),
        "standalone" => Some("standalone"),
        _ => None,
    }
}

fn classify_execution_role(name: &str, target: &str) -> Option<&'static str> {
    if target.contains("compactor") || name.contains("compaction") {
        Some("compactor")
    } else if target.contains("alert")
        || target.contains("incident")
        || target.contains("notification")
        || name.starts_with("alert.")
        || name.starts_with("notification.")
    {
        Some("alert_manager")
    } else if target.contains("::query")
        || target.contains("flight")
        || name.starts_with("query.")
        || name.starts_with("promql.")
        || name.starts_with("datafusion.")
        || name.starts_with("federation.")
    {
        Some("querier")
    } else if target.contains("ingest")
        || target.contains("::wal")
        || name.starts_with("ingest.")
        || name.starts_with("wal.")
        || name.starts_with("parquet.")
    {
        Some("ingester")
    } else if target.contains("api::http") || name == "http.server" {
        Some("router")
    } else {
        None
    }
}

fn execution_role(
    name: &str,
    target: &str,
    attributes: &Map<String, Value>,
    parent: Option<&str>,
    process: &str,
) -> &'static str {
    attributes
        .get("molesignal.execution.role")
        .and_then(Value::as_str)
        .and_then(normalize_execution_role)
        .or_else(|| classify_execution_role(name, target))
        .or_else(|| parent.and_then(normalize_execution_role))
        .or_else(|| normalize_execution_role(process))
        .unwrap_or("standalone")
}

fn deny_target(target: &str) -> bool {
    target.starts_with("molesignal::shared::self_telemetry")
        || target.starts_with("molesignal::app::self_telemetry")
}

impl<S> Layer<S> for SelfTelemetryLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if is_suppressed() || deny_target(attrs.metadata().target()) {
            return;
        }
        let parent = attrs
            .parent()
            .and_then(|parent_id| ctx.span(parent_id))
            .or_else(|| {
                attrs
                    .is_contextual()
                    .then(|| ctx.lookup_current())
                    .flatten()
            });
        let parent_data = parent.as_ref().and_then(|span| {
            span.extensions().get::<SpanData>().map(|data| {
                (
                    data.trace_id.clone(),
                    data.span_id.clone(),
                    data.execution_role.clone(),
                )
            })
        });
        let mut attributes = Map::new();
        attrs.record(&mut JsonVisitor::new(&mut attributes));
        let execution_role = execution_role(
            attrs.metadata().name(),
            attrs.metadata().target(),
            &attributes,
            parent_data.as_ref().map(|(_, _, role)| role.as_str()),
            &self.hub.resource.service_role,
        )
        .to_owned();
        attributes.insert(
            "molesignal.execution.role".into(),
            Value::String(execution_role.clone()),
        );
        let explicit_trace_id = attributes
            .remove("otel.trace_id")
            .or_else(|| attributes.remove("trace_id"))
            .and_then(|value| value.as_str().map(str::to_owned));
        let explicit_span_id = attributes
            .remove("otel.span_id")
            .or_else(|| attributes.remove("span_id"))
            .and_then(|value| value.as_str().map(str::to_owned));
        let explicit_parent_span_id = attributes
            .remove("otel.parent_span_id")
            .or_else(|| attributes.remove("parent_span_id"))
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty());
        let data = SpanData {
            trace_id: explicit_trace_id
                .or_else(|| {
                    parent_data
                        .as_ref()
                        .map(|(trace_id, _, _)| trace_id.clone())
                })
                .unwrap_or_else(trace_context::new_trace_id),
            span_id: explicit_span_id.unwrap_or_else(trace_context::new_span_id),
            parent_span_id: explicit_parent_span_id
                .or_else(|| parent_data.as_ref().map(|(_, span_id, _)| span_id.clone())),
            name: attrs.metadata().name().to_string(),
            target: attrs.metadata().target().to_string(),
            execution_role,
            started_at: TimestampMicros::now(),
            started: Instant::now(),
            attributes,
            events: Vec::new(),
        };
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(data);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(data) = extensions.get_mut::<SpanData>() {
            values.record(&mut JsonVisitor::new(&mut data.attributes));
            if let Some(role) = data
                .attributes
                .get("molesignal.execution.role")
                .and_then(Value::as_str)
                .and_then(normalize_execution_role)
            {
                data.execution_role = role.to_owned();
                data.attributes.insert(
                    "molesignal.execution.role".into(),
                    Value::String(role.to_owned()),
                );
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if is_suppressed() || deny_target(event.metadata().target()) {
            return;
        }
        let mut fields = Map::new();
        event.record(&mut JsonVisitor::new(&mut fields));
        let explicit_span_event = fields
            .remove("molesignal.span_event")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let explicit_event_name = fields
            .remove("otel.event.name")
            .and_then(|value| value.as_str().map(str::to_owned));
        let explicit_event_attributes: BTreeMap<String, Value> =
            fields.clone().into_iter().collect();

        if explicit_span_event && let Some(span) = ctx.lookup_current() {
            let mut extensions = span.extensions_mut();
            if let Some(data) = extensions.get_mut::<SpanData>() {
                data.events.push(CanonicalEvent {
                    time_unix_nano: (TimestampMicros::now().0.max(0) as u64).saturating_mul(1_000),
                    name: explicit_event_name.unwrap_or_else(|| "event".into()),
                    attributes: explicit_event_attributes,
                    dropped_attributes_count: 0,
                });
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        if is_suppressed() {
            return;
        }
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(data) = span.extensions_mut().remove::<SpanData>() else {
            return;
        };
        if deny_target(&data.target) {
            return;
        }
        let duration_ns = data.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let start_ns = (data.started_at.0.max(0) as u64).saturating_mul(1_000);
        let end_ns = start_ns.saturating_add(duration_ns);
        let mut fields = data.attributes;
        for key in ["links", "events"] {
            if let Some(Value::String(encoded)) = fields.get(key) {
                match serde_json::from_str::<Value>(encoded) {
                    Ok(decoded @ Value::Array(_)) => {
                        fields.insert(key.into(), decoded);
                    }
                    _ => {
                        fields.remove(key);
                    }
                }
            }
        }
        fields.insert("target".into(), Value::String(data.target));
        fields.insert(
            "molesignal.execution.role".into(),
            Value::String(data.execution_role),
        );
        let status = match fields.get("otel.status_code").and_then(Value::as_str) {
            Some("ERROR" | "error") => "ERROR",
            Some("OK" | "ok") => "OK",
            _ if fields.get("error").and_then(Value::as_bool) == Some(true) => "ERROR",
            _ => "UNSET",
        };
        if !data.events.is_empty() {
            fields.insert(
                "events".into(),
                serde_json::to_value(data.events).unwrap_or(Value::Array(Vec::new())),
            );
        }
        self.hub.resource.enrich(&mut fields);
        let event = finished_span_to_event(
            fields,
            FinishedSpan {
                name: data.name,
                trace_id: Some(data.trace_id),
                span_id: Some(data.span_id),
                parent_span_id: data.parent_span_id,
                kind: 0,
                start_time_unix_nano: start_ns,
                end_time_unix_nano: end_ns,
                status_code: status.into(),
                status_message: None,
            },
        );
        let partial = event
            .fields
            .get("partial")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let accepted = self.hub.try_send(SelfTelemetrySignal::Traces, event);
        trace_metrics::record_spans(
            "generated",
            if accepted { "accepted" } else { "dropped" },
            1,
        );
        if partial {
            trace_metrics::record_spans("generated", "partial", 1);
        }
    }
}

struct JsonVisitor<'a> {
    fields: &'a mut Map<String, Value>,
}

impl<'a> JsonVisitor<'a> {
    fn new(fields: &'a mut Map<String, Value>) -> Self {
        Self { fields }
    }
}

impl Visit for JsonVisitor<'_> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields.insert(
            field.name().into(),
            Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().into(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().into(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().into(), Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().into(), Value::String(value.into()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.fields
            .insert(field.name().into(), Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields.insert(
            field.name().into(),
            Value::String(format!("{value:?}").trim_matches('"').to_string()),
        );
    }
}

pub fn metric_samples_to_events(
    samples: impl IntoIterator<Item = crate::shared::metrics::MetricSample>,
    resource: &ResourceIdentity,
    timestamp: TimestampMicros,
) -> Vec<RawEvent> {
    samples
        .into_iter()
        .map(|sample| {
            let mut fields = Map::new();
            fields.insert(METRIC_NAME_FIELD.into(), Value::String(sample.metric_name));
            fields.insert(
                METRIC_KIND_FIELD.into(),
                Value::String(sample.metric_kind.into()),
            );
            fields.insert(
                "value".into(),
                Number::from_f64(sample.value)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            );
            for (key, value) in sample.labels {
                fields.insert(key, Value::String(value));
            }
            resource.enrich(&mut fields);
            RawEvent { timestamp, fields }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::prelude::*;

    use super::*;

    fn init(capacity: usize) -> SelfTelemetryInit {
        SelfTelemetryInit {
            queue_capacity: capacity,
            traces_enabled: true,
            resource: ResourceIdentity::new("molesignal", "test", "test", "standalone", "node-1"),
        }
    }

    #[test]
    fn queue_is_non_blocking_and_drops_when_full() {
        let hub = SelfTelemetryHub::new(init(1));
        let event = RawEvent {
            timestamp: TimestampMicros::now(),
            fields: Map::new(),
        };
        assert!(hub.try_send(SelfTelemetrySignal::Traces, event.clone()));
        assert!(!hub.try_send(SelfTelemetrySignal::Traces, event));
    }

    #[test]
    fn queue_depth_never_exceeds_configured_capacity_under_burst() {
        let hub = SelfTelemetryHub::new(init(4));
        let mut accepted = 0;
        for _ in 0..10_000 {
            accepted += usize::from(hub.try_send(
                SelfTelemetrySignal::Traces,
                RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: Map::new(),
                },
            ));
        }
        assert_eq!(accepted, 4);
        assert_eq!(hub.pending_depth(SelfTelemetrySignal::Traces), 4);
    }

    #[test]
    fn tracing_layer_records_spans_with_resource_identity() {
        let hub = SelfTelemetryHub::new(init(8));
        let mut traces = hub.take_receiver(SelfTelemetrySignal::Traces).unwrap();
        let subscriber = tracing_subscriber::registry().with(SelfTelemetryLayer::new(hub));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(target: "test.app", "request", method = "GET");
            let _entered = span.enter();
        });
        let trace = traces.try_recv().unwrap();
        assert_eq!(trace.fields["service.namespace"], "molesignal");
        assert_eq!(trace.fields["deployment.environment.name"], "test");
        assert_eq!(trace.fields["telemetry.sdk.language"], "rust");
        assert_eq!(trace.fields["telemetry.sdk.name"], "molesignal");
        assert_eq!(trace.fields["telemetry.sdk.version"], "test");
        assert_eq!(trace.fields["process.runtime.name"], "rust");
        assert_eq!(
            trace.fields["molesignal.execution.role"],
            Value::String("standalone".into())
        );
    }

    #[test]
    fn execution_role_is_explicit_or_derived_from_the_actual_stage() {
        let mut explicit = Map::new();
        explicit.insert(
            "molesignal.execution.role".into(),
            Value::String("alert-manager".into()),
        );
        assert_eq!(
            execution_role(
                "query.execute",
                "molesignal::app::query",
                &explicit,
                None,
                "router"
            ),
            "alert_manager"
        );
        assert_eq!(
            execution_role(
                "query.execute",
                "molesignal::app::query",
                &Map::new(),
                Some("router"),
                "standalone"
            ),
            "querier"
        );
        assert_eq!(
            execution_role(
                "object_store.operation",
                "molesignal::infra::storage",
                &Map::new(),
                Some("ingester"),
                "standalone"
            ),
            "ingester"
        );
    }

    #[test]
    fn queued_execution_preserves_a_canonical_link() {
        let hub = SelfTelemetryHub::new(init(8));
        let mut traces = hub.take_receiver(SelfTelemetrySignal::Traces).unwrap();
        let subscriber = tracing_subscriber::registry().with(SelfTelemetryLayer::traces(hub));
        let producer = crate::shared::trace_context::TraceContext::new_root("request-1");
        let link = producer.serialized_link();
        tracing::subscriber::with_default(subscriber, || {
            let (_context, span) =
                crate::shared::trace_context::linked_execution_root(Some(&link), "search_job");
            let _entered = span.enter();
        });

        let trace = traces.try_recv().unwrap();
        let links = trace.fields.get("links").and_then(Value::as_array).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].get("trace_id").and_then(Value::as_str),
            Some(producer.trace_id.as_str())
        );
        assert_eq!(
            links[0].get("span_id").and_then(Value::as_str),
            Some(producer.span_id.as_str())
        );
    }

    #[tokio::test]
    async fn suppression_blocks_internal_events() {
        let hub = SelfTelemetryHub::new(init(8));
        let mut traces = hub.take_receiver(SelfTelemetrySignal::Traces).unwrap();
        with_suppression(async {
            let event = RawEvent {
                timestamp: TimestampMicros::now(),
                fields: Map::new(),
            };
            assert!(!hub.try_send(SelfTelemetrySignal::Traces, event));
        })
        .await;
        assert!(traces.try_recv().is_err());
    }

    #[test]
    fn metric_conversion_includes_resource_identity() {
        let resource = ResourceIdentity::new("molesignal", "1", "test", "ingester", "node-1");
        let events = metric_samples_to_events(
            [crate::shared::metrics::MetricSample {
                metric_name: "requests_total".into(),
                metric_kind: "counter",
                value: 2.0,
                labels: BTreeMap::from([("method".into(), "GET".into())]),
            }],
            &resource,
            TimestampMicros(7),
        );
        assert_eq!(events[0].fields["service.name"], "molesignal");
        assert_eq!(events[0].fields["deployment.environment.name"], "test");
        assert_eq!(events[0].fields["telemetry.sdk.language"], "rust");
        assert_eq!(events[0].fields["method"], "GET");
    }
}
