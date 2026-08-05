// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiTokenKind {
    Personal,
    DefaultIngestion,
    RumClient,
}

impl ApiTokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::DefaultIngestion => "default_ingestion",
            Self::RumClient => "rum_client",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "personal" => Some(Self::Personal),
            "default_ingestion" => Some(Self::DefaultIngestion),
            "rum_client" => Some(Self::RumClient),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Id,
    pub prefix: String,
    #[serde(skip_serializing)]
    pub secret_hash: String,
    pub org_id: Id,
    pub user_id: Id,
    pub role_id: Id,
    pub name: String,
    pub expires_at: Option<TimestampMicros>,
    pub last_used_at: Option<TimestampMicros>,
    pub revoked: bool,
    pub created_at: TimestampMicros,
    #[serde(default)]
    pub is_default: bool,
    pub token_kind: ApiTokenKind,
    pub application_id: Option<String>,
}

/// A managed token whose plaintext is sealed at rest and can be shown again.
#[derive(Debug, Clone)]
pub struct ManagedApiToken {
    pub id: Id,
    pub prefix: String,
    pub token: String,
    pub role_id: Id,
    pub token_kind: ApiTokenKind,
    pub application_id: Option<String>,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait ApiTokenRepository: Send + Sync {
    async fn create(&self, token: ApiToken) -> Result<ApiToken>;
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiToken>>;
    async fn list_by_org(&self, org_id: &Id) -> Result<Vec<ApiToken>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ApiToken>;
    async fn mark_revoked(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn touch_last_used(&self, prefix: &str, at: TimestampMicros) -> Result<()>;

    async fn ensure_default(
        &self,
        org_id: &Id,
        user_id: &Id,
        role_id: &Id,
    ) -> Result<ManagedApiToken>;

    async fn ensure_rum_client(
        &self,
        org_id: &Id,
        user_id: &Id,
        role_id: &Id,
        application_id: &str,
    ) -> Result<ManagedApiToken>;
}
