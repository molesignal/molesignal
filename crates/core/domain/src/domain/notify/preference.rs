// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyCategory {
    Alert,
    Oncall,
    Escalation,
    Report,
    Security,
    System,
}

impl NotifyCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Oncall => "oncall",
            Self::Escalation => "escalation",
            Self::Report => "report",
            Self::Security => "security",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "alert" => Ok(Self::Alert),
            "oncall" => Ok(Self::Oncall),
            "escalation" => Ok(Self::Escalation),
            "report" => Ok(Self::Report),
            "security" => Ok(Self::Security),
            "system" => Ok(Self::System),
            other => Err(Error::internal(format!(
                "unknown notify preference category: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNotifyPreferenceStep {
    pub id: Id,
    pub preference_id: Id,
    pub endpoint_id: Id,
    pub step_order: i32,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNotifyPreference {
    pub id: Id,
    pub organization_id: Id,
    pub user_id: Id,
    pub category: NotifyCategory,
    pub enabled: bool,
    pub quiet_hours: Option<Value>,
    pub allow_critical_bypass: bool,
    pub steps: Vec<UserNotifyPreferenceStep>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
