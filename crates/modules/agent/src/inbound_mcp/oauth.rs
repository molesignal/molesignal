// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpOAuthClient {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub scope: String,
    #[serde(skip_serializing)]
    pub client_secret_hash: Option<String>,
    pub client_id_issued_at: i64,
    pub client_secret_expires_at: Option<i64>,
    pub client_uri: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct NewInboundMcpOAuthClient {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub scope: String,
    pub client_secret_hash: Option<String>,
    pub client_id_issued_at: i64,
    pub client_secret_expires_at: Option<i64>,
    pub client_uri: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InboundMcpAuthorizationCode {
    pub code_hash: String,
    pub client_id: String,
    pub org_id: Id,
    pub user_id: Id,
    pub redirect_uri: String,
    pub scope: String,
    pub resource: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundMcpOAuthTokenKind {
    Access,
    Refresh,
}

impl InboundMcpOAuthTokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpOAuthToken {
    pub id: Id,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub token_kind: InboundMcpOAuthTokenKind,
    pub family_id: Id,
    pub client_id: String,
    pub org_id: Id,
    pub user_id: Id,
    pub scope: String,
    pub resource: String,
    pub expires_at: TimestampMicros,
    pub revoked_at: Option<TimestampMicros>,
    pub rotated_from: Option<Id>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpOAuthConnection {
    pub family_id: Id,
    pub client_id: String,
    pub client_name: String,
    pub user_id: Id,
    pub user_display_name: String,
    pub scope: String,
    pub resource: String,
    pub created_at: TimestampMicros,
    pub last_expires_at: TimestampMicros,
    pub active: bool,
}
