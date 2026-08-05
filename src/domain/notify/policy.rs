// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::preference::NotifyCategory;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyDeliveryMode {
    #[default]
    PreferUser,
    ForceConnector,
    MultiConnector,
}

impl NotifyDeliveryMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreferUser => "prefer_user",
            Self::ForceConnector => "force_connector",
            Self::MultiConnector => "multi_connector",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "prefer_user" => Ok(Self::PreferUser),
            "force_connector" => Ok(Self::ForceConnector),
            "multi_connector" => Ok(Self::MultiConnector),
            other => Err(Error::internal(format!(
                "unknown notify delivery mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyFallbackConfig {
    #[serde(default = "default_true")]
    pub use_user_fallbacks: bool,
    #[serde(default = "default_true")]
    pub use_team_defaults: bool,
    #[serde(default = "default_true")]
    pub use_organization_defaults: bool,
}

/// `force_connector` 与 `multi_connector` 按接收人的已绑定端点筛选这些企业连接器。
/// 固定群组/地址仍由团队或公司默认路由承载，避免同一群组按接收人数重复发送。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyDeliveryConfig {
    #[serde(default)]
    pub connector_ids: Vec<Id>,
}

impl Default for NotifyFallbackConfig {
    fn default() -> Self {
        Self {
            use_user_fallbacks: true,
            use_team_defaults: true,
            use_organization_defaults: true,
        }
    }
}

const fn default_true() -> bool {
    true
}

/// 将业务事件映射到接收人和投递路由的组织级策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyPolicy {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub event_type: String,
    pub category: NotifyCategory,
    #[serde(default)]
    pub matchers: Value,
    pub recipient_resolver: String,
    #[serde(default)]
    pub resolver_config: Value,
    pub delivery_mode: NotifyDeliveryMode,
    #[serde(default)]
    pub delivery_config: NotifyDeliveryConfig,
    pub template_id: Option<Id>,
    #[serde(default)]
    pub fallback_config: NotifyFallbackConfig,
    pub ack_timeout_seconds: Option<i32>,
    pub escalation_config: Option<Value>,
    pub enabled: bool,
    pub priority: i32,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 通知引擎的品牌无关输入。事件来源只负责提供稳定 ID、类型、发生时间和属性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyEvent {
    pub id: String,
    pub event_type: String,
    pub organization_id: Id,
    pub occurred_at: TimestampMicros,
    #[serde(default)]
    pub attributes: Value,
}
