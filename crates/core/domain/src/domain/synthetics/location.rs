// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationScope {
    Platform,
    Organization,
}

impl LocationScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Organization => "organization",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "platform" => Some(Self::Platform),
            "organization" => Some(Self::Organization),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationExecution {
    Embedded,
    AgentPool,
}

impl LocationExecution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::AgentPool => "agent_pool",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "embedded" => Some(Self::Embedded),
            "agent_pool" => Some(Self::AgentPool),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationLifecycle {
    Active,
    Paused,
    Archived,
}

impl LocationLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationHealth {
    Online,
    Degraded,
    Offline,
    Unknown,
}

impl LocationHealth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Degraded => "degraded",
            Self::Offline => "offline",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "online" => Some(Self::Online),
            "degraded" => Some(Self::Degraded),
            "offline" => Some(Self::Offline),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EgressPolicy {
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub denied_cidrs: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub denied_domains: Vec<String>,
    #[serde(default)]
    pub allowed_ports: Vec<u16>,
    #[serde(default)]
    pub allow_private_networks: bool,
    #[serde(default)]
    pub allow_loopback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeLocation {
    pub id: Id,
    pub organization_id: Option<Id>,
    pub name: String,
    pub code: String,
    pub description: String,
    pub scope: LocationScope,
    pub execution: LocationExecution,
    pub lifecycle: LocationLifecycle,
    pub health: LocationHealth,
    pub system_managed: bool,
    pub egress_policy: EgressPolicy,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCapability {
    Http,
    Tcp,
    Dns,
    Icmp,
    Tls,
    Grpc,
    Browser,
}

impl ProbeCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Dns => "dns",
            Self::Icmp => "icmp",
            Self::Tls => "tls",
            Self::Grpc => "grpc",
            Self::Browser => "browser",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "http" => Some(Self::Http),
            "tcp" => Some(Self::Tcp),
            "dns" => Some(Self::Dns),
            "icmp" => Some(Self::Icmp),
            "tls" => Some(Self::Tls),
            "grpc" => Some(Self::Grpc),
            "browser" => Some(Self::Browser),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Registered,
    Online,
    Degraded,
    Offline,
    Draining,
    Revoked,
}

impl AgentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Online => "online",
            Self::Degraded => "degraded",
            Self::Offline => "offline",
            Self::Draining => "draining",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "registered" => Some(Self::Registered),
            "online" => Some(Self::Online),
            "degraded" => Some(Self::Degraded),
            "offline" => Some(Self::Offline),
            "draining" => Some(Self::Draining),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentCapacity {
    pub max_concurrent: u32,
    pub max_browser_concurrent: u32,
    pub available: u32,
    pub available_browser: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeAgent {
    pub id: Id,
    pub organization_id: Option<Id>,
    pub location_id: Id,
    pub name: String,
    pub hostname: String,
    pub status: AgentStatus,
    pub agent_version: String,
    pub protocol_version: u32,
    pub capabilities: Vec<ProbeCapability>,
    pub capacity: AgentCapacity,
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing)]
    pub public_key_der: Vec<u8>,
    pub certificate_serial: Option<String>,
    pub certificate_expires_at: Option<TimestampMicros>,
    pub last_heartbeat_at: Option<TimestampMicros>,
    pub last_result_sequence: u64,
    pub revoked_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
