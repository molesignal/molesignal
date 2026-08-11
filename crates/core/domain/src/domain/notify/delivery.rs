// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    UserPrimary,
    UserFallback,
    TeamFallback,
    OrganizationFallback,
    Escalation,
    Test,
}

impl DeliveryStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserPrimary => "user_primary",
            Self::UserFallback => "user_fallback",
            Self::TeamFallback => "team_fallback",
            Self::OrganizationFallback => "organization_fallback",
            Self::Escalation => "escalation",
            Self::Test => "test",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "user_primary" => Ok(Self::UserPrimary),
            "user_fallback" => Ok(Self::UserFallback),
            "team_fallback" => Ok(Self::TeamFallback),
            "organization_fallback" => Ok(Self::OrganizationFallback),
            "escalation" => Ok(Self::Escalation),
            "test" => Ok(Self::Test),
            other => Err(Error::internal(format!(
                "unknown notify delivery stage: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Sending,
    Success,
    Failed,
    Skipped,
    Acknowledged,
}

impl DeliveryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sending => "sending",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Acknowledged => "acknowledged",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "sending" => Ok(Self::Sending),
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            "acknowledged" => Ok(Self::Acknowledged),
            other => Err(Error::internal(format!(
                "unknown notify delivery status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyDelivery {
    pub id: Id,
    pub organization_id: Id,
    pub event_id: String,
    pub policy_id: Option<Id>,
    pub recipient_user_id: Option<Id>,
    pub connector_id: Option<Id>,
    pub endpoint_id: Option<Id>,
    pub target_type: String,
    pub target_value_masked: Option<String>,
    pub stage: DeliveryStage,
    pub attempt: i32,
    pub status: DeliveryStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub latency_ms: Option<i32>,
    pub sent_at: Option<TimestampMicros>,
    pub delivered_at: Option<TimestampMicros>,
    pub acknowledged_at: Option<TimestampMicros>,
    pub escalated_at: Option<TimestampMicros>,
    pub idempotency_key: String,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct DeliveryClaim {
    pub delivery: NotifyDelivery,
    pub acquired: bool,
}

#[derive(Debug, Clone)]
pub struct DeliveryCompletion {
    pub status: DeliveryStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub latency_ms: Option<i32>,
    pub delivered_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryFilter {
    pub event_id: Option<String>,
    pub policy_id: Option<Id>,
    pub recipient_user_id: Option<Id>,
    pub connector_id: Option<Id>,
    pub status: Option<DeliveryStatus>,
    pub stage: Option<DeliveryStage>,
    pub from: Option<TimestampMicros>,
    pub to: Option<TimestampMicros>,
    pub limit: u32,
}
