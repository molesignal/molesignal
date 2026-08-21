// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::Duration;

use object_store::Error as OsError;

use crate::config::ObjectStoreSettings;

#[derive(Debug, Clone, Copy)]
pub(super) struct RetryPolicy {
    pub(super) max_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
    jitter_ratio: f32,
}

impl RetryPolicy {
    pub(super) fn from(settings: &ObjectStoreSettings) -> Self {
        Self {
            max_attempts: settings.retry.max_attempts.max(1),
            base_backoff_ms: settings.retry.base_backoff_ms,
            max_backoff_ms: settings.retry.max_backoff_ms,
            jitter_ratio: settings.retry.jitter_ratio,
        }
    }

    /// 永久错误（NotFound / AlreadyExists / PermissionDenied / InvalidArgument）
    /// 不重试；其余 transient 类型重试。
    pub(super) fn is_retryable(err: &OsError) -> bool {
        match err {
            OsError::NotFound { .. }
            | OsError::AlreadyExists { .. }
            | OsError::PermissionDenied { .. }
            | OsError::Unauthenticated { .. }
            | OsError::InvalidPath { .. }
            | OsError::Precondition { .. }
            | OsError::NotModified { .. }
            | OsError::NotSupported { .. }
            | OsError::UnknownConfigurationKey { .. }
            | OsError::NotImplemented { .. } => false,
            OsError::Generic { source, .. } => {
                let msg = source.to_string().to_lowercase();
                msg.contains("timeout")
                    || msg.contains("slowdown")
                    || msg.contains("throttl")
                    || msg.contains("connection")
                    || msg.contains('5')
            }
            _ => true,
        }
    }

    pub(super) async fn backoff(&self, attempt: u32) {
        let exp = self
            .base_backoff_ms
            .saturating_mul(1u64 << (attempt.saturating_sub(1).min(20)));
        let capped = exp.min(self.max_backoff_ms);
        let jitter_max = (capped as f32 * self.jitter_ratio) as u64;
        let jitter = if jitter_max > 0 {
            use std::time::SystemTime;
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos() as u64)
                .unwrap_or(0);
            (nanos % (jitter_max * 2 + 1)) as i64 - jitter_max as i64
        } else {
            0
        };
        let wait_ms = (capped as i64 + jitter).max(0) as u64;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    }
}
