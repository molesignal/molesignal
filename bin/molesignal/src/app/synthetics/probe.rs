// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, sync::Arc};

use postgres::synthetics::{IssuedProbeCertificate, ProbeCertificateAuthority, ProbeServerTls};
use serde::Serialize;
use sha2::{Digest, Sha256};
use x509_cert::der::Decode as _;

use super::{ProcessResultOutcome, SyntheticService};
use crate::{
    domain::synthetics::{
        AgentCapacity, AgentStatus, BrowserAction, MonitorSpec, ProbeAgent, ProbeCapability,
        ProbeLocation, ProbeTask, SecretMaterial, SyntheticRepository, SyntheticResult,
        ValueSource,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub const PROBE_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct ProbeRegisterInput {
    pub register_token: String,
    pub public_key_der: Vec<u8>,
    pub agent_version: String,
    pub protocol_version: u32,
    pub hostname: String,
    pub capabilities: Vec<ProbeCapability>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeRegisterOutcome {
    pub agent: ProbeAgent,
    pub certificate_chain_pem: String,
    pub ca_certificate_pem: String,
    pub certificate_expires_at: TimestampMicros,
    pub control_endpoint: String,
    pub protocol_version: u32,
}

#[derive(Clone)]
pub struct ResolvedProbeSecret {
    pub reference: String,
    pub secret_id: Id,
    pub version: u32,
    pub material: SecretMaterial,
}

pub struct ProbeControlService {
    repository: Arc<dyn SyntheticRepository>,
    synthetics: Arc<SyntheticService>,
    authority: Arc<ProbeCertificateAuthority>,
    control_endpoint: String,
}

impl ProbeControlService {
    pub fn new(
        repository: Arc<dyn SyntheticRepository>,
        synthetics: Arc<SyntheticService>,
        authority: Arc<ProbeCertificateAuthority>,
        control_endpoint: String,
    ) -> Self {
        Self {
            repository,
            synthetics,
            authority,
            control_endpoint,
        }
    }

    pub async fn register(&self, input: ProbeRegisterInput) -> Result<ProbeRegisterOutcome> {
        validate_register(&input)?;
        let now = TimestampMicros::now();
        let token_hash = Sha256::digest(input.register_token.as_bytes()).to_vec();
        let grant = self.repository.get_register_token(&token_hash, now).await?;
        let agent_id = Id::new();
        let issued = self
            .authority
            .issue_agent(&agent_id, &input.public_key_der)?;
        let agent = self
            .repository
            .consume_register_token(
                &token_hash,
                ProbeAgent {
                    id: agent_id,
                    organization_id: Some(grant.organization_id),
                    location_id: grant.location_id,
                    name: input.hostname.clone(),
                    hostname: input.hostname,
                    status: AgentStatus::Registered,
                    agent_version: input.agent_version,
                    protocol_version: input.protocol_version,
                    capabilities: input.capabilities,
                    capacity: AgentCapacity::default(),
                    labels: input.labels,
                    public_key_der: input.public_key_der,
                    certificate_serial: Some(issued.serial),
                    certificate_expires_at: Some(issued.expires_at),
                    last_heartbeat_at: None,
                    last_result_sequence: 0,
                    revoked_at: None,
                    created_at: now,
                    updated_at: now,
                },
                now,
            )
            .await?;
        Ok(ProbeRegisterOutcome {
            agent,
            certificate_chain_pem: issued.certificate_chain_pem,
            ca_certificate_pem: self.authority.ca_certificate_pem().to_string(),
            certificate_expires_at: issued.expires_at,
            control_endpoint: self.control_endpoint.clone(),
            protocol_version: PROBE_PROTOCOL_VERSION,
        })
    }

    pub async fn authenticate_certificate(&self, certificate_der: &[u8]) -> Result<ProbeAgent> {
        let certificate = x509_cert::Certificate::from_der(certificate_der)
            .map_err(|_| Error::unauthorized("invalid Probe client certificate"))?;
        let serial = certificate
            .tbs_certificate
            .serial_number
            .to_string()
            .to_ascii_lowercase();
        let agent = self
            .repository
            .get_agent_by_certificate_serial(&serial)
            .await
            .map_err(|_| Error::unauthorized("unknown or revoked Probe identity"))?;
        if agent
            .certificate_expires_at
            .is_none_or(|expires| expires <= TimestampMicros::now())
        {
            return Err(Error::unauthorized("Probe client certificate expired"));
        }
        Ok(agent)
    }

    pub async fn accept_hello(
        &self,
        mut agent: ProbeAgent,
        claimed_agent_id: &str,
        agent_version: String,
        protocol_version: u32,
        capabilities: Vec<ProbeCapability>,
        capacity: AgentCapacity,
    ) -> Result<ProbeAgent> {
        if agent.id.as_str() != claimed_agent_id {
            return Err(Error::unauthorized(
                "Probe hello identity does not match its certificate",
            ));
        }
        validate_protocol(protocol_version)?;
        agent.status = AgentStatus::Online;
        agent.agent_version = agent_version;
        agent.protocol_version = protocol_version;
        agent.capabilities = capabilities;
        agent.capacity = capacity;
        agent.last_heartbeat_at = Some(TimestampMicros::now());
        agent.updated_at = TimestampMicros::now();
        self.repository.update_agent(agent).await
    }

    pub async fn heartbeat(
        &self,
        mut agent: ProbeAgent,
        capacity: AgentCapacity,
        draining: bool,
        observed_at: TimestampMicros,
    ) -> Result<ProbeAgent> {
        agent.status = if draining {
            AgentStatus::Draining
        } else {
            AgentStatus::Online
        };
        agent.capacity = capacity;
        agent.last_heartbeat_at = Some(observed_at);
        agent.updated_at = TimestampMicros::now();
        self.repository.update_agent(agent).await
    }

    pub async fn lease_next(&self, agent: &ProbeAgent) -> Result<Option<(ProbeTask, String)>> {
        self.synthetics.lease_next_probe_task(agent).await
    }

    pub async fn verify_lease(
        &self,
        agent: &ProbeAgent,
        task_id: &Id,
        lease_token: &str,
    ) -> Result<ProbeTask> {
        let hash = hex::encode(Sha256::digest(lease_token.as_bytes()));
        self.repository
            .get_leased_task(task_id, &agent.id, &hash, TimestampMicros::now())
            .await
            .map_err(|_| Error::unauthorized("invalid or expired Probe task lease"))
    }

    pub async fn process_result(&self, result: SyntheticResult) -> Result<ProcessResultOutcome> {
        self.synthetics.process_result(result).await
    }

    pub async fn acknowledge_task(
        &self,
        agent: &ProbeAgent,
        task_id: &Id,
        lease_token: &str,
        accepted: bool,
    ) -> Result<()> {
        self.synthetics
            .acknowledge_probe_task(agent, task_id, lease_token, accepted)
            .await
    }

    pub async fn renew_task_lease(
        &self,
        agent: &ProbeAgent,
        task_id: &Id,
        lease_token: &str,
        requested_until: TimestampMicros,
    ) -> Result<TimestampMicros> {
        self.synthetics
            .renew_probe_task_lease(agent, task_id, lease_token, requested_until)
            .await
    }

    pub async fn resolve_task_secrets(&self, task: &ProbeTask) -> Result<Vec<ResolvedProbeSecret>> {
        self.synthetics.resolve_probe_task_secrets(task).await
    }

    pub async fn task_location(&self, task: &ProbeTask) -> Result<ProbeLocation> {
        self.synthetics.probe_task_location(task).await
    }

    pub fn server_tls(&self) -> ProbeServerTls {
        self.authority.server_tls()
    }

    pub async fn rotate_certificate(
        &self,
        agent: &ProbeAgent,
        public_key_der: &[u8],
    ) -> Result<IssuedProbeCertificate> {
        let issued = self.authority.issue_agent(&agent.id, public_key_der)?;
        let mut updated = agent.clone();
        updated.public_key_der = public_key_der.to_vec();
        updated.certificate_serial = Some(issued.serial.clone());
        updated.certificate_expires_at = Some(issued.expires_at);
        updated.updated_at = TimestampMicros::now();
        self.repository.update_agent(updated).await?;
        Ok(issued)
    }
}

pub(super) fn collect_secret_references(spec: &MonitorSpec) -> Result<BTreeMap<String, Id>> {
    let mut references = BTreeMap::new();
    let mut add = |value: &ValueSource| -> Result<()> {
        let ValueSource::Secret {
            reference,
            secret_id,
        } = value
        else {
            return Ok(());
        };
        if let Some(existing) = references.get(reference)
            && existing != secret_id
        {
            return Err(Error::invalid(format!(
                "secret reference `{reference}` points to more than one Secret"
            )));
        }
        references.insert(reference.clone(), secret_id.clone());
        Ok(())
    };
    match spec {
        MonitorSpec::Http(spec) => {
            for step in &spec.steps {
                add(&step.url)?;
                if let Some(body) = &step.body {
                    add(body)?;
                }
                for value in step.headers.iter().chain(&step.query) {
                    add(&value.value)?;
                }
            }
        }
        MonitorSpec::Tcp(spec) => {
            add(&spec.host)?;
            if let Some(send) = &spec.send {
                add(send)?;
            }
        }
        MonitorSpec::Grpc(spec) => {
            for value in &spec.metadata {
                add(&value.value)?;
            }
        }
        MonitorSpec::Browser(spec) => {
            for step in &spec.steps {
                match &step.action {
                    BrowserAction::Navigate { url, .. } => add(url)?,
                    BrowserAction::Fill { value, .. } | BrowserAction::Select { value, .. } => {
                        add(value)?
                    }
                    _ => {}
                }
            }
        }
        MonitorSpec::Dns(_)
        | MonitorSpec::Icmp(_)
        | MonitorSpec::Tls(_)
        | MonitorSpec::Heartbeat => {}
    }
    Ok(references)
}

fn validate_register(input: &ProbeRegisterInput) -> Result<()> {
    validate_protocol(input.protocol_version)?;
    if input.register_token.len() < 32 || input.register_token.len() > 256 {
        return Err(Error::unauthorized("invalid Probe register token"));
    }
    if input.hostname.trim().is_empty() || input.hostname.len() > 255 {
        return Err(Error::invalid("invalid Probe hostname"));
    }
    if input.agent_version.trim().is_empty() || input.agent_version.len() > 64 {
        return Err(Error::invalid("invalid Probe Agent version"));
    }
    if input.capabilities.is_empty() {
        return Err(Error::invalid("Probe Agent must advertise capabilities"));
    }
    Ok(())
}

fn validate_protocol(version: u32) -> Result<()> {
    if version != PROBE_PROTOCOL_VERSION {
        return Err(Error::conflict(format!(
            "unsupported Probe protocol {version}; server supports {PROBE_PROTOCOL_VERSION}"
        )));
    }
    Ok(())
}
