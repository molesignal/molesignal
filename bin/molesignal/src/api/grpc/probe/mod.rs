// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

pub(crate) mod result;
pub(crate) mod task;

use std::{collections::HashSet, pin::Pin, sync::Arc, time::Duration};

use futures::{Stream, StreamExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use self::{result::result_from_wire, task::capabilities_from_wire};
use crate::{
    app::synthetics::{PROBE_PROTOCOL_VERSION, ProbeControlService, ProbeRegisterInput},
    domain::synthetics::{AgentCapacity, AgentStatus, ProbeAgent},
    protocol::probe::v1::{
        self as wire, agent_frame, control_frame,
        probe_service_server::{ProbeService, ProbeServiceServer},
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeListener {
    Register,
    Control,
}

#[derive(Clone)]
pub struct ProbeGrpc {
    service: Arc<ProbeControlService>,
    listener: ProbeListener,
}

impl ProbeGrpc {
    pub fn new(service: Arc<ProbeControlService>, listener: ProbeListener) -> Self {
        Self { service, listener }
    }

    pub fn into_server(self) -> ProbeServiceServer<Self> {
        ProbeServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl ProbeService for ProbeGrpc {
    async fn register(
        &self,
        request: Request<wire::RegisterRequest>,
    ) -> Result<Response<wire::RegisterResponse>, Status> {
        if self.listener != ProbeListener::Register {
            return Err(Status::unimplemented(
                "Register is only available on the Probe registration endpoint",
            ));
        }
        let value = request.into_inner();
        let outcome = self
            .service
            .register(ProbeRegisterInput {
                register_token: value.register_token,
                public_key_der: value.public_key_der.to_vec(),
                agent_version: value.agent_version,
                protocol_version: value.protocol_version,
                hostname: value.hostname,
                capabilities: capabilities_from_wire(&value.capabilities).map_err(to_status)?,
                labels: value.labels.into_iter().collect(),
            })
            .await
            .map_err(to_status)?;
        Ok(Response::new(wire::RegisterResponse {
            agent_id: outcome.agent.id.0,
            location_id: outcome.agent.location_id.0,
            certificate_chain_pem: outcome.certificate_chain_pem.into_bytes().into(),
            ca_certificate_pem: outcome.ca_certificate_pem.into_bytes().into(),
            certificate_expires_at_micros: outcome.certificate_expires_at.0,
            control_endpoint: outcome.control_endpoint,
            protocol_version: outcome.protocol_version,
        }))
    }

    type ControlStreamStream =
        Pin<Box<dyn Stream<Item = Result<wire::ControlFrame, Status>> + Send + 'static>>;

    async fn control_stream(
        &self,
        request: Request<Streaming<wire::AgentFrame>>,
    ) -> Result<Response<Self::ControlStreamStream>, Status> {
        if self.listener != ProbeListener::Control {
            return Err(Status::unimplemented(
                "ControlStream is only available on the Probe control endpoint",
            ));
        }
        let certificate = request
            .peer_certs()
            .and_then(|chain| chain.first().cloned())
            .ok_or_else(|| Status::unauthenticated("Probe client certificate is required"))?;
        let mut agent = self
            .service
            .authenticate_certificate(certificate.as_ref())
            .await
            .map_err(to_status)?;
        let mut input = request.into_inner();
        let first = tokio::time::timeout(Duration::from_secs(15), input.message())
            .await
            .map_err(|_| Status::deadline_exceeded("Probe Hello timed out"))?
            .map_err(|_| Status::invalid_argument("invalid Probe Hello frame"))?
            .ok_or_else(|| Status::invalid_argument("Probe stream ended before Hello"))?;
        let Some(agent_frame::Payload::Hello(hello)) = first.payload else {
            return Err(Status::failed_precondition(
                "the first Probe frame must be Hello",
            ));
        };
        agent = self
            .service
            .accept_hello(
                agent,
                &hello.agent_id,
                hello.agent_version,
                hello.protocol_version,
                capabilities_from_wire(&hello.capabilities).map_err(to_status)?,
                capacity_from_wire(hello.capacity),
            )
            .await
            .map_err(to_status)?;

        let (output, receiver) = mpsc::channel(64);
        let service = self.service.clone();
        tokio::spawn(async move {
            if output
                .send(Ok(control_frame(control_frame::Payload::HelloAck(
                    wire::HelloAck {
                        protocol_version: PROBE_PROTOCOL_VERSION,
                        server_time_micros: TimestampMicros::now().0,
                        heartbeat_interval_secs: 15,
                        lease_renewal_interval_secs: 30,
                        max_in_flight: agent.capacity.max_concurrent.min(32),
                    },
                ))))
                .await
                .is_err()
            {
                return;
            }
            run_stream(service, input, output, agent).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

async fn run_stream(
    service: Arc<ProbeControlService>,
    mut input: Streaming<wire::AgentFrame>,
    output: mpsc::Sender<Result<wire::ControlFrame, Status>>,
    mut agent: ProbeAgent,
) {
    let mut in_flight = HashSet::new();
    let mut dispatch = tokio::time::interval(Duration::from_secs(2));
    dispatch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let outcome = tokio::select! {
            frame = input.next() => match frame {
                Some(Ok(frame)) => handle_frame(
                    &service, &output, &mut agent, &mut in_flight, frame,
                ).await,
                Some(Err(status)) => Err(status),
                None => break,
            },
            _ = dispatch.tick() => dispatch_tasks(
                &service, &output, &agent, &mut in_flight,
            ).await,
        };
        if let Err(status) = outcome {
            let _ = output.send(Err(status)).await;
            break;
        }
    }
}

async fn handle_frame(
    service: &ProbeControlService,
    output: &mpsc::Sender<Result<wire::ControlFrame, Status>>,
    agent: &mut ProbeAgent,
    in_flight: &mut HashSet<Id>,
    frame: wire::AgentFrame,
) -> Result<(), Status> {
    if frame.frame_id.is_empty() || frame.frame_id.len() > 128 {
        return Err(Status::invalid_argument("invalid Probe frame id"));
    }
    match frame.payload {
        Some(agent_frame::Payload::Hello(_)) => Err(Status::failed_precondition(
            "Probe Hello was already accepted",
        )),
        Some(agent_frame::Payload::Heartbeat(value)) => {
            *agent = service
                .heartbeat(
                    agent.clone(),
                    capacity_from_wire(value.capacity),
                    value.draining,
                    TimestampMicros(value.observed_at_micros),
                )
                .await
                .map_err(to_status)?;
            dispatch_tasks(service, output, agent, in_flight).await
        }
        Some(agent_frame::Payload::TaskAck(value)) => {
            let task_id = Id::from_string(value.task_id);
            service
                .acknowledge_task(agent, &task_id, &value.lease_token, value.accepted)
                .await
                .map_err(to_status)?;
            if !value.accepted {
                in_flight.remove(&task_id);
            }
            Ok(())
        }
        Some(agent_frame::Payload::LeaseRenewal(value)) => {
            service
                .renew_task_lease(
                    agent,
                    &Id::from_string(value.task_id),
                    &value.lease_token,
                    TimestampMicros(value.requested_until_micros),
                )
                .await
                .map_err(to_status)?;
            Ok(())
        }
        Some(agent_frame::Payload::Result(value)) => {
            let task_id = Id::from_string(value.task_id.clone());
            let task = service
                .verify_lease(agent, &task_id, &value.lease_token)
                .await
                .map_err(to_status)?;
            let sequence = value.result_sequence;
            let result = result_from_wire(agent, &task, value).map_err(to_status)?;
            service.process_result(result).await.map_err(to_status)?;
            in_flight.remove(&task_id);
            output
                .send(Ok(control_frame(control_frame::Payload::ResultAck(
                    wire::ResultAck {
                        task_id: task_id.0,
                        result_sequence: sequence,
                    },
                ))))
                .await
                .map_err(|_| Status::cancelled("Probe stream closed"))
        }
        Some(agent_frame::Payload::Drain(_)) => {
            agent.status = AgentStatus::Draining;
            *agent = service
                .heartbeat(agent.clone(), agent.capacity, true, TimestampMicros::now())
                .await
                .map_err(to_status)?;
            Ok(())
        }
        Some(agent_frame::Payload::CertificateRotation(value)) => {
            let issued = service
                .rotate_certificate(agent, &value.public_key_der)
                .await
                .map_err(to_status)?;
            agent.public_key_der = value.public_key_der.to_vec();
            agent.certificate_serial = Some(issued.serial);
            agent.certificate_expires_at = Some(issued.expires_at);
            let tls = service.server_tls();
            output
                .send(Ok(control_frame(
                    control_frame::Payload::RotateCertificate(wire::RotateCertificate {
                        certificate_chain_pem: issued.certificate_chain_pem.into_bytes().into(),
                        ca_certificate_pem: tls.ca_certificate_pem.into_bytes().into(),
                        expires_at_micros: issued.expires_at.0,
                    }),
                )))
                .await
                .map_err(|_| Status::cancelled("Probe stream closed"))
        }
        None => Err(Status::invalid_argument("Probe frame payload is required")),
    }
}

async fn dispatch_tasks(
    service: &ProbeControlService,
    output: &mpsc::Sender<Result<wire::ControlFrame, Status>>,
    agent: &ProbeAgent,
    in_flight: &mut HashSet<Id>,
) -> Result<(), Status> {
    if agent.status == AgentStatus::Draining {
        return Ok(());
    }
    let available = agent
        .capacity
        .available
        .min(agent.capacity.max_concurrent)
        .min(32) as usize;
    while in_flight.len() < available {
        let Some((task, token)) = service.lease_next(agent).await.map_err(to_status)? else {
            break;
        };
        let task_id = task.id.clone();
        let wire = async {
            let location = service.task_location(&task).await?;
            let secrets = service.resolve_task_secrets(&task).await?;
            task::task_to_wire(task, token.clone(), location, secrets)
        }
        .await;
        let wire = match wire {
            Ok(value) => value,
            Err(error) => {
                let _ = service
                    .acknowledge_task(agent, &task_id, &token, false)
                    .await;
                return Err(to_status(error));
            }
        };
        in_flight.insert(task_id);
        output
            .send(Ok(control_frame(control_frame::Payload::Task(wire))))
            .await
            .map_err(|_| Status::cancelled("Probe stream closed"))?;
    }
    Ok(())
}

fn capacity_from_wire(value: Option<wire::AgentCapacity>) -> AgentCapacity {
    value.map_or_else(AgentCapacity::default, |value| AgentCapacity {
        max_concurrent: value.max_concurrent.min(32),
        max_browser_concurrent: value.max_browser_concurrent.min(4),
        available: value.available.min(32),
        available_browser: value.available_browser.min(4),
    })
}

fn control_frame(payload: control_frame::Payload) -> wire::ControlFrame {
    wire::ControlFrame {
        frame_id: Id::new().0,
        payload: Some(payload),
    }
}

fn to_status(error: Error) -> Status {
    match error {
        Error::NotFound(message) => Status::not_found(message),
        Error::Conflict(message) => Status::failed_precondition(message),
        Error::InvalidArgument(message) | Error::Validation { message, .. } => {
            Status::invalid_argument(message)
        }
        Error::Unauthorized(message) => Status::unauthenticated(message),
        Error::Forbidden(message) | Error::PaymentRequired(message) => {
            Status::permission_denied(message)
        }
        Error::ResourceExhausted(message) | Error::PayloadTooLarge(message) => {
            Status::resource_exhausted(message)
        }
        Error::Cancelled(message) => Status::cancelled(message),
        Error::Unavailable(message) => Status::unavailable(message),
        Error::Internal(message) => {
            tracing::error!(error = %message, "Probe gRPC internal error");
            Status::internal("internal Probe control error")
        }
        Error::Other(error) => {
            tracing::error!(error = %error, "Probe gRPC internal error");
            Status::internal("internal Probe control error")
        }
    }
}
