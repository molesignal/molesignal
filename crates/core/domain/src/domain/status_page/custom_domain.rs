// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Custom-domain ownership, routing and TLS activation state.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageDomainState {
    PendingDns,
    Verifying,
    Verified,
    ProvisioningTls,
    Active,
    Failed,
    Degraded,
}

impl StatusPageDomainState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingDns => "pending_dns",
            Self::Verifying => "verifying",
            Self::Verified => "verified",
            Self::ProvisioningTls => "provisioning_tls",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Degraded => "degraded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending_dns" => Some(Self::PendingDns),
            "verifying" => Some(Self::Verifying),
            "verified" => Some(Self::Verified),
            "provisioning_tls" => Some(Self::ProvisioningTls),
            "active" => Some(Self::Active),
            "failed" => Some(Self::Failed),
            "degraded" => Some(Self::Degraded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageDomainConfig {
    pub org_id: Id,
    pub status_page_id: Id,
    pub hostname: String,
    pub verification_token: String,
    pub state: StatusPageDomainState,
    pub domain_id: Option<Id>,
    pub routing_valid: bool,
    pub last_checked_at: Option<TimestampMicros>,
    pub last_error: Option<String>,
    pub cert_not_after: Option<TimestampMicros>,
    pub last_alerted_state: Option<StatusPageDomainState>,
    pub last_alerted_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct StatusPageDomainHealthAlert {
    pub org_id: Id,
    pub status_page_id: Id,
    pub page_name: String,
    pub hostname: String,
    pub state: StatusPageDomainState,
    pub error: Option<String>,
}

#[async_trait]
pub trait StatusPageDomainHealthNotifier: Send + Sync {
    async fn notify(&self, alert: &StatusPageDomainHealthAlert) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct StatusPageDomainCheck {
    pub ownership_verified: bool,
    pub routing_valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatusPageDomainCheckUpdate {
    pub org_id: Id,
    pub status_page_id: Id,
    pub state: StatusPageDomainState,
    pub routing_valid: bool,
    pub last_error: Option<String>,
    pub checked_at: TimestampMicros,
    pub provision_tls: bool,
}

#[async_trait]
pub trait StatusPageDomainVerifier: Send + Sync {
    async fn verify(
        &self,
        hostname: &str,
        verification_token: &str,
    ) -> Result<StatusPageDomainCheck>;

    fn routing_target(&self) -> &str;
}
