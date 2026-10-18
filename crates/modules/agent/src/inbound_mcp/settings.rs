// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

/// 请求与响应大小按 MiB 设：默认 1，最多 8。
pub const DEFAULT_REQUEST_SIZE: i32 = 1;
pub const DEFAULT_RESPONSE_SIZE: i32 = 1;
pub const HARD_MAX_SIZE: i32 = 8;

const BYTES_PER_MIB: i64 = 1_048_576;
pub const DEFAULT_CONCURRENT_CALLS: i32 = 8;
pub const DEFAULT_CALLS_PER_MINUTE: i32 = 60;
pub const DEFAULT_READ_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMcpSettings {
    pub org_id: Id,
    pub enabled: bool,
    pub allowed_origins: Vec<String>,
    /// 一次请求的正文最大多少 MiB。
    pub max_request_size: i32,
    /// 一次响应最大多少 MiB。
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

    /// 一次请求的正文最多多少字节。
    pub fn max_request_bytes(&self) -> usize {
        mib_to_bytes(self.max_request_size)
    }

    /// 一次响应最多多少字节。
    pub fn max_response_bytes(&self) -> usize {
        mib_to_bytes(self.max_response_size)
    }
}

/// 无论组织怎么设，一次请求或响应都不会超过这么多字节。
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

        // 存坏了的 0 也至少按 1 MiB 算，不会把每个响应都拦下。
        settings.max_response_size = 0;
        assert_eq!(settings.max_response_bytes(), 1_048_576);
    }
}
