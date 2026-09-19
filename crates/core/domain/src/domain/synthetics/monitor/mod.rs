// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod assertion;
mod browser;
mod http;
mod network;

pub use assertion::{
    AssertionOperator, AssertionSeverity, Extraction, MonitorAssertion, ValueSource,
};
pub use browser::{BrowserAction, BrowserJourneySpec, BrowserStep, Viewport};
pub use http::{HeaderValue, HttpJourneySpec, HttpStep};
pub use network::{
    DnsSpec, GrpcCall, GrpcSpec, IcmpSpec, SshAuthentication, SshSpec, TcpSpec, TlsSpec,
};
use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorKind {
    Http,
    Tcp,
    Ssh,
    Dns,
    Icmp,
    Tls,
    Grpc,
    Browser,
    Heartbeat,
}

impl MonitorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Ssh => "ssh",
            Self::Dns => "dns",
            Self::Icmp => "icmp",
            Self::Tls => "tls",
            Self::Grpc => "grpc",
            Self::Browser => "browser",
            Self::Heartbeat => "heartbeat",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "http" => Some(Self::Http),
            "tcp" => Some(Self::Tcp),
            "ssh" => Some(Self::Ssh),
            "dns" => Some(Self::Dns),
            "icmp" => Some(Self::Icmp),
            "tls" => Some(Self::Tls),
            "grpc" => Some(Self::Grpc),
            "browser" => Some(Self::Browser),
            "heartbeat" => Some(Self::Heartbeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorLifecycle {
    Draft,
    Active,
    Paused,
    Archived,
}

impl MonitorLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorState {
    Healthy,
    Flaky,
    Degraded,
    Failing,
    Unknown,
}

impl MonitorState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Flaky => "flaky",
            Self::Degraded => "degraded",
            Self::Failing => "failing",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "healthy" => Some(Self::Healthy),
            "flaky" => Some(Self::Flaky),
            "degraded" => Some(Self::Degraded),
            "failing" => Some(Self::Failing),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorSchedule {
    Interval {
        every_seconds: u32,
    },
    Cron {
        expression: String,
        timezone: String,
    },
    Heartbeat {
        expected_seconds: u32,
        grace_seconds: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MultiLocationPolicy {
    Any,
    Quorum { required: u32 },
    Majority,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "configuration", rename_all = "snake_case")]
pub enum MonitorSpec {
    Http(HttpJourneySpec),
    Tcp(TcpSpec),
    Ssh(SshSpec),
    Dns(DnsSpec),
    Icmp(IcmpSpec),
    Tls(TlsSpec),
    Grpc(GrpcSpec),
    Browser(BrowserJourneySpec),
    Heartbeat,
}

impl MonitorSpec {
    pub const fn kind(&self) -> MonitorKind {
        match self {
            Self::Http(_) => MonitorKind::Http,
            Self::Tcp(_) => MonitorKind::Tcp,
            Self::Ssh(_) => MonitorKind::Ssh,
            Self::Dns(_) => MonitorKind::Dns,
            Self::Icmp(_) => MonitorKind::Icmp,
            Self::Tls(_) => MonitorKind::Tls,
            Self::Grpc(_) => MonitorKind::Grpc,
            Self::Browser(_) => MonitorKind::Browser,
            Self::Heartbeat => MonitorKind::Heartbeat,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticMonitor {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub description: String,
    pub kind: MonitorKind,
    pub lifecycle: MonitorLifecycle,
    pub state: MonitorState,
    pub team_id: Option<Id>,
    pub tags: Vec<String>,
    pub active_revision_id: Option<Id>,
    pub draft_revision_id: Option<Id>,
    pub next_due_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub archived_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorRevision {
    pub id: Id,
    pub organization_id: Id,
    pub monitor_id: Id,
    pub number: u32,
    pub spec: MonitorSpec,
    pub schedule: MonitorSchedule,
    pub timeout_millis: u32,
    pub max_retries: u8,
    pub consecutive_failures: u32,
    pub consecutive_recoveries: u32,
    pub freshness_seconds: u32,
    pub location_policy: MultiLocationPolicy,
    pub location_ids: Vec<Id>,
    pub escalation_policy_id: Option<Id>,
    pub alert_on_degraded: bool,
    pub alert_on_flaky: bool,
    pub last_test_result_id: Option<Id>,
    pub last_test_passed_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveMonitorRevision {
    pub monitor: SyntheticMonitor,
    pub revision: MonitorRevision,
}
