// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{ids::Id, time::TimestampMicros};

/// 用户在某个企业连接器中的可投递身份。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNotifyEndpoint {
    pub id: Id,
    pub organization_id: Id,
    pub user_id: Id,
    pub connector_id: Id,
    pub provider_type: String,
    pub external_identity: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    pub verified: bool,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
