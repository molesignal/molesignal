// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::iam::api_token::ApiToken,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// A non-human organization principal with no password, session, or JWT sign-in path. An
/// independently managed API Token may authenticate as this principal and inherits its current
/// role. Provisioning atomically creates the first bound Token; the Token never owns Service
/// Account lifecycle or permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub role_id: Id,
    pub disabled: bool,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ServiceAccountRepository: Send + Sync {
    /// Atomically creates the non-human principal and its initial bound API Token. Keeping this as
    /// the only creation primitive prevents adapters from leaving an unusable account behind when
    /// credential persistence fails.
    async fn provision(
        &self,
        account: ServiceAccount,
        initial_token: ApiToken,
    ) -> Result<(ServiceAccount, ApiToken)>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ServiceAccount>;
    async fn list(&self, org_id: &Id) -> Result<Vec<ServiceAccount>>;
    /// Updates account metadata and synchronizes the effective role of all active API Tokens bound
    /// to this principal in the same transaction.
    async fn update(&self, account: ServiceAccount) -> Result<ServiceAccount>;
    /// Disabling also revokes all bound active API Tokens atomically. Enabling never revives them.
    async fn set_disabled(
        &self,
        org_id: &Id,
        id: &Id,
        disabled: bool,
        updated_at: TimestampMicros,
    ) -> Result<ServiceAccount>;
    /// Soft deletion preserves audit history and atomically revokes every bound active API Token.
    async fn delete(&self, org_id: &Id, id: &Id, deleted_at: TimestampMicros) -> Result<()>;
}
