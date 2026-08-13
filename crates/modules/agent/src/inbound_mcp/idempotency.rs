// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone)]
pub struct InboundMcpIdempotencyInput {
    pub org_id: Id,
    pub principal_type: String,
    pub principal_id: Id,
    pub idempotency_key: String,
    pub tool_name: String,
    pub request_hash: String,
    pub lease_expires_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpIdempotencyRecord {
    pub org_id: Id,
    pub principal_type: String,
    pub principal_id: Id,
    pub idempotency_key: String,
    pub tool_name: String,
    pub request_hash: String,
    pub status: String,
    pub approval_id: Option<Id>,
    pub result: Option<Value>,
    pub lease_expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub enum InboundMcpIdempotencyReservation {
    Acquired,
    Existing(Box<InboundMcpIdempotencyRecord>),
}
