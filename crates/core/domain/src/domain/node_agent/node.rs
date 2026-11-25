// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, fmt};

use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::task::TaskKind;

/// `<major>.<minor>`: majors must match, minors only add.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
}

impl ProtocolVersion {
    /// The version this control plane speaks.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    pub fn parse(value: &str) -> Result<Self> {
        let invalid = || Error::invalid(format!("invalid protocol version `{value}`"));
        let (major, minor) = value.trim().split_once('.').ok_or_else(invalid)?;

        Ok(Self {
            major: major.parse().map_err(|_| invalid())?,
            minor: minor.parse().map_err(|_| invalid())?,
        })
    }

    pub fn compatible_with(self, other: Self) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What an agent declared it can run. Task kinds it names that this side does not know are dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub task_kinds: Vec<TaskKind>,
    pub features: Vec<String>,
    pub max_running_tasks: u32,
}

impl Capabilities {
    pub fn from_declared(kinds: &[String], features: Vec<String>, max_running_tasks: u32) -> Self {
        let mut task_kinds = kinds
            .iter()
            .filter_map(|kind| TaskKind::parse(kind))
            .collect::<Vec<_>>();

        task_kinds.sort();
        task_kinds.dedup();

        Self {
            task_kinds,
            features: features.into_iter().take(64).collect(),
            max_running_tasks,
        }
    }

    pub fn supports(&self, kind: TaskKind) -> bool {
        self.task_kinds.contains(&kind)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    #[default]
    Unknown,
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub state: HealthState,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorState {
    pub name: String,
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigState {
    Applied,
    Rejected,
    RolledBack,
}

/// What the agent last said about a configuration revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
    pub revision: u64,
    pub state: ConfigState,
    pub reason: Option<String>,
    pub reported_at: TimestampMicros,
}

/// The configuration the control plane wants on a node. The revision only grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesiredConfig {
    pub revision: u64,
    pub config: Value,
    pub updated_by: Option<String>,
    pub updated_at: TimestampMicros,
}

/// What a `Hello` or a heartbeat tells about a node.
#[derive(Debug, Clone, Default)]
pub struct SessionReport {
    /// Hello only: the version and capabilities of the agent.
    pub agent_version: Option<String>,
    pub protocol_version: Option<String>,
    pub capabilities: Option<Capabilities>,
    pub applied_config_revision: u64,
    pub health: Option<Health>,
    pub collector_states: Option<Vec<CollectorState>>,
    /// A new session (`Hello`).
    pub connected: bool,
    /// The session authenticated with the current certificate: the one the last rotation replaced
    /// is no longer needed.
    pub certificate_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Enrolled, never connected.
    Enrolled,
    Online,
    Offline,
    Draining,
    Drained,
    Revoked,
}

/// A node in the inventory. `(org_id, host_id)` is unique: enrolling a host again keeps the node.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAgent {
    pub id: Id,
    pub org_id: Id,
    pub host_id: String,
    pub boot_id: String,
    pub hostname: String,
    pub labels: BTreeMap<String, String>,
    pub agent_version: String,
    pub protocol_version: String,
    pub capabilities: Capabilities,
    pub certificate_serial: String,
    /// The certificate a rotation replaced: still accepted until a session uses the new one, so an
    /// agent that failed to store the new certificate is not locked out.
    #[serde(skip)]
    pub previous_certificate_serial: Option<String>,
    pub certificate_expires_at: TimestampMicros,
    pub enrollment_token_id: Option<Id>,
    pub health: Health,
    pub collector_states: Vec<CollectorState>,
    pub applied_config_revision: u64,
    pub desired_config: Option<DesiredConfig>,
    pub config_status: Option<ConfigStatus>,
    pub last_heartbeat_at: Option<TimestampMicros>,
    pub last_connected_at: Option<TimestampMicros>,
    pub drain_requested_at: Option<TimestampMicros>,
    pub drain_deadline: Option<TimestampMicros>,
    pub drained_at: Option<TimestampMicros>,
    pub revoked_at: Option<TimestampMicros>,
    pub revoke_reason: Option<String>,
    pub enrolled_at: TimestampMicros,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl NodeAgent {
    pub fn desired_config_revision(&self) -> u64 {
        self.desired_config
            .as_ref()
            .map_or(0, |desired| desired.revision)
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    pub fn is_draining(&self) -> bool {
        self.drain_requested_at.is_some()
    }

    /// Whether `serial` identifies this node now.
    pub fn accepts_certificate(&self, serial: &str) -> bool {
        self.certificate_serial == serial
            || self.previous_certificate_serial.as_deref() == Some(serial)
    }

    /// Online while a heartbeat arrived within `offline_after`.
    pub fn status(&self, now: TimestampMicros, offline_after_micros: i64) -> NodeStatus {
        if self.is_revoked() {
            return NodeStatus::Revoked;
        }

        if self.drained_at.is_some() {
            return NodeStatus::Drained;
        }

        let online = self
            .last_heartbeat_at
            .is_some_and(|at| now.0.saturating_sub(at.0) <= offline_after_micros);

        match (self.is_draining(), online, self.last_heartbeat_at) {
            (true, _, _) => NodeStatus::Draining,
            (false, true, _) => NodeStatus::Online,
            (false, false, None) => NodeStatus::Enrolled,
            (false, false, Some(_)) => NodeStatus::Offline,
        }
    }
}
