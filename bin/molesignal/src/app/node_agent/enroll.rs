// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Enrollment (token + CSR → identity), certificate authentication and rotation (§4, §8).

use domain::node_agent::{Capabilities, NodeAgent, ProtocolVersion};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::{Actor, NodeAgentService, PeerIdentity, peer_identity, session::Authenticated};

#[derive(Debug, Clone)]
pub struct EnrollInput {
    pub enrollment_token: String,
    pub csr_pem: String,
    pub host_id: String,
    pub boot_id: String,
    pub hostname: String,
    pub agent_version: String,
    pub protocol_version: String,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone)]
pub struct EnrollOutcome {
    pub node: NodeAgent,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub not_after: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct RotateOutcome {
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub not_after: TimestampMicros,
}

/// The error a client of another major version gets.
pub(super) fn unsupported_protocol(agent: &str) -> Error {
    Error::conflict(format!(
        "UNSUPPORTED_PROTOCOL: the agent speaks {agent}, the control plane speaks {}",
        ProtocolVersion::CURRENT
    ))
}

pub(super) fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn bounded(name: &str, value: &str, min: usize, max: usize) -> Result<()> {
    if value.trim().len() < min || value.len() > max {
        return Err(Error::invalid(format!(
            "`{name}` must be {min} to {max} characters"
        )));
    }

    Ok(())
}

impl NodeAgentService {
    pub async fn enroll(&self, input: EnrollInput) -> Result<EnrollOutcome> {
        if input.enrollment_token.len() < 32 || input.enrollment_token.len() > 256 {
            return Err(Error::unauthorized("invalid or expired enrollment token"));
        }

        bounded("host_id", &input.host_id, 1, 255)?;
        bounded("boot_id", &input.boot_id, 0, 255)?;
        bounded("hostname", &input.hostname, 0, 255)?;
        bounded("agent_version", &input.agent_version, 1, 64)?;

        let version = ProtocolVersion::parse(&input.protocol_version)?;

        if !version.compatible_with(ProtocolVersion::CURRENT) {
            return Err(unsupported_protocol(&input.protocol_version));
        }

        let now = TimestampMicros::now();
        let hash = token_hash(&input.enrollment_token);

        let token = self
            .repository
            .enrollment_token_by_hash(&hash)
            .await?
            .ok_or_else(|| Error::unauthorized("invalid or expired enrollment token"))?;

        token.check_usable(now)?;

        let existing = self
            .repository
            .node_by_host(&token.org_id, input.host_id.trim())
            .await?;

        let node_id = existing
            .as_ref()
            .map_or_else(Id::new, |node| node.id.clone());

        let issued = self
            .authority
            .issue_node(&token.org_id, &node_id, &input.csr_pem)?;

        let node = NodeAgent {
            id: node_id.clone(),
            org_id: token.org_id.clone(),
            host_id: input.host_id.trim().to_string(),
            boot_id: input.boot_id,
            hostname: input.hostname,
            labels: token.labels.clone(),
            agent_version: input.agent_version,
            protocol_version: input.protocol_version,
            capabilities: input.capabilities,
            certificate_serial: issued.serial,
            previous_certificate_serial: None,
            certificate_expires_at: issued.expires_at,
            enrollment_token_id: Some(token.id.clone()),
            health: Default::default(),
            collector_states: Vec::new(),
            applied_config_revision: 0,
            desired_config: None,
            config_status: None,
            last_heartbeat_at: None,
            last_connected_at: None,
            drain_requested_at: None,
            drain_deadline: None,
            drained_at: None,
            revoked_at: None,
            revoke_reason: None,
            enrolled_at: now,
            created_at: now,
            updated_at: now,
        };

        let stored = self.repository.enroll_node(&hash, &node, now).await?;

        // Two first enrollments of one host at once: the certificate names the losing id.
        if stored.id != node_id {
            return Err(Error::conflict(
                "the host enrolled concurrently; enroll again",
            ));
        }

        let action = if existing.is_some() {
            "node_agent.reenroll"
        } else {
            "node_agent.enroll"
        };

        self.audit
            .record(
                &stored.org_id,
                &Actor {
                    kind: "enrollment_token".into(),
                    id: token.id.0.clone(),
                },
                action,
                ("node_agent", stored.id.as_str()),
                json!({
                    "hostId": stored.host_id,
                    "hostname": stored.hostname,
                    "agentVersion": stored.agent_version,
                    "certificateSerial": stored.certificate_serial,
                    "wasRevoked": existing.as_ref().is_some_and(NodeAgent::is_revoked),
                }),
            )
            .await;

        Ok(EnrollOutcome {
            node: stored,
            certificate_pem: issued.certificate_pem,
            ca_bundle_pem: self.authority.ca_certificate_pem().to_string(),
            not_after: issued.expires_at,
        })
    }

    /// The node a verified client certificate identifies. A revoked node is returned too, so its
    /// session can be told; a serial that is neither the node's current certificate nor the one
    /// its last rotation replaced is refused.
    pub async fn authenticate(&self, certificate_der: &[u8]) -> Result<Authenticated> {
        let PeerIdentity {
            serial,
            node_id,
            org_id,
        } = peer_identity(certificate_der)?;

        let node = self
            .repository
            .node_by_certificate_serial(&serial)
            .await?
            .filter(|node| node.accepts_certificate(&serial))
            .ok_or_else(|| Error::unauthorized("unknown or replaced node certificate"))?;

        // The certificate's organization binding decides isolation.
        if node.id.as_str() != node_id || node.org_id.as_str() != org_id {
            return Err(Error::unauthorized(
                "the certificate does not match the node it was issued to",
            ));
        }

        let used_current = node.certificate_serial == serial;
        Ok(Authenticated { node, used_current })
    }

    pub async fn rotate_certificate(
        &self,
        certificate_der: &[u8],
        csr_pem: &str,
    ) -> Result<RotateOutcome> {
        let Authenticated { node, .. } = self.authenticate(certificate_der).await?;

        if node.is_revoked() {
            return Err(Error::unauthorized("the node is revoked"));
        }

        let now = TimestampMicros::now();
        let issued = self.authority.issue_node(&node.org_id, &node.id, csr_pem)?;

        self.repository
            .rotate_certificate(
                &node.org_id,
                &node.id,
                &issued.serial,
                issued.expires_at,
                now,
            )
            .await?;

        self.audit
            .record(
                &node.org_id,
                &Actor::node(node.id.as_str()),
                "node_agent.certificate.rotate",
                ("node_agent", node.id.as_str()),
                json!({
                    "certificateSerial": issued.serial,
                    "previousCertificateSerial": node.certificate_serial,
                    "notAfter": issued.expires_at,
                }),
            )
            .await;

        Ok(RotateOutcome {
            certificate_pem: issued.certificate_pem,
            ca_bundle_pem: self.authority.ca_certificate_pem().to_string(),
            not_after: issued.expires_at,
        })
    }
}
