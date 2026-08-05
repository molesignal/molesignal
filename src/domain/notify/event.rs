// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{connector::NotifyMessage, policy::NotifyEvent};
use crate::shared::time::TimestampMicros;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyEventStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl NotifyEventStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> crate::shared::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(crate::shared::Error::internal(format!(
                "unknown notify event status: {other}"
            ))),
        }
    }
}

/// 可重试的通知事件信封。业务事件和渲染后的消息一起持久化，确保确认超时后仍能升级。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyEventRecord {
    pub event: NotifyEvent,
    pub message: NotifyMessage,
    pub status: NotifyEventStatus,
    pub attempt: i32,
    pub next_attempt_at: TimestampMicros,
    pub claimed_at: Option<TimestampMicros>,
    pub last_error: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct NotifyEventClaim {
    pub record: NotifyEventRecord,
    pub acquired: bool,
}
