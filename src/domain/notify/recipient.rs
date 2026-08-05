// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::policy::NotifyEvent;
use crate::shared::{Result, ids::Id};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyRecipient {
    pub user_id: Id,
    pub team_id: Option<Id>,
}

/// 接收人来源的扩展点。值班、团队或未来资产负责人等差异只能存在于 resolver 内。
#[async_trait]
pub trait RecipientResolver: Send + Sync {
    fn resolver_type(&self) -> &'static str;
    fn validate_config(&self, config: &Value) -> Result<()>;

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>>;
}
