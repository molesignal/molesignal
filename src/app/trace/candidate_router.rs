// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 非阻塞 CanonicalSpan producer 路由。
//!
//! 每个进程只向有界本地 channel 写入候选。后台 worker 从活跃的 ingester/querier/
//! standalone 节点中按 `trace_id` rendezvous 选一个 sampler owner；本机 owner 直接
//! 投递本地 pipeline，远端 owner 使用集群认证 RPC。传输没有复制或 Trace WAL。

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    api::grpc::trace::candidate_server::{
        MAX_TRACE_CANDIDATE_BYTES, TRACE_CANDIDATE_ORIGIN_HEADER, TRACE_CANDIDATE_ORIGIN_VALUE,
    },
    app::{
        cluster::{ClusterRegistry, PeerInfo, PeerRole},
        trace::{TracePipeline, TraceSubmitError},
    },
    infra::cluster::grpc_channel,
    protocol::cluster::v1::{
        SubmitTraceCandidateRequest, TraceCandidateDisposition, TraceForceKeep,
        trace_candidate_service_client::TraceCandidateServiceClient,
    },
    shared::{
        tail_sampling::{ForceKeep, SamplerNode, TraceCandidate, rendezvous_owner},
        time::TimestampMicros,
        trace_metrics,
        trace_normalization::{TraceLimits, sanitize_and_limit_span},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct TraceCandidateRouterConfig {
    pub queue_capacity: usize,
    pub workers: usize,
    pub max_attempts: usize,
    pub attempt_timeout: Duration,
    pub max_delivery_age: Duration,
    pub initial_backoff: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for TraceCandidateRouterConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 16_384,
            workers: 8,
            max_attempts: 3,
            attempt_timeout: Duration::from_secs(3),
            max_delivery_age: Duration::from_secs(10),
            initial_backoff: Duration::from_millis(100),
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

impl TraceCandidateRouterConfig {
    fn validate(self) -> Result<Self, String> {
        if self.queue_capacity == 0
            || self.workers == 0
            || self.max_attempts == 0
            || self.attempt_timeout.is_zero()
            || self.max_delivery_age.is_zero()
            || self.initial_backoff.is_zero()
            || self.shutdown_timeout.is_zero()
        {
            return Err("Trace candidate routing bounds must be non-zero".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Default)]
struct RouterMetrics {
    delivered_local: AtomicU64,
    delivered_remote: AtomicU64,
    queue_full: AtomicU64,
    no_owner: AtomicU64,
    transport_failed: AtomicU64,
    owner_overloaded: AtomicU64,
    expired: AtomicU64,
    invalid: AtomicU64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TraceCandidateRouterHealth {
    pub accepting: bool,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub delivered_local: u64,
    pub delivered_remote: u64,
    pub queue_full: u64,
    pub no_owner: u64,
    pub transport_failed: u64,
    pub owner_overloaded: u64,
    pub expired: u64,
    pub invalid: u64,
}

struct QueuedCandidate {
    candidate: TraceCandidate,
    enqueued_at: Instant,
}

struct RouterWorkerContext {
    registry: Arc<dyn ClusterRegistry>,
    local_node_id: String,
    local_advertise_addr: String,
    local_accepts_candidates: bool,
    cluster_token: Option<Arc<str>>,
    local_pipeline: Arc<TracePipeline>,
    config: TraceCandidateRouterConfig,
    metrics: Arc<RouterMetrics>,
}

pub struct TraceCandidateRouter {
    sender: mpsc::Sender<QueuedCandidate>,
    depth: Arc<AtomicUsize>,
    capacity: usize,
    accepting: AtomicBool,
    metrics: Arc<RouterMetrics>,
    cancel: CancellationToken,
    joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    shutdown_timeout: Duration,
    limits: TraceLimits,
}

impl TraceCandidateRouter {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        registry: Arc<dyn ClusterRegistry>,
        local_node_id: String,
        local_advertise_addr: String,
        local_accepts_candidates: bool,
        cluster_token: Option<Arc<str>>,
        local_pipeline: Arc<TracePipeline>,
        config: TraceCandidateRouterConfig,
        limits: TraceLimits,
    ) -> Result<Arc<Self>, String> {
        let config = config.validate()?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let depth = Arc::new(AtomicUsize::new(0));
        trace_metrics::set_queue("routing", 0, config.queue_capacity);
        let metrics = Arc::new(RouterMetrics::default());
        let cancel = CancellationToken::new();
        let context = Arc::new(RouterWorkerContext {
            registry,
            local_node_id,
            local_advertise_addr,
            local_accepts_candidates,
            cluster_token,
            local_pipeline,
            config,
            metrics: metrics.clone(),
        });
        let mut joins = Vec::with_capacity(config.workers);
        for _ in 0..config.workers {
            let receiver = receiver.clone();
            let depth = depth.clone();
            let cancel = cancel.clone();
            let context = context.clone();
            joins.push(tokio::spawn(async move {
                loop {
                    let queued = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            let mut receiver = receiver.lock().await;
                            receiver.close();
                            receiver.recv().await
                        }
                        queued = async {
                            let mut receiver = receiver.lock().await;
                            receiver.recv().await
                        } => queued,
                    };
                    let Some(queued) = queued else {
                        break;
                    };
                    depth.fetch_sub(1, Ordering::Relaxed);
                    trace_metrics::set_queue(
                        "routing",
                        depth.load(Ordering::Relaxed),
                        context.config.queue_capacity,
                    );
                    deliver_candidate(context.as_ref(), queued).await;
                }
            }));
        }
        Ok(Arc::new(Self {
            sender,
            depth,
            capacity: config.queue_capacity,
            accepting: AtomicBool::new(true),
            metrics,
            cancel,
            joins: Mutex::new(Some(joins)),
            shutdown_timeout: config.shutdown_timeout,
            limits,
        }))
    }

    /// producer 热路径：先执行中央 sanitizer/limits，再尝试写入本地有界队列。
    pub fn try_submit(
        &self,
        mut candidate: TraceCandidate,
    ) -> Result<(), TraceCandidateSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            trace_metrics::record_spans("routing", "stopped", 1);
            return Err(TraceCandidateSubmitError::Stopped);
        }
        sanitize_and_limit_span(&mut candidate.span, self.limits);
        let encoded_len = serde_json::to_vec(&candidate.span)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        if encoded_len > MAX_TRACE_CANDIDATE_BYTES {
            self.metrics.invalid.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "invalid", 1);
            return Err(TraceCandidateSubmitError::Invalid);
        }
        self.depth.fetch_add(1, Ordering::Relaxed);
        trace_metrics::set_queue("routing", self.depth.load(Ordering::Relaxed), self.capacity);
        match self.sender.try_send(QueuedCandidate {
            candidate,
            enqueued_at: Instant::now(),
        }) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.depth.fetch_sub(1, Ordering::Relaxed);
                self.metrics.queue_full.fetch_add(1, Ordering::Relaxed);
                trace_metrics::set_queue(
                    "routing",
                    self.depth.load(Ordering::Relaxed),
                    self.capacity,
                );
                trace_metrics::record_spans("routing", "queue_full", 1);
                Err(TraceCandidateSubmitError::Full)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.depth.fetch_sub(1, Ordering::Relaxed);
                trace_metrics::set_queue(
                    "routing",
                    self.depth.load(Ordering::Relaxed),
                    self.capacity,
                );
                trace_metrics::record_spans("routing", "stopped", 1);
                Err(TraceCandidateSubmitError::Stopped)
            }
        }
    }

    pub fn health(&self) -> TraceCandidateRouterHealth {
        TraceCandidateRouterHealth {
            accepting: self.accepting.load(Ordering::Acquire),
            queue_depth: self.depth.load(Ordering::Relaxed),
            queue_capacity: self.capacity,
            delivered_local: self.metrics.delivered_local.load(Ordering::Relaxed),
            delivered_remote: self.metrics.delivered_remote.load(Ordering::Relaxed),
            queue_full: self.metrics.queue_full.load(Ordering::Relaxed),
            no_owner: self.metrics.no_owner.load(Ordering::Relaxed),
            transport_failed: self.metrics.transport_failed.load(Ordering::Relaxed),
            owner_overloaded: self.metrics.owner_overloaded.load(Ordering::Relaxed),
            expired: self.metrics.expired.load(Ordering::Relaxed),
            invalid: self.metrics.invalid.load(Ordering::Relaxed),
        }
    }

    /// 停 producer，drain 本地候选与有界传输，然后由 caller 关闭 owner pipeline。
    pub async fn shutdown(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        self.cancel.cancel();
        let Some(mut joins) = self.joins.lock().await.take() else {
            return;
        };
        if timeout(self.shutdown_timeout, async {
            for join in &mut joins {
                let _ = join.await;
            }
        })
        .await
        .is_err()
        {
            for join in joins {
                join.abort();
            }
            let residue = self.depth.swap(0, Ordering::Relaxed);
            self.metrics
                .expired
                .fetch_add(residue as u64, Ordering::Relaxed);
            trace_metrics::set_queue("routing", 0, self.capacity);
            trace_metrics::record_spans("routing", "dropped", residue as u64);
            tracing::warn!(
                target: "molesignal::app::trace::candidate_router",
                candidate_residue = residue,
                "Trace candidate routing shutdown timed out"
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCandidateSubmitError {
    Full,
    Stopped,
    Invalid,
}

async fn deliver_candidate(context: &RouterWorkerContext, queued: QueuedCandidate) {
    let mut failed_nodes = HashSet::new();
    let mut last_failure_was_no_owner = false;
    let canonical_json = match serde_json::to_vec(&queued.candidate.span) {
        Ok(value) if value.len() <= MAX_TRACE_CANDIDATE_BYTES => value,
        _ => {
            context.metrics.invalid.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "invalid", 1);
            return;
        }
    };
    for attempt in 0..context.config.max_attempts {
        if queued.enqueued_at.elapsed() >= context.config.max_delivery_age {
            context.metrics.expired.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "dropped", 1);
            return;
        }
        if attempt > 0 {
            trace_metrics::record_retry("routing", "delivery_failed");
            let shift = (attempt - 1).min(16) as u32;
            let backoff = context
                .config
                .initial_backoff
                .saturating_mul(1_u32 << shift);
            if queued.enqueued_at.elapsed().saturating_add(backoff)
                >= context.config.max_delivery_age
            {
                context.metrics.expired.fetch_add(1, Ordering::Relaxed);
                trace_metrics::record_spans("routing", "dropped", 1);
                return;
            }
            sleep(backoff).await;
        }

        let peers = sampler_peers(context).await;
        let mut nodes = peers
            .iter()
            .filter(|peer| !failed_nodes.contains(&peer.node_id))
            .map(peer_to_sampler_node)
            .collect::<Vec<_>>();
        let mut owner = rendezvous_owner(&queued.candidate.span.trace_id, &nodes).cloned();
        if owner.is_none() && !failed_nodes.is_empty() {
            // 单 owner 集群仍允许在下一次 bounded attempt 重试原节点。
            failed_nodes.clear();
            nodes = peers.iter().map(peer_to_sampler_node).collect();
            owner = rendezvous_owner(&queued.candidate.span.trace_id, &nodes).cloned();
        }
        let Some(owner) = owner else {
            last_failure_was_no_owner = true;
            continue;
        };
        last_failure_was_no_owner = false;

        if context.local_accepts_candidates && owner.node_id == context.local_node_id {
            match context.local_pipeline.try_submit(queued.candidate.clone()) {
                Ok(()) => {
                    context
                        .metrics
                        .delivered_local
                        .fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("routing", "accepted", 1);
                    return;
                }
                Err(TraceSubmitError::Full) => {
                    context
                        .metrics
                        .owner_overloaded
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(TraceSubmitError::Stopped) => {
                    failed_nodes.insert(owner.node_id);
                    continue;
                }
            }
        }

        let Some(token) = context.cluster_token.as_deref() else {
            context
                .metrics
                .transport_failed
                .fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "dropped", 1);
            return;
        };
        let request = build_request(context, &queued.candidate, canonical_json.clone(), token);
        let result = timeout(context.config.attempt_timeout, async {
            let channel = grpc_channel::connect(&owner.endpoint, false).await?;
            let mut client = TraceCandidateServiceClient::new(channel);
            crate::shared::grpc_trace::call(
                request,
                "cluster.v1.TraceCandidateService",
                "Submit",
                crate::shared::grpc_trace::GrpcTarget::Internal,
                |request| client.submit(request),
            )
            .await
            .map_err(|status| format!("candidate submit failed: {}", status.code()))
        })
        .await;
        match result {
            Ok(Ok(response))
                if TraceCandidateDisposition::try_from(response.get_ref().disposition)
                    .unwrap_or(TraceCandidateDisposition::Unspecified)
                    == TraceCandidateDisposition::Accepted =>
            {
                context
                    .metrics
                    .delivered_remote
                    .fetch_add(1, Ordering::Relaxed);
                trace_metrics::record_spans("routing", "accepted", 1);
                return;
            }
            Ok(Ok(response))
                if TraceCandidateDisposition::try_from(response.get_ref().disposition)
                    .unwrap_or(TraceCandidateDisposition::Unspecified)
                    == TraceCandidateDisposition::Overloaded =>
            {
                context
                    .metrics
                    .owner_overloaded
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                failed_nodes.insert(owner.node_id);
            }
        }
    }
    if last_failure_was_no_owner {
        context.metrics.no_owner.fetch_add(1, Ordering::Relaxed);
    } else {
        context
            .metrics
            .transport_failed
            .fetch_add(1, Ordering::Relaxed);
    }
    trace_metrics::record_spans("routing", "dropped", 1);
}

async fn sampler_peers(context: &RouterWorkerContext) -> Vec<PeerInfo> {
    let mut peers = context
        .registry
        .list_all()
        .await
        .into_iter()
        .filter(|peer| {
            peer.roles.iter().any(|role| {
                matches!(
                    role,
                    PeerRole::Standalone | PeerRole::Ingester | PeerRole::Querier
                )
            })
        })
        .collect::<Vec<_>>();
    if context.local_accepts_candidates
        && !peers
            .iter()
            .any(|peer| peer.node_id == context.local_node_id)
    {
        peers.push(PeerInfo {
            node_id: context.local_node_id.clone(),
            advertise_addr: context.local_advertise_addr.clone(),
            roles: vec![PeerRole::Standalone],
        });
    }
    peers.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    peers.dedup_by(|left, right| left.node_id == right.node_id);
    peers
}

fn peer_to_sampler_node(peer: &PeerInfo) -> SamplerNode {
    SamplerNode {
        node_id: peer.node_id.clone(),
        endpoint: peer.advertise_addr.clone(),
        healthy: true,
    }
}

fn build_request(
    context: &RouterWorkerContext,
    candidate: &TraceCandidate,
    canonical_json: Vec<u8>,
    token: &str,
) -> tonic::Request<SubmitTraceCandidateRequest> {
    let force_keep = match candidate.force_keep {
        ForceKeep::None => TraceForceKeep::Unspecified,
        ForceKeep::TrustedInternal => TraceForceKeep::TrustedInternal,
        ForceKeep::DebugToken => TraceForceKeep::DebugToken,
    };
    let mut request = tonic::Request::new(SubmitTraceCandidateRequest {
        org_id: candidate.org_id.clone(),
        stream: candidate.stream.clone().unwrap_or_default(),
        system_self_trace: candidate.stream.is_none(),
        canonical_span_json: canonical_json.into(),
        force_keep: force_keep as i32,
        producer_node_id: context.local_node_id.clone(),
        produced_at_micros: TimestampMicros::now().0,
    });
    // Both headers are required. Metadata construction cannot fail for these static values and
    // the token was already parsed from a process environment variable.
    let _ = grpc_channel::with_bearer(&mut request, token);
    request.metadata_mut().insert(
        TRACE_CANDIDATE_ORIGIN_HEADER,
        TRACE_CANDIDATE_ORIGIN_VALUE.parse().unwrap(),
    );
    request
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    use super::*;
    use crate::{
        api::grpc::trace::candidate_server::TraceCandidateGrpc,
        app::trace::{MemoryTraceSink, TracePipelineConfig, TraceSinkWorkerConfig},
        domain::ingestion::ServiceGraphObserver,
        infra::traces::{ServiceGraphAggregator, ServiceGraphObserverImpl},
        shared::trace_normalization::CanonicalSpan,
    };

    struct FixedRegistry(Vec<PeerInfo>);

    #[async_trait]
    impl ClusterRegistry for FixedRegistry {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            self.0.clone()
        }
    }

    fn peer(id: &str, role: PeerRole) -> PeerInfo {
        PeerInfo {
            node_id: id.into(),
            advertise_addr: format!("{id}:5081"),
            roles: vec![role],
        }
    }

    fn local_pipeline() -> Arc<TracePipeline> {
        let sampler = Arc::new(
            crate::shared::tail_sampling::TailSampler::new(
                crate::shared::tail_sampling::TraceRuntimePolicy::default(),
                false,
                TraceLimits::default(),
            )
            .unwrap(),
        );
        TracePipeline::start(
            sampler,
            None,
            None,
            crate::app::trace::TracePipelineConfig::default(),
            TraceLimits::default(),
        )
        .unwrap()
    }

    fn fast_pipeline_config() -> TracePipelineConfig {
        let sink = TraceSinkWorkerConfig {
            queue_capacity: 32,
            batch_size: 32,
            batch_delay: Duration::from_millis(2),
            export_timeout: Duration::from_secs(1),
            max_attempts: 1,
            initial_backoff: Duration::from_millis(1),
        };
        TracePipelineConfig {
            candidate_capacity: 32,
            decision_tick: Duration::from_millis(2),
            shutdown_timeout: Duration::from_secs(2),
            self_ingest: sink,
            external: sink,
        }
    }

    fn role_span(
        trace_id: &str,
        span_id: &str,
        parent_span_id: Option<&str>,
        name: &str,
        role: &str,
        node_id: &str,
    ) -> CanonicalSpan {
        let mut span = CanonicalSpan::new(
            trace_id.into(),
            span_id.into(),
            name.into(),
            1,
            1_000_000_000,
            1_001_000_000,
        );
        span.parent_span_id = parent_span_id.map(str::to_owned);
        span.resource
            .attributes
            .insert("service.namespace".into(), serde_json::json!("molesignal"));
        span.resource
            .attributes
            .insert("service.name".into(), serde_json::json!("molesignal"));
        span.resource
            .attributes
            .insert("node.id".into(), serde_json::json!(node_id));
        span.attributes
            .insert("molesignal.execution.role".into(), serde_json::json!(role));
        span
    }

    fn remote_router(
        registry: Arc<dyn ClusterRegistry>,
        node_id: &str,
        token: Arc<str>,
        pipeline: Arc<TracePipeline>,
    ) -> Arc<TraceCandidateRouter> {
        TraceCandidateRouter::start(
            registry,
            node_id.into(),
            format!("{node_id}:5081"),
            false,
            Some(token),
            pipeline,
            TraceCandidateRouterConfig {
                queue_capacity: 16,
                workers: 1,
                max_attempts: 3,
                attempt_timeout: Duration::from_secs(1),
                max_delivery_age: Duration::from_secs(3),
                initial_backoff: Duration::from_millis(5),
                shutdown_timeout: Duration::from_secs(2),
            },
            TraceLimits::default(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn sampler_candidates_exclude_nodes_without_internal_grpc() {
        let context = RouterWorkerContext {
            registry: Arc::new(FixedRegistry(vec![
                peer("router", PeerRole::Router),
                peer("worker", PeerRole::Compactor),
                peer("ingester", PeerRole::Ingester),
                peer("querier", PeerRole::Querier),
            ])),
            local_node_id: "router".into(),
            local_advertise_addr: "router:5081".into(),
            local_accepts_candidates: false,
            cluster_token: None,
            local_pipeline: local_pipeline(),
            config: TraceCandidateRouterConfig::default(),
            metrics: Arc::new(RouterMetrics::default()),
        };
        let peers = sampler_peers(&context).await;
        assert_eq!(
            peers
                .iter()
                .map(|peer| peer.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ingester", "querier"]
        );
    }

    #[test]
    fn rendezvous_choice_does_not_depend_on_org_or_stream() {
        let peers = [peer("a", PeerRole::Ingester), peer("b", PeerRole::Querier)];
        let nodes = peers.iter().map(peer_to_sampler_node).collect::<Vec<_>>();
        let first = rendezvous_owner("0123456789abcdef0123456789abcdef", &nodes)
            .unwrap()
            .node_id
            .clone();
        let second = rendezvous_owner("0123456789abcdef0123456789abcdef", &nodes)
            .unwrap()
            .node_id
            .clone();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn no_owner_is_bounded_and_reported_without_blocking_producer() {
        let pipeline = local_pipeline();
        let router = TraceCandidateRouter::start(
            Arc::new(FixedRegistry(Vec::new())),
            "router".into(),
            "router:5081".into(),
            false,
            None,
            pipeline.clone(),
            TraceCandidateRouterConfig {
                queue_capacity: 4,
                workers: 1,
                max_attempts: 2,
                attempt_timeout: Duration::from_millis(10),
                max_delivery_age: Duration::from_millis(100),
                initial_backoff: Duration::from_millis(1),
                shutdown_timeout: Duration::from_secs(1),
            },
            TraceLimits::default(),
        )
        .unwrap();
        router
            .try_submit(TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span: crate::shared::trace_fixtures::canonical_http_trace().remove(0),
                force_keep: ForceKeep::None,
            })
            .unwrap();
        timeout(Duration::from_secs(1), async {
            loop {
                if router.health().no_owner == 1 {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("no-owner outcome");
        assert_eq!(router.health().delivered_local, 0);
        assert_eq!(router.health().delivered_remote, 0);
        router.shutdown().await;
        pipeline.shutdown().await;
    }

    #[tokio::test]
    async fn split_role_candidates_reach_one_remote_owner_and_both_sinks() {
        let self_ingest = Arc::new(MemoryTraceSink::default());
        let external = Arc::new(MemoryTraceSink::default());
        let sampler = Arc::new(
            crate::shared::tail_sampling::TailSampler::new(
                crate::shared::tail_sampling::TraceRuntimePolicy {
                    normal_sample_ratio: 1.0,
                    root_grace_ms: 1,
                    ..crate::shared::tail_sampling::TraceRuntimePolicy::default()
                },
                false,
                TraceLimits::default(),
            )
            .unwrap(),
        );
        let owner_pipeline = TracePipeline::start(
            sampler,
            Some(self_ingest.clone()),
            Some(external.clone()),
            fast_pipeline_config(),
            TraceLimits::default(),
        )
        .unwrap();
        let token: Arc<str> = Arc::from("split-role-cluster-secret");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tail-owner gRPC");
        let owner_addr = listener.local_addr().expect("tail-owner address");
        let owner_server = tokio::spawn(
            Server::builder()
                .add_service(
                    TraceCandidateGrpc::new(owner_pipeline.clone(), Some(token.clone()))
                        .into_server(),
                )
                .serve_with_incoming(TcpListenerStream::new(listener)),
        );
        let registry: Arc<dyn ClusterRegistry> = Arc::new(FixedRegistry(vec![PeerInfo {
            node_id: "tail-owner-ingester".into(),
            advertise_addr: owner_addr.to_string(),
            roles: vec![PeerRole::Ingester],
        }]));
        let router = remote_router(
            registry.clone(),
            "router-producer",
            token.clone(),
            owner_pipeline.clone(),
        );
        let querier = remote_router(
            registry.clone(),
            "querier-producer",
            token.clone(),
            owner_pipeline.clone(),
        );
        let ingester = remote_router(registry, "ingester-producer", token, owner_pipeline.clone());

        let trace_id = "0123456789abcdef0123456789abcdef";
        let candidates = [
            (
                &ingester,
                role_span(
                    trace_id,
                    "0000000000000005",
                    Some("0000000000000004"),
                    "object_store.operation",
                    "ingester",
                    "ingester-a",
                ),
            ),
            (
                &ingester,
                role_span(
                    trace_id,
                    "0000000000000004",
                    Some("0000000000000003"),
                    "ingest.batch",
                    "ingester",
                    "ingester-a",
                ),
            ),
            (
                &querier,
                role_span(
                    trace_id,
                    "0000000000000003",
                    Some("0000000000000002"),
                    "rpc.client.flight",
                    "querier",
                    "querier-a",
                ),
            ),
            (
                &querier,
                role_span(
                    trace_id,
                    "0000000000000002",
                    Some("0000000000000001"),
                    "query.federation",
                    "querier",
                    "querier-a",
                ),
            ),
        ];
        for (producer, span) in candidates {
            producer
                .try_submit(TraceCandidate {
                    org_id: "system-org".into(),
                    stream: None,
                    span,
                    force_keep: ForceKeep::TrustedInternal,
                })
                .expect("non-blocking producer enqueue");
        }
        timeout(Duration::from_secs(3), async {
            loop {
                if querier.health().delivered_remote == 2 && ingester.health().delivered_remote == 2
                {
                    break;
                }
                sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("child spans reach remote owner before root");
        router
            .try_submit(TraceCandidate {
                org_id: "system-org".into(),
                stream: None,
                span: role_span(
                    trace_id,
                    "0000000000000001",
                    None,
                    "http.server",
                    "router",
                    "router-a",
                ),
                force_keep: ForceKeep::TrustedInternal,
            })
            .expect("root producer enqueue");
        timeout(Duration::from_secs(3), async {
            loop {
                if router.health().delivered_remote == 1 {
                    break;
                }
                sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("root reaches remote owner");

        router.shutdown().await;
        querier.shutdown().await;
        ingester.shutdown().await;
        owner_pipeline.shutdown().await;
        owner_server.abort();

        let internal = self_ingest.traces("system-org").await;
        let exported = external.traces("system-org").await;
        assert_eq!(internal.len(), 1);
        assert_eq!(exported.len(), 1);
        assert_eq!(internal[0].reason, exported[0].reason);
        assert_eq!(internal[0].spans.len(), 5);
        assert_eq!(internal[0].spans, exported[0].spans);
        let graph = Arc::new(ServiceGraphAggregator::new());
        let observer = ServiceGraphObserverImpl::new(graph.clone());
        let graph_events = internal[0]
            .spans
            .iter()
            .cloned()
            .map(CanonicalSpan::into_raw_event)
            .collect::<Vec<_>>();
        observer.observe(&crate::shared::ids::Id("system-org".into()), &graph_events);
        let edges = graph.flush_due(i64::MAX);
        assert!(edges.iter().any(|edge| {
            edge.client_service == "molesignal-router"
                && edge.server_service == "molesignal-querier"
        }));
        assert!(edges.iter().any(|edge| {
            edge.client_service == "molesignal-querier"
                && edge.server_service == "molesignal-ingester"
        }));
        assert!(edges.iter().all(|edge| {
            !edge.client_service.contains('+') && !edge.server_service.contains('+')
        }));
        let identities = internal[0]
            .spans
            .iter()
            .map(|span| {
                (
                    span.attributes["molesignal.execution.role"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    span.resource.attributes["node.id"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                )
            })
            .collect::<HashSet<_>>();
        assert!(identities.contains(&("router".into(), "router-a".into())));
        assert!(identities.contains(&("querier".into(), "querier-a".into())));
        assert!(identities.contains(&("ingester".into(), "ingester-a".into())));
    }

    #[tokio::test]
    async fn failed_owner_is_excluded_and_candidate_rehashes_to_live_owner() {
        let sink = Arc::new(MemoryTraceSink::default());
        let sampler = Arc::new(
            crate::shared::tail_sampling::TailSampler::new(
                crate::shared::tail_sampling::TraceRuntimePolicy {
                    normal_sample_ratio: 1.0,
                    root_grace_ms: 1,
                    ..crate::shared::tail_sampling::TraceRuntimePolicy::default()
                },
                false,
                TraceLimits::default(),
            )
            .unwrap(),
        );
        let pipeline = TracePipeline::start(
            sampler,
            Some(sink.clone()),
            None,
            fast_pipeline_config(),
            TraceLimits::default(),
        )
        .unwrap();
        let token: Arc<str> = Arc::from("owner-churn-cluster-secret");
        let live_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind live owner");
        let live_addr = live_listener.local_addr().expect("live owner address");
        let live_server = tokio::spawn(
            Server::builder()
                .add_service(
                    TraceCandidateGrpc::new(pipeline.clone(), Some(token.clone())).into_server(),
                )
                .serve_with_incoming(TcpListenerStream::new(live_listener)),
        );
        let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve failed-owner address");
        let dead_addr = dead_listener.local_addr().expect("failed-owner address");
        drop(dead_listener);
        let peers = vec![
            PeerInfo {
                node_id: "dead-owner".into(),
                advertise_addr: dead_addr.to_string(),
                roles: vec![PeerRole::Querier],
            },
            PeerInfo {
                node_id: "live-owner".into(),
                advertise_addr: live_addr.to_string(),
                roles: vec![PeerRole::Ingester],
            },
        ];
        let nodes = peers.iter().map(peer_to_sampler_node).collect::<Vec<_>>();
        let trace_id = (1_u128..10_000)
            .map(|value| format!("{value:032x}"))
            .find(|trace_id| {
                rendezvous_owner(trace_id, &nodes)
                    .is_some_and(|owner| owner.node_id == "dead-owner")
            })
            .expect("trace ID owned by failed node");
        let router = remote_router(
            Arc::new(FixedRegistry(peers)),
            "router-producer",
            token,
            pipeline.clone(),
        );
        router
            .try_submit(TraceCandidate {
                org_id: "system-org".into(),
                stream: None,
                span: role_span(
                    &trace_id,
                    "0000000000000001",
                    None,
                    "http.server",
                    "router",
                    "router-a",
                ),
                force_keep: ForceKeep::TrustedInternal,
            })
            .unwrap();
        timeout(Duration::from_secs(3), async {
            loop {
                if router.health().delivered_remote == 1 {
                    break;
                }
                sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("candidate rehashed to live owner");

        router.shutdown().await;
        pipeline.shutdown().await;
        live_server.abort();
        let retained = sink.traces("system-org").await;
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].trace_id, trace_id);
    }
}
