// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundMcpTaskStatus {
    Working,
    InputRequired,
    Completed,
    Failed,
    Cancelled,
}

impl InboundMcpTaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::InputRequired => "input_required",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpTask {
    pub id: Id,
    pub org_id: Id,
    pub principal_type: String,
    pub principal_id: Id,
    pub tool_name: String,
    pub request: Value,
    pub status: InboundMcpTaskStatus,
    pub status_message: Option<String>,
    pub input_requests: Option<Value>,
    pub input_responses: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub cancel_requested: bool,
    pub expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
