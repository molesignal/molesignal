// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 服务自身遥测的异步批处理与 role-aware delivery。
//!
//! `tracing` callback 永远只写 bounded channel；本模块在 bootstrap 完成后取得
//! receiver，按事件数/延迟成批，并在 suppression scope 内直接调用可信 ingestion
//! 或集群内部 gRPC。任何失败都只影响 self telemetry，不反压业务请求。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};

use crate::{
    app::{
        cluster::ClusterRegistry,
        ingestion::IngestService,
        profile_storage::ProfileStorageService,
        profiling::{CaptureError, CapturedProfile, ProfilingService},
        trace::candidate_router::TraceCandidateRouter,
    },
    config::SelfCollectSettings,
    domain::{
        ingestion::{IngestBatch, RawEvent},
        stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
    },
    infra::cluster::grpc_channel,
    protocol::ingest::v1::{
        PushRequest, StreamType as ProtoStreamType, ingest_service_client::IngestServiceClient,
    },
    shared::{
        Error, Result,
        ids::Id,
        metrics::gather_structured,
        self_telemetry::{
            SelfTelemetryHub, SelfTelemetrySignal, metric_samples_to_events, record_accepted,
            record_batch, record_drop, record_retry, with_suppression,
        },
        tail_sampling::{ForceKeep, TraceCandidate},
        time::TimestampMicros,
        trace_normalization::CanonicalSpan,
    },
};

/// 分角色内部 RPC 的共享 token。该 token 只来自进程环境，不写入日志或数据流。
pub const CLUSTER_TOKEN_ENV: &str = "MS_SELF_TELEMETRY_CLUSTER_TOKEN";
const INTERNAL_ORIGIN_HEADER: &str = "x-molesignal-internal-origin";
const INTERNAL_ORIGIN_VALUE: &str = "self-telemetry";
const MAX_REMOTE_ATTEMPTS: usize = 3;
const MAX_REMOTE_DELIVERY_AGE: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct SelfProfileContext {
    pub profiling: Arc<ProfilingService>,
    pub storage: Arc<ProfileStorageService>,
}

pub fn configured_cluster_token() -> Option<String> {
    std::env::var(CLUSTER_TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// 固定时间比较，避免内部 bearer 的明显 timing oracle。
pub fn cluster_token_matches(expected: &str, presented: &str) -> bool {
    if expected.len() != presented.len() {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(presented.as_bytes())
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

#[async_trait]
trait Delivery: Send + Sync {
    async fn deliver(
        &self,
        org_id: &Id,
        signal: SelfTelemetrySignal,
        events: Vec<RawEvent>,
    ) -> std::result::Result<(), String>;
}

struct LocalDelivery {
    ingestion: Arc<IngestService>,
}

#[async_trait]
impl Delivery for LocalDelivery {
    async fn deliver(
        &self,
        org_id: &Id,
        signal: SelfTelemetrySignal,
        events: Vec<RawEvent>,
    ) -> std::result::Result<(), String> {
        let result = self
            .ingestion
            .ingest_self_telemetry(make_batch(org_id, signal, events))
            .await
            .map_err(|error| error.to_string())?;
        if result.rejected == 0 {
            Ok(())
        } else {
            Err(format!(
                "{} self telemetry records rejected",
                result.rejected
            ))
        }
    }
}

struct RemoteDelivery {
    registry: Arc<dyn ClusterRegistry>,
    token: String,
}

#[async_trait]
impl Delivery for RemoteDelivery {
    async fn deliver(
        &self,
        org_id: &Id,
        signal: SelfTelemetrySignal,
        events: Vec<RawEvent>,
    ) -> std::result::Result<(), String> {
        let started = Instant::now();
        let payload =
            serde_json::to_vec(&events).map_err(|error| format!("encode failed: {error}"))?;
        let mut last_error = "no ingester available".to_string();
        for attempt in 0..MAX_REMOTE_ATTEMPTS {
            if attempt > 0 {
                record_retry(signal, retry_reason(&last_error));
                let backoff = Duration::from_millis(100u64 << (attempt - 1));
                if started.elapsed().saturating_add(backoff) >= MAX_REMOTE_DELIVERY_AGE {
                    break;
                }
                sleep(backoff).await;
            }
            let remaining = MAX_REMOTE_DELIVERY_AGE.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }

            let attempt_result = timeout(remaining, async {
                let peer = self
                    .registry
                    .pick_ingester(org_id, MOLESIGNAL_SYSTEM_STREAM)
                    .await
                    .ok_or_else(|| "no ingester available".to_string())?;
                let channel = grpc_channel::connect(&peer.advertise_addr, false)
                    .await
                    .map_err(|error| format!("connect failed: {error}"))?;
                let mut request = tonic::Request::new(PushRequest {
                    batch_id: Id::new().0,
                    org_id: org_id.0.clone(),
                    stream: MOLESIGNAL_SYSTEM_STREAM.into(),
                    stream_type: proto_stream_type(signal) as i32,
                    payload: payload.clone().into(),
                    received_at_micros: TimestampMicros::now().0,
                });
                grpc_channel::with_bearer(&mut request, &self.token)?;
                request.metadata_mut().insert(
                    INTERNAL_ORIGIN_HEADER,
                    INTERNAL_ORIGIN_VALUE
                        .parse()
                        .map_err(|_| "invalid internal origin metadata".to_string())?,
                );
                let mut client = IngestServiceClient::new(channel);
                let response = crate::shared::grpc_trace::call(
                    request,
                    "ingest.v1.IngestService",
                    "Push",
                    crate::shared::grpc_trace::GrpcTarget::Internal,
                    |request| client.push(request),
                )
                .await
                .map_err(|status| format!("push failed: {}", status.code()))?;
                if response.get_ref().rejected == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "ingester rejected {} records",
                        response.get_ref().rejected
                    ))
                }
            })
            .await;
            match attempt_result {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) => last_error = error,
                Err(_) => {
                    last_error = "delivery deadline exceeded".into();
                    break;
                }
            }
        }
        Err(last_error)
    }
}

fn retry_reason(error: &str) -> &'static str {
    if error.starts_with("no ingester") {
        "no_ingester"
    } else if error.starts_with("connect") {
        "connect"
    } else {
        "push"
    }
}

fn proto_stream_type(signal: SelfTelemetrySignal) -> ProtoStreamType {
    match signal {
        SelfTelemetrySignal::Logs => ProtoStreamType::Logs,
        SelfTelemetrySignal::Metrics => ProtoStreamType::Metrics,
        SelfTelemetrySignal::Traces => ProtoStreamType::Traces,
        SelfTelemetrySignal::Profiles => ProtoStreamType::Profiles,
    }
}

fn stream_type(signal: SelfTelemetrySignal) -> StreamType {
    match signal {
        SelfTelemetrySignal::Logs => StreamType::Logs,
        SelfTelemetrySignal::Metrics => StreamType::Metrics,
        SelfTelemetrySignal::Traces => StreamType::Traces,
        SelfTelemetrySignal::Profiles => StreamType::Profiles,
    }
}

fn make_batch(org_id: &Id, signal: SelfTelemetrySignal, events: Vec<RawEvent>) -> IngestBatch {
    IngestBatch {
        batch_id: Id::new(),
        org_id: org_id.clone(),
        stream: MOLESIGNAL_SYSTEM_STREAM.into(),
        stream_type: stream_type(signal),
        events,
        received_at: TimestampMicros::now(),
    }
}

/// 已激活的 runtime handle。`stop_and_flush` 可重复调用，只有第一次真正关停。
pub struct SelfTelemetryRuntime {
    hub: Arc<SelfTelemetryHub>,
    org_id: Id,
    delivery: Arc<dyn Delivery>,
    profile_context: Option<SelfProfileContext>,
    stop_tx: watch::Sender<bool>,
    handles: Mutex<Option<Vec<JoinHandle<()>>>>,
    stopped: AtomicBool,
    flush_timeout: Duration,
}

#[derive(Clone)]
struct WorkerContext {
    hub: Arc<SelfTelemetryHub>,
    org_id: Id,
    delivery: Arc<dyn Delivery>,
    trace_candidates: Option<Arc<TraceCandidateRouter>>,
}

struct SignalWorkerSettings {
    signal: SelfTelemetrySignal,
    max_events: usize,
    max_delay: Duration,
}

struct ProfilesWorkerSettings {
    profile_context: SelfProfileContext,
    kinds: Vec<String>,
    interval: Duration,
    cpu_duration_secs: u32,
}

impl SelfTelemetryRuntime {
    pub fn start_local(
        hub: Arc<SelfTelemetryHub>,
        org_id: Id,
        settings: SelfCollectSettings,
        ingestion: Arc<IngestService>,
        profile_context: Option<SelfProfileContext>,
    ) -> Arc<Self> {
        Self::start(
            hub,
            org_id,
            settings,
            Arc::new(LocalDelivery { ingestion }),
            profile_context,
            None,
        )
    }

    pub fn start_local_with_trace_candidates(
        hub: Arc<SelfTelemetryHub>,
        org_id: Id,
        settings: SelfCollectSettings,
        ingestion: Arc<IngestService>,
        profile_context: Option<SelfProfileContext>,
        trace_candidates: Arc<TraceCandidateRouter>,
    ) -> Arc<Self> {
        Self::start(
            hub,
            org_id,
            settings,
            Arc::new(LocalDelivery { ingestion }),
            profile_context,
            Some(trace_candidates),
        )
    }

    pub fn start_remote(
        hub: Arc<SelfTelemetryHub>,
        org_id: Id,
        settings: SelfCollectSettings,
        registry: Arc<dyn ClusterRegistry>,
        token: String,
        profile_context: Option<SelfProfileContext>,
    ) -> Result<Arc<Self>> {
        if token.trim().is_empty() {
            return Err(Error::invalid(format!(
                "{CLUSTER_TOKEN_ENV} must be set on split-role nodes when self ingestion is enabled"
            )));
        }
        Ok(Self::start(
            hub,
            org_id,
            settings,
            Arc::new(RemoteDelivery { registry, token }),
            profile_context,
            None,
        ))
    }

    pub fn start_remote_with_trace_candidates(
        hub: Arc<SelfTelemetryHub>,
        org_id: Id,
        settings: SelfCollectSettings,
        registry: Arc<dyn ClusterRegistry>,
        token: String,
        profile_context: Option<SelfProfileContext>,
        trace_candidates: Arc<TraceCandidateRouter>,
    ) -> Result<Arc<Self>> {
        if token.trim().is_empty() {
            return Err(Error::invalid(format!(
                "{CLUSTER_TOKEN_ENV} must be set on split-role nodes when self ingestion is enabled"
            )));
        }
        Ok(Self::start(
            hub,
            org_id,
            settings,
            Arc::new(RemoteDelivery { registry, token }),
            profile_context,
            Some(trace_candidates),
        ))
    }

    fn start(
        hub: Arc<SelfTelemetryHub>,
        org_id: Id,
        settings: SelfCollectSettings,
        delivery: Arc<dyn Delivery>,
        profile_context: Option<SelfProfileContext>,
        trace_candidates: Option<Arc<TraceCandidateRouter>>,
    ) -> Arc<Self> {
        let (stop_tx, stop_rx) = watch::channel(false);
        let mut handles = Vec::new();
        let worker_context = WorkerContext {
            hub: hub.clone(),
            org_id: org_id.clone(),
            delivery: delivery.clone(),
            trace_candidates,
        };

        for signal in [SelfTelemetrySignal::Logs, SelfTelemetrySignal::Traces] {
            if let Some(receiver) = hub.take_receiver(signal) {
                handles.push(tokio::spawn(with_suppression(run_signal_worker(
                    worker_context.clone(),
                    SignalWorkerSettings {
                        signal,
                        max_events: settings.batch_max_events,
                        max_delay: Duration::from_millis(settings.batch_max_delay_ms),
                    },
                    receiver,
                    stop_rx.clone(),
                ))));
            }
        }

        if settings.enabled && settings.metrics_enabled {
            handles.push(tokio::spawn(with_suppression(run_metrics_worker(
                worker_context.clone(),
                settings.batch_max_events,
                Duration::from_secs(settings.metrics_interval_secs),
                stop_rx.clone(),
            ))));
        }

        if settings.enabled
            && let Some(context) = profile_context.clone()
        {
            handles.push(tokio::spawn(with_suppression(run_profiles_worker(
                worker_context,
                ProfilesWorkerSettings {
                    profile_context: context,
                    kinds: settings.profile_kinds.clone(),
                    interval: Duration::from_secs(settings.profile_interval_secs),
                    cpu_duration_secs: settings.profile_duration_secs as u32,
                },
                stop_rx,
            ))));
        }

        Arc::new(Self {
            hub,
            org_id,
            delivery,
            profile_context,
            stop_tx,
            handles: Mutex::new(Some(handles)),
            stopped: AtomicBool::new(false),
            flush_timeout: Duration::from_secs(settings.flush_timeout_secs),
        })
    }

    /// 停 producer/callback，随后在固定 deadline 内冲刷队列；超时后 abort worker，
    /// 让进程继续进入 WAL drain。
    pub async fn stop_and_flush(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        self.hub.stop_accepting();
        let _ = self.stop_tx.send(true);
        let Some(mut handles) = self
            .handles
            .lock()
            .expect("self telemetry handles lock")
            .take()
        else {
            return;
        };

        let joined = timeout(self.flush_timeout, async {
            for handle in &mut handles {
                let _ = handle.await;
            }
        })
        .await;
        if joined.is_err() {
            for handle in handles {
                handle.abort();
            }
            for signal in [SelfTelemetrySignal::Logs, SelfTelemetrySignal::Traces] {
                let pending = self.hub.pending_depth(signal);
                if pending > 0 {
                    record_drop(signal, "flush_timeout", pending as u64);
                }
            }
            tracing::warn!(
                target: "molesignal::app::self_telemetry",
                timeout_secs = self.flush_timeout.as_secs(),
                "self telemetry flush timed out"
            );
        }
    }

    /// on-demand pprof 响应完成后的异步自归档入口；与 scheduled capture 共用
    /// role-aware metadata delivery。
    pub async fn persist_profile(
        &self,
        captured: CapturedProfile,
    ) -> std::result::Result<(), String> {
        let context = self
            .profile_context
            .as_ref()
            .ok_or_else(|| "self profile persistence is disabled".to_string())?;
        persist_profile_with(
            self.hub.as_ref(),
            &self.org_id,
            self.delivery.as_ref(),
            context,
            captured,
        )
        .await
    }
}

async fn run_signal_worker(
    context: WorkerContext,
    settings: SignalWorkerSettings,
    mut receiver: mpsc::Receiver<RawEvent>,
    mut stop_rx: watch::Receiver<bool>,
) {
    loop {
        let first = tokio::select! {
            biased;
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    break;
                }
                continue;
            }
            event = receiver.recv() => event,
        };
        let Some(first) = first else {
            break;
        };
        let mut batch = vec![first];
        let deadline = sleep(settings.max_delay);
        tokio::pin!(deadline);
        while batch.len() < settings.max_events {
            tokio::select! {
                biased;
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = &mut deadline => break,
                event = receiver.recv() => match event {
                    Some(event) => batch.push(event),
                    None => break,
                },
            }
            if *stop_rx.borrow() {
                break;
            }
        }
        flush_batch(
            settings.signal,
            context.hub.as_ref(),
            &context.org_id,
            context.delivery.as_ref(),
            context.trace_candidates.as_deref(),
            batch,
        )
        .await;
        if *stop_rx.borrow() {
            break;
        }
    }

    // sender 由 hub 持有，不会自然 close；收到 stop 后显式 drain 当前缓冲。
    loop {
        let mut batch = Vec::with_capacity(settings.max_events);
        while batch.len() < settings.max_events {
            match receiver.try_recv() {
                Ok(event) => batch.push(event),
                Err(_) => break,
            }
        }
        if batch.is_empty() {
            break;
        }
        flush_batch(
            settings.signal,
            context.hub.as_ref(),
            &context.org_id,
            context.delivery.as_ref(),
            context.trace_candidates.as_deref(),
            batch,
        )
        .await;
    }
}

async fn flush_batch(
    signal: SelfTelemetrySignal,
    hub: &SelfTelemetryHub,
    org_id: &Id,
    delivery: &dyn Delivery,
    trace_candidates: Option<&TraceCandidateRouter>,
    batch: Vec<RawEvent>,
) {
    let count = batch.len();
    hub.record_dequeued(signal, count);
    if signal == SelfTelemetrySignal::Traces
        && let Some(trace_candidates) = trace_candidates
    {
        let mut dropped = 0_u64;
        for event in batch {
            match CanonicalSpan::try_from_raw_event(&event) {
                Ok(span) => {
                    if trace_candidates
                        .try_submit(TraceCandidate {
                            org_id: org_id.0.clone(),
                            stream: None,
                            span,
                            force_keep: ForceKeep::None,
                        })
                        .is_err()
                    {
                        dropped += 1;
                    }
                }
                Err(_) => dropped += 1,
            }
        }
        record_batch(signal, dropped == 0);
        if dropped > 0 {
            record_drop(signal, "trace_candidate_queue", dropped);
        }
        return;
    }
    match delivery.deliver(org_id, signal, batch).await {
        Ok(()) => record_batch(signal, true),
        Err(error) => {
            record_batch(signal, false);
            record_drop(signal, "delivery_failed", count as u64);
            tracing::warn!(
                target: "molesignal::app::self_telemetry",
                signal = signal.as_str(),
                error = %error,
                "self telemetry batch delivery failed"
            );
        }
    }
}

async fn run_metrics_worker(
    context: WorkerContext,
    max_events: usize,
    interval: Duration,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval_at(Instant::now() + interval, interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            _ = tick.tick() => {
                let events = metric_samples_to_events(
                    gather_structured(),
                    context.hub.resource(),
                    TimestampMicros::now(),
                );
                record_accepted(SelfTelemetrySignal::Metrics, events.len() as u64);
                for batch in events.chunks(max_events) {
                    let owned = batch.to_vec();
                    let count = owned.len();
                    match context.delivery
                        .deliver(&context.org_id, SelfTelemetrySignal::Metrics, owned)
                        .await
                    {
                        Ok(()) => record_batch(SelfTelemetrySignal::Metrics, true),
                        Err(error) => {
                            record_batch(SelfTelemetrySignal::Metrics, false);
                            record_drop(
                                SelfTelemetrySignal::Metrics,
                                "delivery_failed",
                                count as u64,
                            );
                            tracing::warn!(
                                target: "molesignal::app::self_telemetry",
                                error = %error,
                                "self telemetry metrics delivery failed"
                            );
                        }
                    }
                }
            }
        }
    }
}

async fn run_profiles_worker(
    context: WorkerContext,
    settings: ProfilesWorkerSettings,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval_at(Instant::now() + settings.interval, settings.interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            _ = tick.tick() => {
                for kind in &settings.kinds {
                    let captured = match kind.as_str() {
                        "cpu" => settings
                            .profile_context
                            .profiling
                            .capture_cpu(settings.cpu_duration_secs)
                            .await,
                        "heap" => settings.profile_context.profiling.capture_heap().await,
                        _ => continue,
                    };
                    match captured {
                        Ok(captured) => {
                            if let Err(error) = persist_profile_with(
                                context.hub.as_ref(),
                                &context.org_id,
                                context.delivery.as_ref(),
                                &settings.profile_context,
                                captured,
                            )
                            .await
                            {
                                tracing::warn!(
                                    target: "molesignal::app::self_telemetry",
                                    kind,
                                    error = %error,
                                    "scheduled self profile persistence failed"
                                );
                            }
                        }
                        Err(error) => {
                            let reason = match error {
                                CaptureError::Busy => "capture_busy",
                                CaptureError::Unavailable(_) => "unavailable",
                                CaptureError::InvalidDuration => "invalid_duration",
                                CaptureError::Failed(_) => "capture_failed",
                            };
                            record_drop(SelfTelemetrySignal::Profiles, reason, 1);
                            tracing::warn!(
                                target: "molesignal::app::self_telemetry",
                                kind,
                                error = %error,
                                "scheduled self profile capture failed"
                            );
                        }
                    }
                    if *stop_rx.borrow() {
                        return;
                    }
                }
            }
        }
    }
}

async fn persist_profile_with(
    hub: &SelfTelemetryHub,
    org_id: &Id,
    delivery: &dyn Delivery,
    context: &SelfProfileContext,
    mut captured: CapturedProfile,
) -> std::result::Result<(), String> {
    captured.normalized.labels.extend(hub.resource().labels());
    record_accepted(SelfTelemetrySignal::Profiles, 1);
    let event = context
        .storage
        .archive_metadata_event(org_id, &captured.normalized, &captured.raw_pprof)
        .await
        .map_err(|error| error.to_string())?;
    match delivery
        .deliver(org_id, SelfTelemetrySignal::Profiles, vec![event])
        .await
    {
        Ok(()) => {
            record_batch(SelfTelemetrySignal::Profiles, true);
            Ok(())
        }
        Err(error) => {
            record_batch(SelfTelemetrySignal::Profiles, false);
            record_drop(SelfTelemetrySignal::Profiles, "persistence_failed", 1);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::Map;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status, transport::Server};

    use super::*;
    use crate::{
        app::cluster::{PeerInfo, PeerRole},
        protocol::ingest::v1::{
            PushResponse,
            ingest_service_server::{IngestService as ProtoIngestService, IngestServiceServer},
        },
        shared::self_telemetry::{ResourceIdentity, SelfTelemetryInit},
    };

    struct RecordingDelivery {
        calls: AtomicUsize,
        events: AtomicUsize,
    }

    struct NeverDelivery;

    #[async_trait]
    impl Delivery for NeverDelivery {
        async fn deliver(
            &self,
            _org_id: &Id,
            _signal: SelfTelemetrySignal,
            _events: Vec<RawEvent>,
        ) -> std::result::Result<(), String> {
            std::future::pending().await
        }
    }

    fn runtime_settings() -> SelfCollectSettings {
        SelfCollectSettings {
            enabled: true,
            metrics_enabled: false,
            queue_capacity: 8,
            batch_max_events: 2,
            batch_max_delay_ms: 10,
            flush_timeout_secs: 1,
            ..SelfCollectSettings::default()
        }
    }

    #[async_trait]
    impl Delivery for RecordingDelivery {
        async fn deliver(
            &self,
            _org_id: &Id,
            _signal: SelfTelemetrySignal,
            events: Vec<RawEvent>,
        ) -> std::result::Result<(), String> {
            tracing::warn!(target: "molesignal::app::self_telemetry", "delivery diagnostic");
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.events.fetch_add(events.len(), Ordering::Relaxed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn batches_and_flushes_without_recursive_enqueue() {
        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 8,
            logs_enabled: true,
            traces_enabled: false,
            resource: ResourceIdentity::new("molesignal", "test", "test", "test", "node"),
        });
        let delivery = Arc::new(RecordingDelivery {
            calls: AtomicUsize::new(0),
            events: AtomicUsize::new(0),
        });
        let logs = hub.take_receiver(SelfTelemetrySignal::Logs).unwrap();
        // Put the receiver back through the worker test seam: run it directly so the
        // suppression scope covers diagnostics emitted by delivery.
        for _ in 0..3 {
            assert!(hub.try_send(
                SelfTelemetrySignal::Logs,
                RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: Map::new(),
                },
            ));
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker = tokio::spawn(with_suppression(run_signal_worker(
            WorkerContext {
                hub: hub.clone(),
                org_id: Id::new(),
                delivery: delivery.clone(),
                trace_candidates: None,
            },
            SignalWorkerSettings {
                signal: SelfTelemetrySignal::Logs,
                max_events: 2,
                max_delay: Duration::from_millis(10),
            },
            logs,
            stop_rx,
        )));
        tokio::time::sleep(Duration::from_millis(30)).await;
        hub.stop_accepting();
        stop_tx.send(true).unwrap();
        worker.await.unwrap();
        assert_eq!(delivery.events.load(Ordering::Relaxed), 3);
        assert_eq!(hub.pending_depth(SelfTelemetrySignal::Logs), 0);
    }

    #[tokio::test]
    async fn runtime_stop_flushes_pending_records_before_returning() {
        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 8,
            logs_enabled: true,
            traces_enabled: false,
            resource: ResourceIdentity::new("molesignal", "test", "test", "test", "node"),
        });
        let delivery = Arc::new(RecordingDelivery {
            calls: AtomicUsize::new(0),
            events: AtomicUsize::new(0),
        });
        let runtime = SelfTelemetryRuntime::start(
            hub.clone(),
            Id::new(),
            runtime_settings(),
            delivery.clone(),
            None,
            None,
        );
        for _ in 0..3 {
            assert!(hub.try_send(
                SelfTelemetrySignal::Logs,
                RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: Map::new(),
                },
            ));
        }
        runtime.stop_and_flush().await;
        assert_eq!(delivery.events.load(Ordering::Relaxed), 3);
        assert_eq!(hub.pending_depth(SelfTelemetrySignal::Logs), 0);
    }

    #[tokio::test]
    async fn runtime_flush_timeout_is_bounded() {
        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 1,
            logs_enabled: true,
            traces_enabled: false,
            resource: ResourceIdentity::new("molesignal", "test", "test", "test", "node"),
        });
        let runtime = SelfTelemetryRuntime::start(
            hub.clone(),
            Id::new(),
            runtime_settings(),
            Arc::new(NeverDelivery),
            None,
            None,
        );
        assert!(hub.try_send(
            SelfTelemetrySignal::Logs,
            RawEvent {
                timestamp: TimestampMicros::now(),
                fields: Map::new(),
            },
        ));
        let started = Instant::now();
        runtime.stop_and_flush().await;
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn cluster_token_comparison_is_exact() {
        assert!(cluster_token_matches("secret", "secret"));
        assert!(!cluster_token_matches("secret", "Secret"));
        assert!(!cluster_token_matches("secret", "secret-longer"));
    }

    struct FixedRegistry {
        peer: PeerInfo,
    }

    #[async_trait]
    impl ClusterRegistry for FixedRegistry {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            vec![self.peer.clone()]
        }
    }

    struct EmptyCountingRegistry {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ClusterRegistry for EmptyCountingRegistry {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Vec::new()
        }
    }

    #[tokio::test]
    async fn remote_delivery_retries_are_bounded() {
        let calls = Arc::new(AtomicUsize::new(0));
        let delivery = RemoteDelivery {
            registry: Arc::new(EmptyCountingRegistry {
                calls: calls.clone(),
            }),
            token: "secret".into(),
        };
        let started = Instant::now();
        let result = delivery
            .deliver(
                &Id::new(),
                SelfTelemetrySignal::Logs,
                vec![RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: Map::new(),
                }],
            )
            .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), MAX_REMOTE_ATTEMPTS);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[derive(Clone)]
    struct CapturingIngest {
        seen: Arc<Mutex<Vec<RawEvent>>>,
    }

    #[tonic::async_trait]
    impl ProtoIngestService for CapturingIngest {
        async fn push(
            &self,
            request: Request<PushRequest>,
        ) -> std::result::Result<Response<PushResponse>, Status> {
            if request
                .metadata()
                .get(INTERNAL_ORIGIN_HEADER)
                .and_then(|value| value.to_str().ok())
                != Some(INTERNAL_ORIGIN_VALUE)
                || request
                    .metadata()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer secret")
            {
                return Err(Status::unauthenticated("missing internal auth"));
            }
            let events: Vec<RawEvent> = serde_json::from_slice(&request.get_ref().payload)
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
            let accepted = events.len() as u32;
            self.seen.lock().unwrap().extend(events);
            Ok(Response::new(PushResponse {
                accepted,
                rejected: 0,
                errors: Vec::new(),
            }))
        }
    }

    #[tokio::test]
    async fn remote_delivery_authenticates_and_preserves_producer_identity() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let service = CapturingIngest { seen: seen.clone() };
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(IngestServiceServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let delivery = RemoteDelivery {
            registry: Arc::new(FixedRegistry {
                peer: PeerInfo {
                    node_id: "ingester".into(),
                    advertise_addr: address.to_string(),
                    roles: vec![PeerRole::Ingester],
                },
            }),
            token: "secret".into(),
        };
        let event = RawEvent {
            timestamp: TimestampMicros::now(),
            fields: Map::from_iter([(
                "node.id".into(),
                serde_json::Value::String("producer-querier".into()),
            )]),
        };
        delivery
            .deliver(&Id::new(), SelfTelemetrySignal::Logs, vec![event])
            .await
            .unwrap();
        assert_eq!(
            seen.lock().unwrap()[0].fields["node.id"],
            "producer-querier"
        );
        server.abort();
    }
}
