// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Delivering a worker's batch of queued candidates to their sampler owners.
//!
//! A tenant's span was acknowledged to its sender before it reached this queue, so it is
//! delivered or the process stops trying: candidates are grouped by owner, a local owner is
//! waited on, and each remote owner gets one call per batch, retried with a capped backoff while
//! it is overloaded, unreachable or missing. While a worker waits the queue fills, and the intake
//! answers the next request with a retryable refusal. The process's own spans keep their bounded
//! attempts and are dropped and counted after them.
//!
//! A logged span carries its place in the candidate log. A local owner takes the place along and
//! settles it when the span's fate is durable. A remote owner accepts a batch only once it has
//! written the tenant spans to its own candidate log, so its acceptance settles them here.

use std::collections::HashMap;

use protocol::cluster::v1::TraceCandidateItem;

use super::*;

/// The longest wait between two attempts at tenant candidates.
const MAX_TENANT_BACKOFF: Duration = Duration::from_secs(5);
/// Candidates one call to a remote owner carries at most.
const MAX_CALL_CANDIDATES: usize = 128;
/// Encoded span bytes one call carries at most (one candidate is never split).
const MAX_CALL_BYTES: usize = 4 * 1024 * 1024;

/// A queued candidate with its span encoded for the wire.
struct Outgoing {
    queued: QueuedCandidate,
    json: Vec<u8>,
}

/// What a remote owner made of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remote {
    Accepted,
    Overloaded,
    Failed,
}

pub(super) async fn deliver_batch(context: &RouterWorkerContext, batch: Vec<QueuedCandidate>) {
    let mut tenant = Vec::new();

    for queued in batch {
        let json = match serde_json::to_vec(&queued.candidate.span) {
            Ok(value) if value.len() <= MAX_TRACE_CANDIDATE_BYTES => value,
            _ => {
                context.metrics.invalid.fetch_add(1, Ordering::Relaxed);
                trace_metrics::record_spans("routing", "invalid", 1);
                // Refused for good: nothing is left to replay.
                settle(context, &[queued.origin]);
                continue;
            }
        };

        let outgoing = Outgoing { queued, json };

        if outgoing.queued.candidate.stream.is_some() {
            tenant.push(outgoing);
        } else {
            deliver_best_effort(context, outgoing).await;
        }
    }

    if !tenant.is_empty() {
        deliver_until_taken(context, tenant).await;
    }
}

/// Tenant candidates: retried until their owners take them.
async fn deliver_until_taken(context: &RouterWorkerContext, mut pending: Vec<Outgoing>) {
    let mut failed_nodes = HashSet::new();
    let mut retries = 0_u32;

    loop {
        let peers = sampler_peers(context).await;
        let mut by_owner: HashMap<String, (SamplerNode, Vec<Outgoing>)> = HashMap::new();
        let mut left = Vec::new();

        for outgoing in pending.drain(..) {
            let trace_id = outgoing.queued.candidate.span.trace_id.clone();

            match owner_of(&peers, &mut failed_nodes, &trace_id) {
                Some(owner) => {
                    by_owner
                        .entry(owner.node_id.clone())
                        .or_insert_with(|| (owner, Vec::new()))
                        .1
                        .push(outgoing);
                }
                None => left.push(outgoing),
            }
        }

        let ownerless = !left.is_empty();

        for (owner, group) in by_owner.into_values() {
            left.extend(hand_to_owner(context, &owner, group, &mut failed_nodes).await);
        }

        if left.is_empty() {
            return;
        }

        pending = left;
        retries = retries.saturating_add(1);
        let reason = if ownerless {
            "no_owner"
        } else {
            "delivery_failed"
        };
        trace_metrics::record_retry("routing", reason);

        if retries.is_power_of_two() {
            tracing::warn!(
                target: "molesignal::app::trace::candidate_router",
                retries,
                candidates = pending.len(),
                reason,
                "Trace candidates not delivered yet; retrying"
            );
        }

        let shift = (retries - 1).min(16);

        let backoff = context
            .config
            .initial_backoff
            .saturating_mul(1_u32 << shift)
            .min(MAX_TENANT_BACKOFF);

        sleep(backoff).await;
    }
}

/// Hands one owner its tenant candidates; returns those it did not take.
async fn hand_to_owner(
    context: &RouterWorkerContext,
    owner: &SamplerNode,
    group: Vec<Outgoing>,
    failed_nodes: &mut HashSet<String>,
) -> Vec<Outgoing> {
    if context.local_accepts_candidates && owner.node_id == context.local_node_id {
        let mut left = Vec::new();

        for outgoing in group {
            let candidate = outgoing.queued.candidate.clone();

            match context
                .local_pipeline
                .submit(candidate, outgoing.queued.origin)
                .await
            {
                Ok(()) => delivered_local(context),
                Err(_) => {
                    failed_nodes.insert(owner.node_id.clone());
                    left.push(outgoing);
                }
            }
        }

        return left;
    }

    if context.cluster_token.is_none() {
        failed_nodes.insert(owner.node_id.clone());
        return group;
    }

    let mut left = Vec::new();
    let mut calls = calls(group).into_iter();

    while let Some(call) = calls.next() {
        match send(context, owner, &call).await {
            Remote::Accepted => {
                delivered_remote(context, call.len());
                let origins: Vec<_> = call.iter().map(|outgoing| outgoing.queued.origin).collect();
                settle(context, &origins);
            }
            Remote::Overloaded => {
                context
                    .metrics
                    .owner_overloaded
                    .fetch_add(call.len() as u64, Ordering::Relaxed);
                left.extend(call);
            }
            Remote::Failed => {
                context.drop_channel(&owner.endpoint);
                failed_nodes.insert(owner.node_id.clone());
                left.extend(call);
                left.extend(calls.by_ref().flatten());
            }
        }
    }

    left
}

/// Splits candidates for one owner into calls within the size limits.
fn calls(group: Vec<Outgoing>) -> Vec<Vec<Outgoing>> {
    let mut calls: Vec<Vec<Outgoing>> = Vec::new();
    let mut bytes = 0;

    for outgoing in group {
        let full = calls.last().is_none_or(|call| {
            call.len() >= MAX_CALL_CANDIDATES || bytes + outgoing.json.len() > MAX_CALL_BYTES
        });

        if full {
            calls.push(Vec::new());
            bytes = 0;
        }

        bytes += outgoing.json.len();
        calls
            .last_mut()
            .expect("a call was just opened")
            .push(outgoing);
    }

    calls
}

/// The process's own candidate: bounded attempts, then dropped and counted.
async fn deliver_best_effort(context: &RouterWorkerContext, outgoing: Outgoing) {
    let mut failed_nodes = HashSet::new();
    let mut no_owner = false;
    let queued = &outgoing.queued;

    for attempt_index in 0..context.config.max_attempts {
        if queued.enqueued_at.elapsed() >= context.config.max_delivery_age {
            context.metrics.expired.fetch_add(1, Ordering::Relaxed);
            trace_metrics::record_spans("routing", "dropped", 1);
            return;
        }

        if attempt_index > 0 {
            trace_metrics::record_retry("routing", "delivery_failed");
            let shift = (attempt_index - 1).min(16) as u32;

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

        let Some(owner) = owner_of(&peers, &mut failed_nodes, &queued.candidate.span.trace_id)
        else {
            no_owner = true;
            continue;
        };

        no_owner = false;

        if context.local_accepts_candidates && owner.node_id == context.local_node_id {
            match context.local_pipeline.try_submit(queued.candidate.clone()) {
                Ok(()) => return delivered_local(context),
                Err(TraceSubmitError::Full) => {
                    context
                        .metrics
                        .owner_overloaded
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    failed_nodes.insert(owner.node_id);
                }
            }

            continue;
        }

        if context.cluster_token.is_none() {
            break;
        }

        match send(context, &owner, std::slice::from_ref(&outgoing)).await {
            Remote::Accepted => return delivered_remote(context, 1),
            Remote::Overloaded => {
                context
                    .metrics
                    .owner_overloaded
                    .fetch_add(1, Ordering::Relaxed);
            }
            Remote::Failed => {
                context.drop_channel(&owner.endpoint);
                failed_nodes.insert(owner.node_id);
            }
        }
    }

    let counter = if no_owner {
        &context.metrics.no_owner
    } else {
        &context.metrics.transport_failed
    };

    counter.fetch_add(1, Ordering::Relaxed);
    trace_metrics::record_spans("routing", "dropped", 1);
}

/// The owner of `trace_id` among the nodes not failed yet; when every node failed, among all of
/// them again (a single-owner cluster retries its only node).
fn owner_of(
    peers: &[PeerInfo],
    failed_nodes: &mut HashSet<String>,
    trace_id: &str,
) -> Option<SamplerNode> {
    let nodes = peers
        .iter()
        .filter(|peer| !failed_nodes.contains(&peer.node_id))
        .map(peer_to_sampler_node)
        .collect::<Vec<_>>();

    if let Some(owner) = rendezvous_owner(trace_id, &nodes) {
        return Some(owner.clone());
    }

    if failed_nodes.is_empty() {
        return None;
    }

    failed_nodes.clear();
    let nodes = peers.iter().map(peer_to_sampler_node).collect::<Vec<_>>();
    rendezvous_owner(trace_id, &nodes).cloned()
}

/// One call to a remote owner with `call`'s candidates.
async fn send(context: &RouterWorkerContext, owner: &SamplerNode, call: &[Outgoing]) -> Remote {
    let Some(token) = context.cluster_token.as_deref() else {
        return Remote::Failed;
    };

    let request = build_request(context, call, token);

    let result = timeout(context.config.attempt_timeout, async {
        let channel = context.channel(&owner.endpoint).await?;
        let mut client = TraceCandidateServiceClient::new(channel);

        signals::grpc_trace::call(
            request,
            "cluster.v1.TraceCandidateService",
            "Submit",
            signals::grpc_trace::GrpcTarget::Internal,
            |request| client.submit(request),
        )
        .await
        .map_err(|status| format!("candidate submit failed: {}", status.code()))
    })
    .await;

    let Ok(Ok(response)) = result else {
        return Remote::Failed;
    };

    match TraceCandidateDisposition::try_from(response.get_ref().disposition) {
        Ok(TraceCandidateDisposition::Accepted) => Remote::Accepted,
        Ok(TraceCandidateDisposition::Overloaded) => Remote::Overloaded,
        _ => Remote::Failed,
    }
}

fn delivered_local(context: &RouterWorkerContext) {
    context
        .metrics
        .delivered_local
        .fetch_add(1, Ordering::Relaxed);

    trace_metrics::record_spans("routing", "accepted", 1);
}

fn delivered_remote(context: &RouterWorkerContext, count: usize) {
    context
        .metrics
        .delivered_remote
        .fetch_add(count as u64, Ordering::Relaxed);

    trace_metrics::record_spans("routing", "accepted", count as u64);
}

fn settle(context: &RouterWorkerContext, origins: &[Option<SpanOrigin>]) {
    let Some(log) = context.candidate_log.get() else {
        return;
    };

    let origins: Vec<SpanOrigin> = origins.iter().flatten().copied().collect();

    if !origins.is_empty() {
        log.settle(&origins);
    }
}

pub(super) async fn sampler_peers(context: &RouterWorkerContext) -> Vec<PeerInfo> {
    let mut peers = context
        .registry
        .list_all()
        .await
        .into_iter()
        .filter(|peer| {
            peer.roles.iter().any(|role| {
                matches!(
                    role,
                    PeerRole::Standalone | PeerRole::Intake | PeerRole::Querier
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

pub(super) fn peer_to_sampler_node(peer: &PeerInfo) -> SamplerNode {
    SamplerNode {
        node_id: peer.node_id.clone(),
        endpoint: peer.advertise_addr.clone(),
        healthy: true,
    }
}

fn build_request(
    context: &RouterWorkerContext,
    call: &[Outgoing],
    token: &str,
) -> tonic::Request<TraceCandidateServiceSubmitRequest> {
    let candidates = call
        .iter()
        .map(|outgoing| {
            let candidate = &outgoing.queued.candidate;

            let force_keep = match candidate.force_keep {
                ForceKeep::None => TraceForceKeep::Unspecified,
                ForceKeep::TrustedInternal => TraceForceKeep::TrustedInternal,
                ForceKeep::DebugToken => TraceForceKeep::DebugToken,
            };

            TraceCandidateItem {
                org_id: candidate.org_id.clone(),
                stream: candidate.stream.clone().unwrap_or_default(),
                system_self_trace: candidate.stream.is_none(),
                canonical_span_json: outgoing.json.clone().into(),
                force_keep: force_keep as i32,
            }
        })
        .collect();

    let mut request = tonic::Request::new(TraceCandidateServiceSubmitRequest {
        candidates,
        producer_node_id: context.local_node_id.clone(),
        produced_at_micros: TimestampMicros::now().0,
    });

    // Both headers are required. Metadata construction cannot fail for these static values and
    // the token was already parsed from a process environment variable.
    let _ = grpc_client::with_bearer(&mut request, token);

    request.metadata_mut().insert(
        TRACE_CANDIDATE_ORIGIN_HEADER,
        TRACE_CANDIDATE_ORIGIN_VALUE.parse().unwrap(),
    );

    request
}
