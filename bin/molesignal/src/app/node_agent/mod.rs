// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node Agent control plane (managed mode): enrollment and identities, sessions, task leases,
//! inventory management, quotas and audit.
//!
//! The protocol is molesignal-node-agent's `docs/control-protocol.md`; the wire is
//! `proto/molesignal/node/control/v1/node_control.proto`, served by `api::grpc::node_control`.
//! Everything here is transport-neutral: the gRPC layer turns messages into calls and
//! [`Outbound`] values back into messages.

mod audit;
mod enroll;
mod identity;
mod manage;
mod session;
mod tasks;

#[cfg(test)]
pub(crate) mod memory;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use domain::{iam::IamContext, node_agent::NodeAgentRepository};
use postgres::repositories::audit_events::AuditEventRepository;
use settings::NodeAgentControlSettings;

use self::audit::Audit;
pub use self::{
    enroll::{EnrollInput, EnrollOutcome, RotateOutcome},
    identity::{NodeAgentAuthority, PeerIdentity, peer_identity},
    manage::{CreateEnrollmentTokenInput, EnrollmentTokenIssued, NodeView},
    session::{Authenticated, HeartbeatInput, HelloInput, Outbound, Session, TaskResultInput},
    tasks::CreateTaskInput,
};

/// Who did something: a member or token (`IamContext`), a node, or an enrollment token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub kind: String,
    pub id: String,
}

impl Actor {
    pub fn from_context(context: &IamContext) -> Self {
        Self {
            kind: context.principal_type().as_str().into(),
            id: context.principal_id().0.clone(),
        }
    }

    pub fn node(node_id: &str) -> Self {
        Self {
            kind: "node_agent".into(),
            id: node_id.into(),
        }
    }
}

/// Protocol timings, from `[node_agent]`.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    pub heartbeat_interval_secs: u32,
    pub lease_ttl_micros: i64,
    pub offer_ttl_micros: i64,
}

impl Timings {
    pub fn from_settings(settings: &NodeAgentControlSettings) -> Self {
        Self {
            heartbeat_interval_secs: settings.heartbeat_interval_secs,
            lease_ttl_micros: i64::from(settings.lease_ttl_secs) * 1_000_000,
            offer_ttl_micros: i64::from(settings.offer_ttl_secs) * 1_000_000,
        }
    }

    /// A node is offline after three heartbeat intervals without one.
    pub fn offline_after_micros(&self) -> i64 {
        i64::from(self.heartbeat_interval_secs) * 3 * 1_000_000
    }
}

pub struct NodeAgentService {
    repository: Arc<dyn NodeAgentRepository>,
    authority: Arc<NodeAgentAuthority>,
    audit: Audit,
    timings: Timings,
    endpoint: String,
}

impl NodeAgentService {
    pub fn new(
        repository: Arc<dyn NodeAgentRepository>,
        authority: Arc<NodeAgentAuthority>,
        audit_events: Arc<dyn AuditEventRepository>,
        timings: Timings,
        endpoint: String,
    ) -> Self {
        Self {
            repository,
            authority,
            audit: Audit::new(audit_events),
            timings,
            endpoint,
        }
    }

    pub fn authority(&self) -> &NodeAgentAuthority {
        &self.authority
    }

    pub fn timings(&self) -> Timings {
        self.timings
    }
}
