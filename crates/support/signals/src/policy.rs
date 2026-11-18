// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use crate::shared::tail_sampling::TraceRuntimePolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTracePolicy {
    pub id: Id,
    pub system_org_id: Id,
    pub policy: TraceRuntimePolicy,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceDebugToken {
    pub id: Id,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub organization_id: Option<Id>,
    pub route_pattern: Option<String>,
    pub expires_at: TimestampMicros,
    pub max_uses: u64,
    pub used_count: u64,
    pub revoked_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait TracePolicyRepository: Send + Sync {
    async fn active(&self) -> Result<Option<PersistedTracePolicy>>;
    async fn history(&self) -> Result<Vec<PersistedTracePolicy>>;
    /// Allocates a monotonic version, inserts immutable history, and switches the active pointer
    /// in a single transaction.
    async fn publish(
        &self,
        system_org_id: &Id,
        policy: TraceRuntimePolicy,
        actor_id: &Id,
    ) -> Result<PersistedTracePolicy>;
}

#[async_trait]
pub trait TraceDebugTokenRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<TraceDebugToken>>;
    async fn create(&self, token: TraceDebugToken) -> Result<TraceDebugToken>;
    async fn revoke(&self, id: &Id, revoked_at: TimestampMicros) -> Result<()>;
    /// Atomically consumes one use of the usage quota; returns None on expiry, revocation, scope
    /// mismatch, or exhaustion.
    async fn consume(
        &self,
        token_hash: &str,
        organization_id: Option<&Id>,
        route: Option<&str>,
        now: TimestampMicros,
    ) -> Result<Option<TraceDebugToken>>;
}
