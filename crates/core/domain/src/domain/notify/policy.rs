// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{connector::NotifyChannel, preference::NotifyCategory};
use crate::{
    domain::alerting::time_window::TimeCondition,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct NotifyPolicy {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    /// 从哪条内置策略复制来：组织建立时复制的策略带着它，名字不能改、不能删除，能恢复成内置的样子；
    /// 自己建的策略没有。
    #[serde(default)]
    pub builtin_key: Option<String>,
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
    /// 通用模板：哪个渠道没有另选模板就用它；空着时照通知本身的内容发。
    pub template_id: Option<Id>,
    /// 给各个渠道另选的模板，只能选那个渠道的模板。
    #[serde(default)]
    pub channel_templates: BTreeMap<NotifyChannel, Id>,
    #[serde(default)]
    pub fallback_config: NotifyFallbackConfig,
    pub ack_timeout_seconds: Option<i32>,
    pub escalation_config: Option<Value>,
    /// 告警通知发往同一个接收人与渠道（一个人的飞书、一个群、一个号码）时，这么多秒里的合成一条：
    /// 第一条照常立刻发，其余排着，窗口结束时合成一条。空着时不合并；Webhook 不合并。
    #[serde(default)]
    pub merge_window_secs: Option<u32>,
    /// 只在这些时段（或不在这些时段）发，例如节假日改走电话；空着不限。按通知事件发生的时刻判断，
    /// 不满足时这条策略不发，其他策略照常匹配。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeCondition>,
    pub enabled: bool,
    pub priority: i32,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 合并窗口最短多久：10 秒。
pub const MIN_MERGE_WINDOW_SECS: u32 = 10;

/// 合并窗口最长多久：10 分钟。
pub const MAX_MERGE_WINDOW_SECS: u32 = 600;

/// 合并窗口要在 10 秒到 10 分钟之间，只有告警类（告警、升级）的策略能设。
pub fn validate_merge_window(category: NotifyCategory, secs: Option<u32>) -> Result<()> {
    let Some(secs) = secs else {
        return Ok(());
    };

    if !matches!(category, NotifyCategory::Alert | NotifyCategory::Escalation) {
        return Err(Error::invalid(
            "only alert and escalation notify policies can merge notifications",
        ));
    }

    if !(MIN_MERGE_WINDOW_SECS..=MAX_MERGE_WINDOW_SECS).contains(&secs) {
        return Err(Error::invalid(format!(
            "merge_window_secs must be between {MIN_MERGE_WINDOW_SECS} and {MAX_MERGE_WINDOW_SECS}"
        )));
    }

    Ok(())
}

/// 通知引擎的品牌无关输入。事件来源只负责提供稳定 ID、类型、发生时间和属性。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEvent {
    pub id: String,
    pub event_type: String,
    pub organization_id: Id,
    pub occurred_at: TimestampMicros,
    #[serde(default)]
    pub attributes: Value,
}
