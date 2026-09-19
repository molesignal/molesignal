// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, tail_sampling::TraceRuntimePolicy, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 在一个事务中分配单调版本、插入不可变历史并切换 active pointer。
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
    /// 原子消费一次使用额度；过期、撤销、范围不匹配或已耗尽均返回 None。
    async fn consume(
        &self,
        token_hash: &str,
        organization_id: Option<&Id>,
        route: Option<&str>,
        now: TimestampMicros,
    ) -> Result<Option<TraceDebugToken>>;
}
