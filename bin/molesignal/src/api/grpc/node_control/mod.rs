// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `molesignal.node.control.v1.NodeControl`: the Node Agent managed-mode endpoint.
//!
//! One TLS listener with optional client certificates: `Enroll` needs none; `Session` and
//! `RotateCertificate` need a certificate of the Node Agent CA, which the TLS layer verifies before
//! the handler looks up the node it names.

mod convert;

#[cfg(test)]
mod tests;

use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{Stream, StreamExt as _};
use protocol::node_control::v1::{
    self as wire,
    agent_message::Payload,
    node_control_server::{NodeControl, NodeControlServer},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    Request, Response, Status, Streaming,
    transport::{Certificate, Identity, ServerTlsConfig},
};

use self::convert::{Frame, frame, status};
use crate::app::node_agent::{
    EnrollInput, HeartbeatInput, HelloInput, NodeAgentAuthority, NodeAgentService, Outbound,
    Session, TaskResultInput,
};

/// How often a session looks for new offers, cancels, configuration, drain and revocation.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct NodeControlGrpc {
    service: Arc<NodeAgentService>,
}

impl NodeControlGrpc {
    pub fn new(service: Arc<NodeAgentService>) -> Self {
        Self { service }
    }

    pub fn into_server(self) -> NodeControlServer<Self> {
        NodeControlServer::new(self)
    }
}

/// The listener's TLS: the server certificate of the Node Agent CA, client certificates of the
/// same CA verified when presented and optional (enrollment has none yet).
pub fn server_tls_config(authority: &NodeAgentAuthority) -> ServerTlsConfig {
    let tls = authority.server_tls();

    ServerTlsConfig::new()
        .identity(Identity::from_pem(
            tls.certificate_chain_pem,
            tls.private_key_pem,
        ))
        .client_ca_root(Certificate::from_pem(tls.ca_certificate_pem))
        .client_auth_optional(true)
}

fn peer_certificate<T>(request: &Request<T>) -> Result<Vec<u8>, Status> {
    request
        .peer_certs()
        .and_then(|chain| chain.first().map(|certificate| certificate.to_vec()))
        .ok_or_else(|| Status::unauthenticated("a node client certificate is required"))
}

#[tonic::async_trait]
impl NodeControl for NodeControlGrpc {
    async fn enroll(
        &self,
        request: Request<wire::EnrollRequest>,
    ) -> Result<Response<wire::EnrollResponse>, Status> {
        let value = request.into_inner();

        let outcome = self
            .service
            .enroll(EnrollInput {
                enrollment_token: value.enrollment_token,
                csr_pem: value.csr_pem,
                host_id: value.host_id,
                boot_id: value.boot_id,
                hostname: value.hostname,
                agent_version: value.agent_version,
                protocol_version: value.protocol_version,
                capabilities: convert::capabilities(value.capabilities),
            })
            .await
            .map_err(status)?;

        Ok(Response::new(wire::EnrollResponse {
            node_id: outcome.node.id.0,
            certificate_pem: outcome.certificate_pem,
            ca_bundle_pem: outcome.ca_bundle_pem,
            not_after: convert::seconds(outcome.not_after),
            protocol_version: domain::node_agent::ProtocolVersion::CURRENT.to_string(),
        }))
    }

    async fn rotate_certificate(
        &self,
        request: Request<wire::RotateCertificateRequest>,
    ) -> Result<Response<wire::RotateCertificateResponse>, Status> {
        let certificate = peer_certificate(&request)?;

        let outcome = self
            .service
            .rotate_certificate(&certificate, &request.into_inner().csr_pem)
            .await
            .map_err(status)?;

        Ok(Response::new(wire::RotateCertificateResponse {
            certificate_pem: outcome.certificate_pem,
            ca_bundle_pem: outcome.ca_bundle_pem,
            not_after: convert::seconds(outcome.not_after),
        }))
    }

    type SessionStream =
        Pin<Box<dyn Stream<Item = Result<wire::ControlMessage, Status>> + Send + 'static>>;

    async fn session(
        &self,
        request: Request<Streaming<wire::AgentMessage>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let certificate = peer_certificate(&request)?;
        let auth = self
            .service
            .authenticate(&certificate)
            .await
            .map_err(status)?;
        let mut input = request.into_inner();

        let first = tokio::time::timeout(HELLO_TIMEOUT, input.message())
            .await
            .map_err(|_| Status::deadline_exceeded("no Hello within 15 seconds"))??
            .ok_or_else(|| Status::invalid_argument("the stream ended before Hello"))?;

        let Some(Payload::Hello(hello)) = first.payload else {
            return Err(Status::failed_precondition(
                "the first message must be Hello",
            ));
        };

        let hello = HelloInput {
            node_id: hello.node_id,
            protocol_version: hello.protocol_version,
            agent_version: hello.agent_version.chars().take(64).collect(),
            capabilities: convert::capabilities(hello.capabilities),
            applied_config_revision: hello.applied_config_revision,
            active_leases: convert::lease_keys(hello.active_leases),
            pending_results: hello.pending_results,
        };

        let (session, opening) = self
            .service
            .open_session(auth, hello)
            .await
            .map_err(status)?;

        let (output, receiver) = mpsc::channel(64);
        let service = self.service.clone();

        tokio::spawn(async move {
            if !send(&output, opening).await {
                return;
            }

            if let Some(session) = session {
                run(service, session, input, output).await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

type Output = mpsc::Sender<Result<wire::ControlMessage, Status>>;

/// Sends the messages in order; `false` once the session is over.
async fn send(output: &Output, messages: Vec<Outbound>) -> bool {
    for message in messages {
        match frame(message) {
            Frame::Send(message) => {
                if output.send(Ok(message)).await.is_err() {
                    return false;
                }
            }
            Frame::End => return false,
            Frame::Fail(error) => {
                let _ = output.send(Err(error)).await;
                return false;
            }
        }
    }

    true
}

async fn run(
    service: Arc<NodeAgentService>,
    mut session: Session,
    mut input: Streaming<wire::AgentMessage>,
    output: Output,
) {
    let quiet_limit = Duration::from_secs(u64::from(service.timings().heartbeat_interval_secs) * 3);
    let mut poll = tokio::time::interval(POLL_INTERVAL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_seen = tokio::time::Instant::now();

    loop {
        let outcome = tokio::select! {
            message = input.next() => match message {
                Some(Ok(message)) => {
                    last_seen = tokio::time::Instant::now();
                    handle(&service, &mut session, message).await
                }
                Some(Err(error)) => {
                    tracing::debug!(node_id = session.node_id.as_str(), %error, "node session stream failed");
                    break;
                }
                None => break,
            },
            _ = poll.tick() => {
                if last_seen.elapsed() > quiet_limit {
                    let _ = output
                        .send(Err(Status::deadline_exceeded("no heartbeat for three intervals")))
                        .await;
                    break;
                }

                service.poll(&mut session).await.map_err(status)
            }
        };

        let keep_going = match outcome {
            Ok(messages) => send(&output, messages).await,
            Err(error) => {
                let _ = output.send(Err(error)).await;
                false
            }
        };

        if !keep_going {
            break;
        }
    }
}

async fn handle(
    service: &NodeAgentService,
    session: &mut Session,
    message: wire::AgentMessage,
) -> Result<Vec<Outbound>, Status> {
    let result = match message.payload {
        Some(Payload::Hello(_)) => {
            return Err(Status::failed_precondition("Hello was already accepted"));
        }
        Some(Payload::Heartbeat(value)) => {
            service
                .heartbeat(
                    session,
                    HeartbeatInput {
                        seq: value.seq,
                        applied_config_revision: value.applied_config_revision,
                        health: convert::health(value.health),
                        collector_states: convert::collector_states(value.collector_states),
                        active_leases: convert::lease_keys(value.active_leases),
                    },
                )
                .await
        }
        Some(Payload::ConfigStatus(value)) => {
            let state = convert::config_state(value.state)?;
            let reason = (!value.reason.is_empty()).then_some(value.reason);
            service
                .config_status(session, value.revision, state, reason)
                .await
        }
        Some(Payload::LeaseAccept(value)) => {
            service
                .lease_accept(session, convert::lease_key(value.task_id, value.attempt))
                .await
        }
        Some(Payload::LeaseReject(value)) => {
            let reason = convert::reject_reason(value.reason)?;

            service
                .lease_reject(
                    session,
                    convert::lease_key(value.task_id, value.attempt),
                    reason,
                )
                .await
        }
        Some(Payload::TaskResult(value)) => {
            let status = convert::task_status(value.status)?;

            service
                .task_result(
                    session,
                    TaskResultInput {
                        result_id: value.result_id,
                        task_id: value.task_id,
                        attempt: value.attempt,
                        status,
                        summary: value.summary,
                        artifact_refs: value.artifact_refs,
                    },
                )
                .await
        }
        Some(Payload::Drained(_)) => service.drained(session).await,
        // A message of a later minor version: ignored.
        None => Ok(Vec::new()),
    };

    result.map_err(status)
}
