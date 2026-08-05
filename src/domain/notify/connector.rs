// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorStatus {
    #[default]
    Unknown,
    Connected,
    Error,
}

impl ConnectorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "connected" => Ok(Self::Connected),
            "error" => Ok(Self::Error),
            other => Err(crate::shared::Error::internal(format!(
                "unknown notify connector status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorTestStatus {
    #[default]
    Success,
    Failed,
}

impl ConnectorTestStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            other => Err(crate::shared::Error::internal(format!(
                "unknown notify connector test status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorCapabilities {
    pub direct_user: bool,
    pub group: bool,
    pub rich_text: bool,
    pub interactive: bool,
    pub acknowledgement: bool,
    pub attachments: bool,
}

/// 企业级发送连接器。`config` 是运行时解密后的配置，只允许在受控 service /
/// adapter 路径中使用，序列化时永不输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConnector {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub connector_type: String,
    #[serde(skip_serializing)]
    pub config: Value,
    pub capabilities: ConnectorCapabilities,
    pub enabled: bool,
    pub status: ConnectorStatus,
    pub last_tested_at: Option<TimestampMicros>,
    pub last_test_status: Option<ConnectorTestStatus>,
    pub last_test_error: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyTargetType {
    DirectUser,
    FixedAddress,
    FixedGroup,
    Webhook,
}

impl NotifyTargetType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectUser => "direct_user",
            Self::FixedAddress => "fixed_address",
            Self::FixedGroup => "fixed_group",
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyTarget {
    pub target_type: NotifyTargetType,
    pub value: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMessage {
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorDeliveryResult {
    pub provider_message_id: Option<String>,
    pub delivered: bool,
    pub latency_ms: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// 品牌差异的唯一扩展点。通知路由层只按 `connector_type` 查注册表，不直接
/// 判断 Email、Slack、Lark 等具体品牌。
#[async_trait]
pub trait ConnectorAdapter: Send + Sync {
    fn connector_type(&self) -> &'static str;
    fn capabilities(&self) -> ConnectorCapabilities;
    fn validate_config(&self, config: &Value) -> Result<()>;
    fn validate_target(&self, target: &NotifyTarget) -> Result<()>;

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult>;
}
