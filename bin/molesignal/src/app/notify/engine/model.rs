// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::notify::{
        connector::{NotifyMessage, NotifyTargetType},
        delivery::{DeliveryStage, NotifyDelivery},
        policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyEvent, NotifyFallbackConfig},
        preference::NotifyCategory,
        routing::NotifyDefaultRoute,
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
    pub fallback_config: NotifyFallbackConfig,
    pub ack_timeout_seconds: Option<i32>,
    pub escalation_config: Option<Value>,
    pub enabled: bool,
    pub priority: i32,
}

#[derive(Debug, Clone)]
pub struct NotifyDefaultInput {
    pub routes: Vec<NotifyDefaultRoute>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyDeliveryPlanStep {
    pub stage: DeliveryStage,
    pub connector_id: Id,
    pub connector_name: String,
    pub endpoint_id: Option<Id>,
    pub target_type: NotifyTargetType,
    pub target_value_masked: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyRecipientPlan {
    pub user_id: Id,
    pub team_id: Option<Id>,
    pub resolved_by: String,
    pub delivery_plan: Vec<NotifyDeliveryPlanStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyPolicyPreview {
    pub policy_id: Id,
    pub matched: bool,
    pub recipients: Vec<NotifyRecipientPlan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyRecipientOutcome {
    pub user_id: Id,
    pub team_id: Option<Id>,
    pub delivered: bool,
    pub attempts: Vec<NotifyDelivery>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyPolicyOutcome {
    pub policy_id: Id,
    pub recipients: Vec<NotifyRecipientOutcome>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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
}

pub(super) fn now() -> TimestampMicros {
    TimestampMicros::now()
}
