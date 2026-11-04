// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// How every credential starts, except the JWTs of a sign-in session: API tokens of every kind,
/// alert source, probe, automation, trace debug and OAuth credentials. What one may do is in its
/// own store, never in how it looks.
pub const TOKEN_MARKER: &str = "mspat-";

/// The one permission of a RUM client token.
pub const RUM_CLIENT_PERMISSION: &str = "rum.write";

/// Whether a credential is one of those rather than a session's JWT.
pub fn has_token_marker(credential: &str) -> bool {
    credential.starts_with(TOKEN_MARKER)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiTokenKind {
    Personal,
    DefaultIntake,
    RumClient,
    ServiceAccount,
}

impl ApiTokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::DefaultIntake => "default_intake",
            Self::RumClient => "rum_client",
            Self::ServiceAccount => "service_account",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "personal" => Some(Self::Personal),
            "default_intake" => Some(Self::DefaultIntake),
            "rum_client" => Some(Self::RumClient),
            "service_account" => Some(Self::ServiceAccount),
            _ => None,
        }
    }
}

/// What an API token may do. A token never does more than its principal may do at the time
/// of a request: the scope only narrows the principal's access. A RUM client token has no
/// principal and holds exactly its scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ApiTokenScope {
    /// Whatever its principal may do.
    Principal,
    /// A permission policy; the token follows the policy as it changes.
    Policy { policy_id: Id },
    /// Permissions fixed when the token was issued.
    Permissions { permissions: Vec<String> },
}

impl ApiTokenScope {
    /// The scope stored as a policy id or a permission list, neither for `Principal`.
    pub fn from_columns(policy_id: Option<String>, permissions: Option<Vec<String>>) -> Self {
        match (policy_id, permissions) {
            (Some(policy_id), _) => Self::Policy {
                policy_id: Id(policy_id),
            },
            (None, Some(permissions)) => Self::Permissions { permissions },
            (None, None) => Self::Principal,
        }
    }

    pub fn policy_id(&self) -> Option<&Id> {
        match self {
            Self::Policy { policy_id } => Some(policy_id),
            _ => None,
        }
    }

    pub fn permissions(&self) -> Option<&[String]> {
        match self {
            Self::Permissions { permissions } => Some(permissions),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiToken {
    pub id: Id,
    pub prefix: String,
    #[serde(skip_serializing)]
    pub secret_hash: String,
    pub org_id: Id,
    /// The user who issued the token. A personal token acts as this user.
    pub user_id: Id,
    pub scope: ApiTokenScope,
    pub name: String,
    pub expires_at: Option<TimestampMicros>,
    pub last_used_at: Option<TimestampMicros>,
    pub revoked: bool,
    pub created_at: TimestampMicros,
    #[serde(default)]
    pub is_default: bool,
    pub token_kind: ApiTokenKind,
    pub application_id: Option<String>,
    pub service_account_id: Option<Id>,
}

/// A managed token whose plaintext is sealed at rest and can be shown again.
#[derive(Debug, Clone)]
pub struct ManagedApiToken {
    pub id: Id,
    pub prefix: String,
    pub token: String,
    pub token_kind: ApiTokenKind,
    pub application_id: Option<String>,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait ApiTokenRepository: Send + Sync {
    async fn create(&self, token: ApiToken) -> Result<ApiToken>;
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiToken>>;
    async fn list_by_org(&self, org_id: &Id) -> Result<Vec<ApiToken>>;
    /// The personal and default intake tokens a user issued in an organization.
    async fn list_personal(&self, org_id: &Id, user_id: &Id) -> Result<Vec<ApiToken>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ApiToken>;
    async fn mark_revoked(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn touch_last_used(&self, prefix: &str, at: TimestampMicros) -> Result<()>;
    async fn ensure_default(&self, org_id: &Id, user_id: &Id) -> Result<ManagedApiToken>;

    async fn ensure_rum_client(
        &self,
        org_id: &Id,
        user_id: &Id,
        application_id: &str,
    ) -> Result<ManagedApiToken>;
}
