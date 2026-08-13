// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

pub const DEFAULT_REQUEST_BYTES: i64 = 1_048_576;
pub const DEFAULT_RESPONSE_BYTES: i64 = 1_048_576;
pub const HARD_BODY_BYTES: i64 = 8 * 1_048_576;
pub const DEFAULT_CONCURRENT_CALLS: i32 = 8;
pub const DEFAULT_CALLS_PER_MINUTE: i32 = 60;
pub const DEFAULT_READ_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpSettings {
    pub org_id: Id,
    pub enabled: bool,
    pub allowed_origins: Vec<String>,
    pub max_request_bytes: i64,
    pub max_response_bytes: i64,
    pub max_concurrent_calls: i32,
    pub calls_per_minute: i32,
    pub read_timeout_ms: i64,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl InboundMcpSettings {
    pub fn defaults(org_id: Id) -> Self {
        let now = TimestampMicros::now();
        Self {
            org_id,
            enabled: true,
            allowed_origins: Vec::new(),
            max_request_bytes: DEFAULT_REQUEST_BYTES,
            max_response_bytes: DEFAULT_RESPONSE_BYTES,
            max_concurrent_calls: DEFAULT_CONCURRENT_CALLS,
            calls_per_minute: DEFAULT_CALLS_PER_MINUTE,
            read_timeout_ms: DEFAULT_READ_TIMEOUT_MS,
            updated_by: Id::from_string("system"),
            created_at: now,
            updated_at: now,
        }
    }
}
