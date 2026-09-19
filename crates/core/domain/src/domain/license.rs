// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseVersion {
    pub id: Id,
    pub system_org_id: Id,
    /// 原始签名包；不可编辑、不可删除、不得写入 audit/log/trace。
    pub signed_package: Value,
    pub payload_digest: String,
    /// 仅含 edition、expiry、feature 数等非敏感摘要。
    pub summary: Value,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLicenseVersion {
    pub version: LicenseVersion,
    pub activated_by: Option<Id>,
    pub activated_at: TimestampMicros,
}

#[async_trait]
pub trait LicenseVersionRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<LicenseVersion>>;
    async fn get(&self, id: &Id) -> Result<LicenseVersion>;
    async fn active(&self) -> Result<Option<ActiveLicenseVersion>>;
    async fn insert_and_activate(
        &self,
        version: LicenseVersion,
        actor_id: Option<&Id>,
    ) -> Result<ActiveLicenseVersion>;
    async fn activate(&self, id: &Id, actor_id: &Id) -> Result<ActiveLicenseVersion>;
}
