// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use url::Url;

use crate::{
    executor,
    identity::{AgentIdentity, PendingKey, StoredIdentity},
    protocol::v1::{
        self as wire, AgentFrame, AgentHello, Capability, CertificateRotationRequest, ControlFrame,
        RegisterRequest, TaskAck, agent_frame, control_frame,
        probe_service_client::ProbeServiceClient,
    },
    spool::ResultSpool,
};

const PROTOCOL_VERSION: u32 = 1;

pub struct RegisterOptions {
    pub state_dir: PathBuf,
    pub endpoint: String,
    pub token: String,
    pub ca_certificate_base64: String,
    pub hostname: Option<String>,
}

pub struct RunOptions {
    pub state_dir: PathBuf,
    pub max_concurrent: u32,
    pub max_browser_concurrent: u32,
}

pub async fn register(options: RegisterOptions) -> Result<()> {
    if options.state_dir.join("identity.json").exists() {
        bail!("this Probe Agent is already registered; revoke it before replacing its identity");
    }
    let ca = base64::engine::general_purpose::STANDARD
        .decode(&options.ca_certificate_base64)
        .context("decode Probe CA certificate")?;
    let ca = String::from_utf8(ca).context("Probe CA certificate is not UTF-8 PEM")?;
    let pending = PendingKey::generate()?;
    let hostname = options
        .hostname
        .or_else(|| {
            hostname::get()
                .ok()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "molesignal-probe".to_string());
    let mut client = ProbeServiceClient::new(connect_tls(&options.endpoint, &ca, None).await?);
    let response = client
        .register(RegisterRequest {
            register_token: options.token,
            public_key_der: pending.public_key_der().into(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
            hostname,
            capabilities: capabilities(),
            labels: Default::default(),
        })
        .await
        .context("register Probe Agent")?
        .into_inner();
    let returned_ca = String::from_utf8(response.ca_certificate_pem.to_vec())
        .context("server returned invalid Probe CA PEM")?;
    if returned_ca.as_bytes() != ca.as_bytes() {
        bail!("registration response CA does not match the pinned Probe CA");
    }
    let identity = AgentIdentity {
        metadata: StoredIdentity {
            agent_id: response.agent_id,
            location_id: response.location_id,
            control_endpoint: response.control_endpoint,
            certificate_expires_at_micros: response.certificate_expires_at_micros,
            protocol_version: response.protocol_version,
        },
        private_key_pem: pending.private_key_pem(),
        certificate_chain_pem: String::from_utf8(response.certificate_chain_pem.to_vec())
            .context("server returned invalid Agent certificate PEM")?,
        ca_certificate_pem: returned_ca,
    };
    identity.persist(&options.state_dir)?;
    tracing::info!(agent_id = %identity.metadata.agent_id, "Probe Agent registered");
    Ok(())
}

pub async fn run(options: RunOptions) -> Result<()> {
    let spool = ResultSpool::open(options.state_dir.clone())?;
    let sequence = Arc::new(AtomicU64::new(
        spool
            .pending()?
            .iter()
            .map(|entry| entry.result.result_sequence)
            .max()
            .unwrap_or(0),
    ));
    let mut delay = Duration::from_secs(1);
    loop {
        let identity = AgentIdentity::load(&options.state_dir)?;
        match run_session(&options, identity, spool.clone(), sequence.clone()).await {
            Ok(()) => tracing::warn!("Probe control stream closed"),
            Err(error) => tracing::warn!(%error, "Probe control stream failed"),
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

async fn run_session(
    options: &RunOptions,
    identity: AgentIdentity,
    spool: ResultSpool,
    sequence: Arc<AtomicU64>,
) -> Result<()> {
    let channel = connect_tls(
        &identity.metadata.control_endpoint,
        &identity.ca_certificate_pem,
        Some((&identity.certificate_chain_pem, &identity.private_key_pem)),
    )
    .await?;
    let mut client = ProbeServiceClient::new(channel);
    let (outbound, receiver) = mpsc::channel::<AgentFrame>(64);
    let response = client
        .control_stream(ReceiverStream::new(receiver))
        .await
        .context("open Probe control stream")?;
    let mut inbound = response.into_inner();
    let active_tasks = Arc::new(AtomicU32::new(0));
    let semaphore = Arc::new(Semaphore::new(options.max_concurrent.clamp(1, 32) as usize));
    let pending_rotation = Arc::new(Mutex::new(None::<PendingKey>));

    outbound
        .send(frame(agent_frame::Payload::Hello(AgentHello {
            agent_id: identity.metadata.agent_id.clone(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: capabilities(),
            capacity: Some(capacity(options, &semaphore)),
            last_acked_result_sequence: sequence.load(Ordering::Relaxed),
        })))
        .await?;
    let first = inbound
        .message()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Probe stream ended before HelloAck"))?;
    if !matches!(first.payload, Some(control_frame::Payload::HelloAck(_))) {
        bail!("Probe server did not acknowledge Hello");
    }
    for entry in spool.pending()? {
        outbound
            .send(frame(agent_frame::Payload::Result(entry.result)))
            .await?;
    }
    maybe_request_rotation(&identity, &outbound, &pending_rotation).await?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                outbound.send(frame(agent_frame::Payload::Heartbeat(wire::AgentHeartbeat {
                    observed_at_micros: now_micros(),
                    capacity: Some(capacity(options, &semaphore)),
                    active_tasks: active_tasks.load(Ordering::Relaxed),
                    result_spool_bytes: spool.size_bytes(),
                    draining: false,
                }))).await?;
            }
            message = inbound.message() => {
                let Some(message) = message? else { return Ok(()); };
                handle_control(
                    options,
                    message,
                    &outbound,
                    &semaphore,
                    &active_tasks,
                    &spool,
                    &sequence,
                    &pending_rotation,
                ).await?;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_control(
    options: &RunOptions,
    message: ControlFrame,
    outbound: &mpsc::Sender<AgentFrame>,
    semaphore: &Arc<Semaphore>,
    active_tasks: &Arc<AtomicU32>,
    spool: &ResultSpool,
    sequence: &Arc<AtomicU64>,
    pending_rotation: &Arc<Mutex<Option<PendingKey>>>,
) -> Result<()> {
    match message.payload {
        Some(control_frame::Payload::Task(task)) => {
            let permit = semaphore.clone().try_acquire_owned();
            let accepted = permit.is_ok();
            outbound
                .send(frame(agent_frame::Payload::TaskAck(TaskAck {
                    task_id: task.task_id.clone(),
                    lease_token: task.lease_token.clone(),
                    accepted,
                    rejection_code: if accepted {
                        String::new()
                    } else {
                        "capacity_exhausted".into()
                    },
                })))
                .await?;
            if let Ok(permit) = permit {
                let outbound = outbound.clone();
                let spool = spool.clone();
                let sequence = sequence.clone();
                let active_tasks = active_tasks.clone();
                active_tasks.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let result_sequence = sequence.fetch_add(1, Ordering::SeqCst) + 1;
                    let result = executor::execute(task, result_sequence).await;
                    match result {
                        Ok(result) => {
                            if let Err(error) = spool.store(&result) {
                                tracing::error!(%error, "cannot spool Probe result; result not sent");
                            } else if outbound
                                .send(frame(agent_frame::Payload::Result(result)))
                                .await
                                .is_err()
                            {
                                tracing::warn!("Probe result remains spooled after stream close");
                            }
                        }
                        Err(error) => {
                            tracing::error!(%error, "Probe task execution failed internally")
                        }
                    }
                    active_tasks.fetch_sub(1, Ordering::Relaxed);
                    drop(permit);
                });
            }
        }
        Some(control_frame::Payload::ResultAck(ack)) => {
            spool.acknowledge(&ack.task_id, ack.result_sequence)?;
        }
        Some(control_frame::Payload::RotateCertificate(rotated)) => {
            let Some(pending) = pending_rotation.lock().await.take() else {
                bail!("received an unsolicited Probe certificate rotation");
            };
            let mut installed = AgentIdentity::load(&options.state_dir)?;
            installed.install_rotated(
                &options.state_dir,
                pending,
                String::from_utf8(rotated.certificate_chain_pem.to_vec())?,
                String::from_utf8(rotated.ca_certificate_pem.to_vec())?,
                rotated.expires_at_micros,
            )?;
            tracing::info!("rotated Probe Agent certificate; reconnecting with new identity");
            return Err(anyhow::anyhow!("Probe identity rotated"));
        }
        Some(control_frame::Payload::CancelTask(cancel)) => {
            tracing::warn!(task_id = %cancel.task_id, reason = %cancel.reason, "Probe task cancellation requested");
        }
        Some(control_frame::Payload::DrainAgent(drain)) => {
            outbound
                .send(frame(agent_frame::Payload::Drain(wire::AgentDrain {
                    reason: drain.reason,
                })))
                .await?;
        }
        Some(control_frame::Payload::HelloAck(_)) => {}
        None => bail!("Probe control frame has no payload"),
    }
    Ok(())
}

async fn maybe_request_rotation(
    identity: &AgentIdentity,
    outbound: &mpsc::Sender<AgentFrame>,
    pending_rotation: &Arc<Mutex<Option<PendingKey>>>,
) -> Result<()> {
    if identity.metadata.certificate_expires_at_micros - now_micros() > 7 * 24 * 60 * 60 * 1_000_000
    {
        return Ok(());
    }
    let pending = PendingKey::generate()?;
    let public_key_der = pending.public_key_der();
    *pending_rotation.lock().await = Some(pending);
    outbound
        .send(frame(agent_frame::Payload::CertificateRotation(
            CertificateRotationRequest {
                public_key_der: public_key_der.into(),
            },
        )))
        .await?;
    Ok(())
}

async fn connect_tls(
    endpoint: &str,
    ca_pem: &str,
    identity: Option<(&str, &str)>,
) -> Result<Channel> {
    let url = Url::parse(endpoint).context("parse Probe endpoint")?;
    if url.scheme() != "https" {
        bail!("Probe endpoint must use https");
    }
    let domain = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Probe endpoint has no host"))?;
    let mut tls = ClientTlsConfig::new()
        .domain_name(domain.to_string())
        .ca_certificate(Certificate::from_pem(ca_pem));
    if let Some((certificate, private_key)) = identity {
        tls = tls.identity(Identity::from_pem(certificate, private_key));
    }
    Endpoint::from_shared(endpoint.to_string())?
        .tls_config(tls)?
        .connect()
        .await
        .context("connect to Probe endpoint")
}

fn capacity(options: &RunOptions, semaphore: &Semaphore) -> wire::AgentCapacity {
    wire::AgentCapacity {
        max_concurrent: options.max_concurrent.clamp(1, 32),
        max_browser_concurrent: options.max_browser_concurrent.min(4),
        available: semaphore.available_permits().min(32) as u32,
        available_browser: options.max_browser_concurrent.min(4),
    }
}

fn capabilities() -> Vec<i32> {
    [
        Capability::Http,
        Capability::Tcp,
        Capability::Dns,
        Capability::Icmp,
        Capability::Tls,
        Capability::Grpc,
        Capability::Browser,
    ]
    .into_iter()
    .map(|value| value as i32)
    .collect()
}

fn frame(payload: agent_frame::Payload) -> AgentFrame {
    AgentFrame {
        frame_id: format!("{}-{}", now_micros(), rand::random::<u64>()),
        payload: Some(payload),
    }
}

fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}
