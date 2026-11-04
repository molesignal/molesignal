// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::{
        alerting::time_window::TimeCondition,
        notify::{
            connector::{NotifyChannel, NotifyMessage, NotifyTargetType},
            delivery::{DeliveryStage, NotifyDelivery},
            policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyEvent, NotifyFallbackConfig},
            preference::NotifyCategory,
            routing::NotifyDefaultRoute,
        },
    },
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct NotifyPolicyInput {
    pub name: String,
    pub event_type: String,
    pub category: NotifyCategory,
    pub matchers: Value,
    pub recipient_resolver: String,
    pub resolver_config: Value,
    pub delivery_mode: NotifyDeliveryMode,
    pub delivery_config: NotifyDeliveryConfig,
    pub template_id: Option<Id>,
    pub channel_templates: BTreeMap<NotifyChannel, Id>,
    pub fallback_config: NotifyFallbackConfig,
    pub ack_timeout_seconds: Option<i32>,
    pub escalation_config: Option<Value>,
    pub merge_window_secs: Option<u32>,
    pub time: Option<TimeCondition>,
    pub enabled: bool,
    pub priority: i32,
}

#[derive(Debug, Clone)]
pub struct NotifyDefaultInput {
    pub routes: Vec<NotifyDefaultRoute>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyDeliveryPlanStep {
    pub stage: DeliveryStage,
    pub connector_id: Id,
    pub connector_name: String,
    pub endpoint_id: Option<Id>,
    pub target_type: NotifyTargetType,
    pub target_value_masked: String,
    /// 这一步用的模板：策略为这个连接器的渠道另选的，没有时是通用的；都没有时为空。
    pub template_id: Option<Id>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyRecipientPlan {
    pub user_id: Id,
    pub team_id: Option<Id>,
    pub resolved_by: String,
    pub delivery_plan: Vec<NotifyDeliveryPlanStep>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyPolicyPreview {
    pub policy_id: Id,
    pub matched: bool,
    pub recipients: Vec<NotifyRecipientPlan>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyRecipientOutcome {
    pub user_id: Id,
    pub team_id: Option<Id>,
    pub delivered: bool,
    pub attempts: Vec<NotifyDelivery>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyPolicyOutcome {
    pub policy_id: Id,
    pub recipients: Vec<NotifyRecipientOutcome>,
    pub error: Option<String>,
    /// An alert pipeline kept this policy from sending, which is not a failure.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dropped_by_pipeline: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEventOutcome {
    pub event_id: String,
    pub policies: Vec<NotifyPolicyOutcome>,
}

#[derive(Debug, Clone)]
pub struct NotifyDispatch {
    pub event: NotifyEvent,
    pub message: NotifyMessage,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedRoute {
    pub stage: DeliveryStage,
    pub connector: crate::domain::notify::connector::NotifyConnector,
    pub endpoint_id: Option<Id>,
    pub target: crate::domain::notify::connector::NotifyTarget,
    /// 个人接收顺序里这一步的等待时间（秒）；团队与组织兜底是 0。
    pub delay_secs: i32,
}

pub(super) fn now() -> TimestampMicros {
    TimestampMicros::now()
}
