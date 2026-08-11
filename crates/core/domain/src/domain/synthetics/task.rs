// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::monitor::MonitorSpec;
use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeTaskState {
    Pending,
    Leased,
    Running,
    Completed,
    Cancelled,
    Expired,
    Skipped,
}

impl ProbeTaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "leased" => Some(Self::Leased),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeTask {
    pub id: Id,
    pub organization_id: Id,
    pub monitor_id: Id,
    pub monitor_revision_id: Id,
    pub location_id: Id,
    pub spec: MonitorSpec,
    pub is_test: bool,
    pub state: ProbeTaskState,
    pub scheduled_at: TimestampMicros,
    pub deadline_at: TimestampMicros,
    pub timeout_millis: u32,
    pub max_attempts: u8,
    pub lease_agent_id: Option<Id>,
    pub lease_token_hash: Option<String>,
    pub leased_until: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
