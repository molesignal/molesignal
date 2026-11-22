// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Replaying the candidate log of a writer that is gone.
//!
//! Each round claims the candidate logs whose writer is fenced (crashed, lost its disk, was
//! partitioned away, or drained with spans unsettled), reads every object, and hands the spans
//! with no settled marker to this node's router, which writes them into this node's own log
//! before routing them. Each chunk handed over is recorded as settled in the old log, so a round
//! that fails half-way resumes without handing a span over twice. Then the old objects are
//! deleted and the row closed.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use domain::storage::OrphanedStreams;
use futures::TryStreamExt;
use kernel::{Error, Result};
use object_store::{ObjectStoreExt, PutPayload, path::Path};
use signals::tail_sampling::TraceCandidate;
use tokio::task::JoinHandle;

use super::{
    TraceCandidateLog, candidate_log_type, epoch_prefix, object_path, record::LogRecord,
    sequence_of,
};
use crate::{
    app::trace::candidate_router::{TraceCandidateRouter, TraceCandidateSubmitError},
    infra::intake::object_wal::{decode_object, delete_objects, encode_object},
};

/// Rows looked at in one round.
const ROWS_PER_ROUND: u32 = 256;
/// Spans handed to the router at once.
const CHUNK_SPANS: usize = 512;

/// What one round did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TraceTakeoverRound {
    /// Candidate logs replayed and closed.
    pub logs: usize,
    /// Spans handed over again.
    pub spans: usize,
}

/// Claims and replays the candidate logs of fenced writers.
pub async fn takeover_round(
    log: &Arc<TraceCandidateLog>,
    router: &TraceCandidateRouter,
) -> Result<TraceTakeoverRound> {
    let wal = log.wal();
    let lease = wal.lease();
    let mut round = TraceTakeoverRound::default();

    if !lease.is_valid() {
        return Ok(round);
    }

    let rows = wal
        .registry()
        .orphaned_streams(
            lease.epoch(),
            &OrphanedStreams::Only(candidate_log_type()),
            ROWS_PER_ROUND,
        )
        .await?;

    for row in rows {
        if row.writer_epoch == lease.epoch() {
            continue;
        }

        if !wal.registry().claim_stream(&row, lease.epoch()).await? {
            continue;
        }

        match replay(log, router, row.writer_epoch.0).await {
            Ok(spans) => {
                wal.registry()
                    .close_stream(
                        &row.organization_id,
                        &row.dataset_id,
                        row.writer_epoch,
                        lease.epoch(),
                    )
                    .await?;

                tracing::info!(
                    writer_epoch = %row.writer_epoch,
                    writer_node_id = %row.writer_node_id,
                    spans,
                    "trace candidate log of a gone writer replayed"
                );

                round.logs += 1;
                round.spans += spans;
            }
            Err(error) => tracing::warn!(
                writer_epoch = %row.writer_epoch,
                %error,
                "replaying a trace candidate log failed; it resumes next round"
            ),
        }
    }

    Ok(round)
}

/// Runs a round every `interval`.
pub fn spawn_takeover_loop(
    log: Arc<TraceCandidateLog>,
    router: Arc<TraceCandidateRouter>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval.max(Duration::from_millis(100)));
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if let Err(error) = takeover_round(&log, &router).await {
                tracing::warn!(%error, "trace candidate log takeover round failed");
            }
        }
    })
}

/// Hands every unsettled span of `epoch`'s log to `router`, then deletes the log's objects.
async fn replay(
    log: &Arc<TraceCandidateLog>,
    router: &TraceCandidateRouter,
    epoch: u64,
) -> Result<usize> {
    let store = log.wal().store().clone();

    let listed: Vec<_> = store
        .list(Some(&epoch_prefix(epoch)))
        .try_collect()
        .await
        .map_err(|error| Error::unavailable(format!("list trace candidate log: {error}")))?;

    let objects: Vec<(u64, Path)> = listed
        .into_iter()
        .filter_map(|meta| sequence_of(&meta.location).map(|sequence| (sequence, meta.location)))
        .collect();

    let mut candidates: BTreeMap<(u64, u32, u32), TraceCandidate> = BTreeMap::new();
    let mut settled: HashSet<(u64, u32, u32)> = HashSet::new();

    for (sequence, path) in &objects {
        let bytes = store
            .get(path)
            .await
            .map_err(|error| Error::unavailable(format!("read {path}: {error}")))?
            .bytes()
            .await
            .map_err(|error| Error::unavailable(format!("read {path}: {error}")))?;

        let records = decode_object(&bytes, log.cipher())
            .map_err(|error| Error::internal(format!("trace candidate object {path}: {error}")))?;

        for (index, raw) in records.iter().enumerate() {
            let record = LogRecord::decode(raw)
                .map_err(|error| Error::internal(format!("record in {path}: {error}")))?;

            let index = u32::try_from(index).unwrap_or(u32::MAX);

            match &record {
                LogRecord::Candidates { .. } => {
                    for (span, candidate) in record.candidates().into_iter().enumerate() {
                        let span = u32::try_from(span).unwrap_or(u32::MAX);
                        candidates.insert((*sequence, index, span), candidate);
                    }
                }
                LogRecord::Settled { spans } => settled.extend(spans.iter().copied()),
            }
        }
    }

    candidates.retain(|key, _| !settled.contains(key));
    let next_sequence = objects
        .iter()
        .map(|(sequence, _)| *sequence)
        .max()
        .unwrap_or(0)
        + 1;
    let total = candidates.len();
    let pending: Vec<_> = candidates.into_iter().collect();

    let mut doomed = objects;

    for (sequence, chunk) in (next_sequence..).zip(pending.chunks(CHUNK_SPANS)) {
        let spans = chunk
            .iter()
            .map(|(_, candidate)| candidate.clone())
            .collect();
        hand_over(router, spans).await?;

        let keys = chunk.iter().map(|(key, _)| *key).collect();
        let path = record_handed_over(log, epoch, sequence, keys).await?;
        doomed.push((sequence, path));
    }

    let failed = delete_objects(&store, doomed).await;

    if !failed.is_empty() {
        return Err(Error::unavailable(format!(
            "{} trace candidate log objects could not be deleted",
            failed.len()
        )));
    }

    Ok(total)
}

/// Gives spans to the router until it takes them: it writes them into this node's log first.
async fn hand_over(router: &TraceCandidateRouter, spans: Vec<TraceCandidate>) -> Result<()> {
    let mut wait = Duration::from_millis(50);

    loop {
        match router.submit_durable(spans.clone()).await {
            Ok(_) => return Ok(()),
            Err(TraceCandidateSubmitError::Full | TraceCandidateSubmitError::Unlogged(_)) => {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(2));
            }
            Err(error) => {
                return Err(Error::unavailable(format!(
                    "the router refused replayed spans: {error:?}"
                )));
            }
        }
    }
}

/// Records in the old log that the spans at `keys` were handed over.
async fn record_handed_over(
    log: &Arc<TraceCandidateLog>,
    epoch: u64,
    sequence: u64,
    keys: Vec<(u64, u32, u32)>,
) -> Result<Path> {
    let record = LogRecord::Settled { spans: keys }
        .encode()
        .and_then(|record| encode_object([record.as_slice()], log.cipher()))
        .map_err(|error| Error::internal(format!("encode replay marker: {error}")))?;

    let path = object_path(epoch, sequence);

    log.wal()
        .store()
        .put(&path, PutPayload::from(record))
        .await
        .map_err(|error| Error::unavailable(format!("store replay marker: {error}")))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use domain::{
        cluster::{ClusterRegistry, PeerInfo, PeerRole},
        storage::WalWriterRegistry,
    };
    use futures::TryStreamExt;
    use kernel::ids::Id;
    use object_store::{ObjectStore, memory::InMemory};
    use signals::{
        tail_sampling::{ForceKeep, TailSampler, TraceRuntimePolicy},
        trace_normalization::TraceLimits,
    };

    use super::*;
    use crate::{
        app::trace::{
            MemoryTraceSink, TracePipeline, TracePipelineConfig,
            candidate_router::TraceCandidateRouterConfig,
            durable::{SpanSettlement, TraceLogConfig},
        },
        infra::intake::object_wal::{MemoryWalWriters, ObjectWal, test_support::config},
    };

    struct NoPeers;

    #[async_trait]
    impl ClusterRegistry for NoPeers {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            Vec::new()
        }
    }

    async fn node(
        store: &Arc<InMemory>,
        registry: &Arc<MemoryWalWriters>,
        name: &str,
    ) -> Arc<TraceCandidateLog> {
        let wal = ObjectWal::start(
            store.clone(),
            registry.clone(),
            name,
            config(Duration::from_secs(30)),
            None,
        )
        .await
        .unwrap();

        Arc::new(TraceCandidateLog::new(
            Arc::new(wal),
            Id("sys".into()),
            TraceLogConfig::default(),
        ))
    }

    /// A node that samples everything quickly and stores into memory.
    fn pipeline_and_router(
        log: &Arc<TraceCandidateLog>,
    ) -> (
        Arc<TracePipeline>,
        Arc<TraceCandidateRouter>,
        Arc<MemoryTraceSink>,
    ) {
        let policy = TraceRuntimePolicy {
            normal_sample_ratio: 1.0,
            decision_window_ms: 5_000,
            root_grace_ms: 1,
            decision_cache_ms: 10_000,
            ..TraceRuntimePolicy::default()
        };

        let sampler = Arc::new(TailSampler::new(policy, false, TraceLimits::default()).unwrap());
        let sink = Arc::new(MemoryTraceSink::default());

        let defaults = TracePipelineConfig::default();

        let config = TracePipelineConfig {
            decision_tick: Duration::from_millis(5),
            self_intake: crate::app::trace::TraceSinkWorkerConfig {
                batch_delay: Duration::from_millis(5),
                ..defaults.self_intake
            },
            ..defaults
        };

        let pipeline = TracePipeline::start(
            sampler,
            Some(sink.clone()),
            None,
            config,
            TraceLimits::default(),
        )
        .unwrap();

        let router = TraceCandidateRouter::start(
            Arc::new(NoPeers),
            "node".into(),
            "node:5081".into(),
            true,
            None,
            pipeline.clone(),
            TraceCandidateRouterConfig::default(),
            TraceLimits::default(),
        )
        .unwrap();

        router.attach_candidate_log(log.clone());
        pipeline.attach_settlement(log.clone());
        (pipeline, router, sink)
    }

    async fn objects(store: &InMemory, epoch: u64) -> usize {
        store
            .list(Some(&epoch_prefix(epoch)))
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len()
    }

    async fn stored_spans(sink: &MemoryTraceSink) -> Vec<String> {
        let mut spans: Vec<String> = sink
            .traces("org")
            .await
            .iter()
            .flat_map(|trace| trace.spans.iter().map(|span| span.span_id.clone()))
            .collect();

        spans.sort();
        spans
    }

    #[tokio::test]
    async fn spans_acknowledged_by_a_node_that_died_are_stored_once_by_another() {
        let store = Arc::new(InMemory::new());
        let registry = Arc::new(MemoryWalWriters::default());

        // Node A acknowledges a trace's spans, stores one of them (a child: the root stays, so the
        // replayed trace is decided at once), and dies with its disk and memory before writing
        // the rest.
        let a = node(&store, &registry, "node-a").await;
        let epoch_a = a.wal().lease().epoch();
        let spans = signals::trace_fixtures::canonical_http_trace();
        assert!(spans.len() >= 2);

        let origins = a
            .append("org", "default", ForceKeep::None, &spans)
            .await
            .unwrap();

        let stored = spans.len() - 1;
        assert!(
            spans[stored].parent_span_id.is_some(),
            "the last fixture span is a child"
        );
        a.settle(&origins[stored..]);
        a.write_markers().await.unwrap();
        drop(a);

        registry.expire(epoch_a);
        registry.fence_expired().await.unwrap();

        // Node B claims A's log and replays what was not settled.
        let b = node(&store, &registry, "node-b").await;
        let (pipeline, router, sink) = pipeline_and_router(&b);

        let round = takeover_round(&b, &router).await.unwrap();
        assert_eq!(round.logs, 1);
        assert_eq!(round.spans, spans.len() - 1);

        let mut expected: Vec<String> = spans[..stored]
            .iter()
            .map(|span| span.span_id.clone())
            .collect();
        expected.sort();

        tokio::time::timeout(Duration::from_secs(3), async {
            while stored_spans(&sink).await.len() < expected.len() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("replayed spans stored");

        assert_eq!(
            stored_spans(&sink).await,
            expected,
            "each unsettled span once, none other"
        );
        assert_eq!(
            objects(&store, epoch_a.0).await,
            0,
            "the dead node's log is gone"
        );
        assert!(
            !registry
                .streams()
                .iter()
                .any(|row| row.writer_epoch == epoch_a),
            "its row is closed"
        );

        // A second round finds nothing, and B's own log empties once the spans are stored.
        assert_eq!(
            takeover_round(&b, &router).await.unwrap(),
            TraceTakeoverRound::default()
        );

        tokio::time::timeout(Duration::from_secs(3), async {
            while b.unsettled() > 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("B's replayed spans settle");

        router.shutdown().await;
        pipeline.shutdown().await;
        assert_eq!(stored_spans(&sink).await, expected);
    }

    #[tokio::test]
    async fn a_replay_that_fails_half_way_does_not_hand_a_span_over_twice() {
        let store = Arc::new(InMemory::new());
        let registry = Arc::new(MemoryWalWriters::default());

        let a = node(&store, &registry, "node-a").await;
        let epoch_a = a.wal().lease().epoch();
        let spans = signals::trace_fixtures::canonical_http_trace();
        let origins = a
            .append("org", "default", ForceKeep::None, &spans)
            .await
            .unwrap();

        drop(origins);
        drop(a);
        registry.expire(epoch_a);
        registry.fence_expired().await.unwrap();

        // B hands every span over and records it, then dies before deleting A's objects.
        let b = node(&store, &registry, "node-b").await;
        let keys: Vec<(u64, u32, u32)> = (0..spans.len())
            .map(|span| (1, 0, u32::try_from(span).unwrap()))
            .collect();

        record_handed_over(&b, epoch_a.0, 2, keys).await.unwrap();

        // Whoever resumes the replay finds nothing left to hand over.
        let (pipeline, router, sink) = pipeline_and_router(&b);
        let round = takeover_round(&b, &router).await.unwrap();

        assert_eq!(round.logs, 1);
        assert_eq!(round.spans, 0);
        assert_eq!(objects(&store, epoch_a.0).await, 0);

        router.shutdown().await;
        pipeline.shutdown().await;
        assert!(stored_spans(&sink).await.is_empty());
    }
    /// A sink that never finishes a write: the owner holds the spans and dies with them.
    struct StuckSink;

    #[async_trait]
    impl crate::app::trace::TraceSink for StuckSink {
        fn name(&self) -> &'static str {
            "self_intake"
        }

        async fn export(
            &self,
            _traces: &mut Vec<signals::tail_sampling::DecidedTrace>,
        ) -> std::result::Result<(), String> {
            std::future::pending().await
        }
    }

    struct OnePeer(PeerInfo);

    #[async_trait]
    impl ClusterRegistry for OnePeer {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            vec![self.0.clone()]
        }
    }

    #[tokio::test]
    async fn spans_held_by_a_remote_owner_that_died_are_replayed_from_its_log() {
        use tokio_stream::wrappers::TcpListenerStream;
        use tonic::transport::Server;

        use crate::api::grpc::trace::candidate_server::TraceCandidateGrpc;

        let store = Arc::new(InMemory::new());
        let registry = Arc::new(MemoryWalWriters::default());
        let token: Arc<str> = Arc::from("cluster-secret");

        // The owner logs what it accepts, then never manages to store it.
        let owner_log = node(&store, &registry, "owner").await;
        let owner_epoch = owner_log.wal().lease().epoch();
        let owner_sampler = Arc::new(
            TailSampler::new(TraceRuntimePolicy::default(), false, TraceLimits::default()).unwrap(),
        );
        let owner_pipeline = TracePipeline::start(
            owner_sampler,
            Some(Arc::new(StuckSink)),
            None,
            TracePipelineConfig::default(),
            TraceLimits::default(),
        )
        .unwrap();
        owner_pipeline.attach_settlement(owner_log.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let owner_addr = listener.local_addr().unwrap();
        let server = tokio::spawn(
            Server::builder()
                .add_service(
                    TraceCandidateGrpc::new(owner_pipeline.clone(), Some(token.clone()))
                        .with_candidate_log(Some(owner_log.clone()))
                        .into_server(),
                )
                .serve_with_incoming(TcpListenerStream::new(listener)),
        );

        // The producer acknowledges spans and hands them to the remote owner.
        let producer_log = node(&store, &registry, "producer").await;
        let producer_epoch = producer_log.wal().lease().epoch();
        let (producer_pipeline, _, _) = pipeline_and_router(&producer_log);
        let producer = TraceCandidateRouter::start(
            Arc::new(OnePeer(PeerInfo {
                node_id: "owner".into(),
                advertise_addr: owner_addr.to_string(),
                roles: vec![PeerRole::Intake],
            })),
            "producer".into(),
            "producer:5081".into(),
            false,
            Some(token),
            producer_pipeline.clone(),
            TraceCandidateRouterConfig::default(),
            TraceLimits::default(),
        )
        .unwrap();
        producer.attach_candidate_log(producer_log.clone());

        let spans = signals::trace_fixtures::canonical_http_trace();
        let candidates = spans
            .iter()
            .map(|span| TraceCandidate {
                org_id: "org".into(),
                stream: Some("default".into()),
                span: span.clone(),
                force_keep: ForceKeep::None,
            })
            .collect();

        producer.submit_durable(candidates).await.unwrap();

        // The owner's acceptance moved the spans from the producer's log into the owner's.
        tokio::time::timeout(Duration::from_secs(5), async {
            while producer_log.unsettled() > 0 || objects(&store, producer_epoch.0).await > 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the producer let go once the owner accepted");

        assert_eq!(owner_log.unsettled(), spans.len() as u64);
        assert_eq!(producer.health().delivered_remote, spans.len() as u64);

        // The owner dies with the spans in memory. Another node replays its log.
        server.abort();
        registry.expire(owner_epoch);
        registry.fence_expired().await.unwrap();

        let heir = node(&store, &registry, "heir").await;
        let (heir_pipeline, heir_router, sink) = pipeline_and_router(&heir);
        let round = takeover_round(&heir, &heir_router).await.unwrap();
        assert_eq!(round.spans, spans.len());

        let mut expected: Vec<String> = spans.iter().map(|span| span.span_id.clone()).collect();
        expected.sort();

        tokio::time::timeout(Duration::from_secs(3), async {
            while stored_spans(&sink).await.len() < expected.len() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the owner's spans stored by the heir");

        assert_eq!(stored_spans(&sink).await, expected);

        heir_router.shutdown().await;
        heir_pipeline.shutdown().await;
        producer.shutdown().await;
        producer_pipeline.shutdown().await;
    }
}
