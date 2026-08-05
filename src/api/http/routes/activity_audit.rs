// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 敏感生命周期操作的审计事件写入 helper。
//!
//! provider / prompt / chat / tool-call / archive 生命周期操作经此写 `audit_events`。
//! payload **必须**排除明文密钥、超限 prompt body、原始 tool 结果行、完整 transcript；
//! 只留稳定 id / target_kind / status / 版本 / hash / object_key / masked 元数据。

use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{ids::Id, time::TimestampMicros},
};

/// 审计 payload 大小软上限（字节）；超限字段（如完整 prompt body）应替换为 hash/摘要。
pub const AUDIT_PAYLOAD_LIMIT: usize = 8 * 1024;

/// best-effort 写一条 Intelligence lifecycle 审计事件；失败仅 warn，不阻塞主流程。
pub async fn record(
    state: &AppState,
    ctx: &IamContext,
    action: &str,
    target_kind: &str,
    target_id: &str,
    payload: Value,
) {
    let e = AuditEvent {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        actor_kind: "user".into(),
        actor_id: ctx.user_id.0.clone(),
        action: action.into(),
        target_kind: Some(target_kind.into()),
        target_id: Some(target_id.into()),
        ip: None,
        user_agent: None,
        payload,
        ts: TimestampMicros::now(),
    };
    if let Err(err) = state.iam.audit_events.record(e).await {
        tracing::warn!(action, target_id, error = %err, "failed to record intelligence audit event");
    }
}

/// 超 [`AUDIT_PAYLOAD_LIMIT`] 的字符串字段截断为摘要（用于 prompt body 之类）。
pub fn redact_oversized(s: &str) -> Value {
    if s.len() > AUDIT_PAYLOAD_LIMIT {
        Value::String(format!("<redacted {} bytes>", s.len()))
    } else {
        Value::String(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_passes_small_and_truncates_large() {
        assert_eq!(redact_oversized("short"), Value::String("short".into()));
        let big = "x".repeat(AUDIT_PAYLOAD_LIMIT + 1);
        let red = redact_oversized(&big);
        let s = red.as_str().unwrap();
        assert!(s.starts_with("<redacted "));
        assert!(!s.contains("xxxx"), "oversized content must not be echoed");
    }
}
