// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeRegisterToken {
    pub id: Id,
    pub organization_id: Id,
    pub location_id: Id,
    pub expires_at: TimestampMicros,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeAgentTokenStatus {
    Active,
    Disabled,
}

impl ProbeAgentTokenStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeAgentToken {
    pub id: Id,
    pub organization_id: Id,
    pub location_id: Id,
    pub name: String,
    pub token_prefix: String,
    pub status: ProbeAgentTokenStatus,
    pub expires_at: Option<TimestampMicros>,
    pub last_used_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub rotated_at: Option<TimestampMicros>,
    pub disabled_at: Option<TimestampMicros>,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct ProbeRegistrationGrant {
    pub organization_id: Id,
    pub location_id: Id,
}

#[derive(Clone, Serialize)]
pub struct ProbeAgentTokenInstructions {
    pub token: ProbeAgentToken,
    pub agent_token: String,
    pub register_endpoint: String,
    pub control_endpoint: String,
    pub ca_certificate_pem: String,
    pub ca_sha256: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeRegisterInstructions {
    pub token: ProbeRegisterToken,
    pub register_token: String,
    pub register_endpoint: String,
    pub control_endpoint: String,
    pub ca_certificate_pem: String,
    pub ca_sha256: String,
    pub command: String,
}
