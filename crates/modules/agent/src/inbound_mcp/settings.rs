// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::{ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

/// Request and response sizes are set in MiB: the default is 1, and the maximum is 8.
pub const DEFAULT_REQUEST_SIZE: i32 = 1;
pub const DEFAULT_RESPONSE_SIZE: i32 = 1;
pub const HARD_MAX_SIZE: i32 = 8;

const BYTES_PER_MIB: i64 = 1_048_576;
pub const DEFAULT_CONCURRENT_CALLS: i32 = 8;
pub const DEFAULT_CALLS_PER_MINUTE: i32 = 60;
pub const DEFAULT_READ_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundMcpSettings {
    pub org_id: Id,
    pub enabled: bool,
    pub allowed_origins: Vec<String>,
    /// The maximum request body size in MiB.
    pub max_request_size: i32,
    /// The maximum response size in MiB.
    pub max_response_size: i32,
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
            max_request_size: DEFAULT_REQUEST_SIZE,
            max_response_size: DEFAULT_RESPONSE_SIZE,
            max_concurrent_calls: DEFAULT_CONCURRENT_CALLS,
            calls_per_minute: DEFAULT_CALLS_PER_MINUTE,
            read_timeout_ms: DEFAULT_READ_TIMEOUT_MS,
            updated_by: Id::from_string("system"),
            created_at: now,
            updated_at: now,
        }
    }

    /// The maximum request body size in bytes.
    pub fn max_request_bytes(&self) -> usize {
        mib_to_bytes(self.max_request_size)
    }

    /// The maximum response size in bytes.
    pub fn max_response_bytes(&self) -> usize {
        mib_to_bytes(self.max_response_size)
    }
}

/// The maximum bytes per request or response, regardless of the organization's settings.
pub fn hard_max_bytes() -> usize {
    mib_to_bytes(HARD_MAX_SIZE)
}

fn mib_to_bytes(size: i32) -> usize {
    usize::try_from(i64::from(size.max(1)).saturating_mul(BYTES_PER_MIB)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_mebibytes() {
        let mut settings = InboundMcpSettings::defaults(Id::from_string("org"));
        assert_eq!(settings.max_request_bytes(), 1_048_576);

        settings.max_response_size = 8;
        assert_eq!(settings.max_response_bytes(), 8 * 1_048_576);

        // Even an incorrectly stored 0 counts as at least 1 MiB, so it does not block every
        // response.
        settings.max_response_size = 0;
        assert_eq!(settings.max_response_bytes(), 1_048_576);
    }
}
