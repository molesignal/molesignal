// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a producer hands the router: one candidate at a time for the process's own spans, or a
//! whole OTLP request at once, taken or refused as a unit.
//!
//! A request is taken in three steps: room for all its spans is reserved in the queue, the
//! tenant spans are written to the candidate log when one is attached, and only then do the
//! spans enter the queue. A refusal at any step leaves nothing behind but the reservation, which
//! is given back.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceCandidateSubmitError {
    Full,
    Stopped,
    Invalid,
    /// The batch alone holds more spans than the queue does: it can never be taken whole.
    TooLarge,
    /// The candidate log could not store the spans; nothing was taken.
    Unlogged(String),
}

/// Room reserved in the queue for the valid spans of one request.
struct Reserved<'a> {
    permits: Option<mpsc::PermitIterator<'a, QueuedCandidate>>,
    valid: Vec<TraceCandidate>,
    invalid: usize,
}

/// The outcome of a batch the router took: how many spans went into the queue, and how many were
/// refused for good because sanitising left them over the size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceBatchAdmission {
    pub accepted: usize,
    pub invalid: usize,
}

impl TraceCandidateRouter {
    /// producer 热路径：先执行中央 sanitizer/limits，再尝试写入本地有界队列。
    pub fn try_submit(
        &self,
        mut candidate: TraceCandidate,
    ) -> Result<(), TraceCandidateSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            trace_metrics::record_spans("routing", "stopped", 1);
            return Err(TraceCandidateSubmitError::Stopped);
        }

        self.admit(&mut candidate)?;
        self.depth.fetch_add(1, Ordering::Relaxed);
        trace_metrics::set_queue("routing", self.depth.load(Ordering::Relaxed), self.capacity);

        match self.sender.try_send(QueuedCandidate {
            candidate,
            enqueued_at: Instant::now(),
            origin: None,
        }) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.release(1);
                self.metrics.queue_full.fetch_add(1, Ordering::Relaxed);
                trace_metrics::record_spans("routing", "queue_full", 1);
                Err(TraceCandidateSubmitError::Full)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.release(1);
                trace_metrics::record_spans("routing", "stopped", 1);
                Err(TraceCandidateSubmitError::Stopped)
            }
        }
    }

    /// Queues every valid span of one request, or none of them: room for the whole batch is
    /// reserved before the first span goes in, so a sender told "full" knows nothing was taken
    /// and can retry the request as it is. Spans refused for size are counted, not queued.
    ///
    /// Nothing is written to the candidate log: for spans that need no durability.
    pub fn try_submit_batch(
        &self,
        candidates: Vec<TraceCandidate>,
    ) -> Result<TraceBatchAdmission, TraceCandidateSubmitError> {
        let reserved = self.reserve(candidates)?;
        let origins = vec![None; reserved.valid.len()];
        Ok(self.send(reserved, origins))
    }

    /// [`Self::try_submit_batch`] that, with a candidate log attached, answers only once the
    /// request's tenant spans are stored in it. The OTLP intake's way in.
    pub async fn submit_durable(
        &self,
        candidates: Vec<TraceCandidate>,
    ) -> Result<TraceBatchAdmission, TraceCandidateSubmitError> {
        let reserved = self.reserve(candidates)?;

        let origins = match self.candidate_log.get() {
            Some(log) => log
                .append_candidates(&reserved.valid)
                .await
                .map_err(|error| TraceCandidateSubmitError::Unlogged(error.to_string()))?,
            None => vec![None; reserved.valid.len()],
        };

        Ok(self.send(reserved, origins))
    }

    fn reserve(
        &self,
        candidates: Vec<TraceCandidate>,
    ) -> Result<Reserved<'_>, TraceCandidateSubmitError> {
        if !self.accepting.load(Ordering::Acquire) {
            trace_metrics::record_spans("routing", "stopped", candidates.len() as u64);
            return Err(TraceCandidateSubmitError::Stopped);
        }

        let total = candidates.len();
        let mut valid = Vec::with_capacity(total);

        for mut candidate in candidates {
            if self.admit(&mut candidate).is_ok() {
                valid.push(candidate);
            }
        }

        let invalid = total - valid.len();

        if valid.is_empty() {
            return Ok(Reserved {
                permits: None,
                valid,
                invalid,
            });
        }

        if valid.len() > self.capacity {
            return Err(TraceCandidateSubmitError::TooLarge);
        }

        let permits = match self.sender.try_reserve_many(valid.len()) {
            Ok(permits) => permits,
            Err(mpsc::error::TrySendError::Full(())) => {
                self.metrics
                    .queue_full
                    .fetch_add(valid.len() as u64, Ordering::Relaxed);

                trace_metrics::record_spans("routing", "queue_full", valid.len() as u64);
                return Err(TraceCandidateSubmitError::Full);
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                trace_metrics::record_spans("routing", "stopped", valid.len() as u64);
                return Err(TraceCandidateSubmitError::Stopped);
            }
        };

        Ok(Reserved {
            permits: Some(permits),
            valid,
            invalid,
        })
    }

    fn send(
        &self,
        reserved: Reserved<'_>,
        origins: Vec<Option<SpanOrigin>>,
    ) -> TraceBatchAdmission {
        let Reserved {
            permits,
            valid,
            invalid,
        } = reserved;

        let Some(permits) = permits else {
            return TraceBatchAdmission {
                accepted: 0,
                invalid,
            };
        };

        let accepted = valid.len();
        self.depth.fetch_add(accepted, Ordering::Relaxed);
        trace_metrics::set_queue("routing", self.depth.load(Ordering::Relaxed), self.capacity);
        let enqueued_at = Instant::now();

        for ((permit, candidate), origin) in permits.zip(valid).zip(origins) {
            permit.send(QueuedCandidate {
                candidate,
                enqueued_at,
                origin,
            });
        }

        TraceBatchAdmission { accepted, invalid }
    }

    /// Sanitises one candidate and refuses it when it is still over the size limit.
    fn admit(&self, candidate: &mut TraceCandidate) -> Result<(), TraceCandidateSubmitError> {
        sanitize_and_limit_span(&mut candidate.span, self.limits);

        let encoded_len = serde_json::to_vec(&candidate.span)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);

        if encoded_len > MAX_TRACE_CANDIDATE_BYTES {
            self.metrics.invalid.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "invalid", 1);
            return Err(TraceCandidateSubmitError::Invalid);
        }

        Ok(())
    }

    /// Gives back queue depth counted for spans that did not go in after all.
    fn release(&self, count: usize) {
        self.depth.fetch_sub(count, Ordering::Relaxed);
        trace_metrics::set_queue("routing", self.depth.load(Ordering::Relaxed), self.capacity);
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    struct NoPeers;

    #[async_trait]
    impl ClusterRegistry for NoPeers {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            Vec::new()
        }
    }

    /// A router whose one worker holds the first candidate it takes for a long backoff, so the
    /// queue keeps what the test puts in.
    fn stalled_router(queue_capacity: usize) -> Arc<TraceCandidateRouter> {
        let sampler = Arc::new(
            signals::tail_sampling::TailSampler::new(
                signals::tail_sampling::TraceRuntimePolicy::default(),
                false,
                TraceLimits::default(),
            )
            .unwrap(),
        );

        let pipeline = TracePipeline::start(
            sampler,
            None,
            None,
            crate::app::trace::TracePipelineConfig::default(),
            TraceLimits::default(),
        )
        .unwrap();

        TraceCandidateRouter::start(
            Arc::new(NoPeers),
            "router".into(),
            "router:5081".into(),
            false,
            None,
            pipeline,
            TraceCandidateRouterConfig {
                queue_capacity,
                workers: 1,
                max_attempts: 2,
                attempt_timeout: Duration::from_millis(10),
                max_delivery_age: Duration::from_secs(120),
                initial_backoff: Duration::from_secs(60),
                shutdown_timeout: Duration::from_millis(50),
            },
            TraceLimits::default(),
        )
        .unwrap()
    }

    fn candidates(count: usize) -> Vec<TraceCandidate> {
        let span = signals::trace_fixtures::canonical_http_trace().remove(0);

        (0..count)
            .map(|_| TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span: span.clone(),
                force_keep: ForceKeep::None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_batch_goes_in_whole_or_not_at_all() {
        let router = stalled_router(4);

        let admission = router.try_submit_batch(candidates(4)).unwrap();
        assert_eq!(
            admission,
            TraceBatchAdmission {
                accepted: 4,
                invalid: 0
            }
        );

        // At most one candidate left the queue for the stalled worker: two more do not fit.
        let refused = router.try_submit_batch(candidates(2));
        assert_eq!(refused, Err(TraceCandidateSubmitError::Full));
        assert_eq!(router.health().queue_full, 2);
        assert!(
            router.health().queue_depth <= 4,
            "nothing of the refused batch went in"
        );

        router.shutdown().await;
    }

    #[tokio::test]
    async fn a_batch_larger_than_the_queue_is_refused_as_too_large() {
        let router = stalled_router(4);

        assert_eq!(
            router.try_submit_batch(candidates(5)),
            Err(TraceCandidateSubmitError::TooLarge)
        );
        assert_eq!(router.health().queue_depth, 0);

        router.shutdown().await;
    }
}
