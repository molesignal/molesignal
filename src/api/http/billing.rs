// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 计费/订阅门禁（usage-gating：ingest 入口超额返 402）。
//!
//! 写入路径门禁 [`ensure_ingest_allowed`] 依次判定：
//! 1. **license 过期** → 402；
//! 2. **订阅 / 试用门禁**：计费启用时（Stripe `billing_enabled` 或 marketplace 授权），
//!    订阅全部 suspended/cancelled，或无可服务订阅且试用到期 → 402（见 [`org_blocked`]）；
//! 3. **每日 ingest 配额**：`license.add_ingest_bytes` 超 cap → 402；
//! 4. 记录 per-org 每日用量到 `license_usage_daily`（仅计费部署，失败不阻断 ingest）。
//!
//! 对 OSS / 未接计费的部署：`billing_enabled=false` 且无 marketplace feature →
//! 跳过订阅查询与用量持久化；社区版 license 永不过期、无 cap → 零行为变化。
//!
//! 读路径（search）的计费门禁连同宽限期 / 只读降级策略仍留作后续，避免把用户锁在
//! 自己的数据之外。

use std::sync::atomic::Ordering;

use crate::{
    api::AppState,
    cloud_marketplace::{MARKETPLACE_FEATURE, SubscriptionState, subscriptions_block},
    domain::billing::{TrialState, evaluate_trial_state},
    infra::quotas::QuotaDim,
    shared::{Error, LicenseGate, Result, ids::Id, time::TimestampMicros},
};

/// license 过期判定（纯函数，便于单测）：过期返回 402。
fn ensure_not_expired(license: &dyn LicenseGate, now_micros: i64) -> Result<()> {
    if license.expired(now_micros) {
        return Err(Error::payment_required(
            "subscription expired; ingestion is paused until the license is renewed",
        ));
    }
    Ok(())
}

/// micros → `YYYY-MM-DD`（UTC），用作 `license_usage_daily.day`。
pub(crate) fn utc_day(now_micros: i64) -> String {
    chrono::DateTime::from_timestamp(now_micros / 1_000_000, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string()
}

/// 计费是否启用（cheap）：Stripe billing 开关 或 marketplace 授权。
fn billing_on(state: &AppState) -> bool {
    state.platform.billing_enabled.load(Ordering::Relaxed)
        || state.platform.license.has_feature(MARKETPLACE_FEATURE)
}

/// 统一的"org 是否被停服"判定（写入门禁与请求拦截 middleware 共用）。
///
/// 计费启用时，满足任一即视为停服：
/// - 订阅存在但**全部**停服 / 取消（无 active / pending，见 [`subscriptions_block`]）；
/// - **无任何可服务订阅**且试用已到期。
///
/// 无订阅记录且无试用记录 → 不拦截（新 org 宽限，避免在开通前把人锁死）。
/// 计费未启用（OSS / 未接计费）→ 永远不拦截。
pub(crate) async fn org_blocked(state: &AppState, org_id: &Id, now_micros: i64) -> Result<bool> {
    if !billing_on(state) {
        return Ok(false);
    }
    let subs = state.platform.marketplace.list(org_id).await?;
    let sub_states: Vec<SubscriptionState> = subs
        .iter()
        .map(|s| SubscriptionState::parse(&s.state))
        .collect();
    // 仅在"无任何订阅"时才需要看试用（有订阅则由订阅状态完全决定，省一次查库）。
    let trial_state = if sub_states.is_empty() {
        state
            .platform
            .trials
            .get(org_id)
            .await?
            .map(|t| evaluate_trial_state(t.ends_at, TimestampMicros(now_micros), false))
    } else {
        None
    };
    Ok(decide_blocked(&sub_states, trial_state))
}

/// 纯停服判定（抽离便于单测）：
/// - 订阅存在但无一可服务（active/pending）→ 拦截；
/// - 存在可服务订阅 → 放行（试用视作已转付费）；
/// - 无任何订阅：试用已到期 → 拦截，否则（含无试用记录）放行。
fn decide_blocked(sub_states: &[SubscriptionState], trial_state: Option<TrialState>) -> bool {
    if subscriptions_block(sub_states.iter().copied()) {
        return true;
    }
    let has_serviceable = sub_states
        .iter()
        .any(|s| matches!(s, SubscriptionState::Active | SubscriptionState::Pending));
    if has_serviceable {
        return false;
    }
    matches!(trial_state, Some(TrialState::Expired))
}

/// [`org_blocked`] 的缓存包装。短 TTL 缓存把"每请求查订阅/试用"降到"每 org 每 TTL 一次"：
/// 命中 [`BillingStateCache`] 直接返回，未命中查库算 blocked 并回填。订阅变更经 webhook
/// 精确失效（见 routes/billing.rs、marketplace.rs）；试用到期由 30s TTL 兜底。
pub(crate) async fn org_blocked_cached(
    state: &AppState,
    org_id: &Id,
    now_micros: i64,
) -> Result<bool> {
    if !billing_on(state) {
        return Ok(false);
    }
    if let Some(blocked) = state.platform.billing_state_cache.get(org_id).await {
        return Ok(blocked);
    }
    let blocked = org_blocked(state, org_id, now_micros).await?;
    state
        .platform
        .billing_state_cache
        .put(org_id, blocked)
        .await;
    Ok(blocked)
}

/// 写入路径计费门禁 + 计量。`bytes` 为本批原始字节数（计量与 cap 用）。
pub(crate) async fn ensure_ingest_allowed(
    state: &AppState,
    org_id: &Id,
    bytes: u64,
    now_micros: i64,
) -> Result<()> {
    let license = state.platform.license.as_ref();
    ensure_not_expired(license, now_micros)?;

    // 订阅停服 / 试用到期门禁（仅计费部署；OSS 直接放行）。
    if org_blocked_cached(state, org_id, now_micros).await? {
        return Err(Error::payment_required(
            "organization subscription is suspended or cancelled, or the trial has expired",
        ));
    }

    // per-org 配额：所有信号摄取共用此门禁。`max_ingest_qps` 超 →
    // 429 + 重试秒数；`max_storage_bytes` 超 → 413（调用方据此不写对象）。无 `quotas`
    // 记录的 org 上限为 0 → 视作无限制，直接放行（不影响 OSS / 未配额 org）。
    if let Some(retry_secs) = state.platform.quotas.acquire(org_id, QuotaDim::Ingest) {
        return Err(Error::resource_exhausted(format!(
            "ingest rate limit exceeded; retry after {retry_secs}s"
        )));
    }
    if !state.platform.quotas.check_storage_cap(org_id, bytes) {
        return Err(Error::payload_too_large(
            "organization storage quota exceeded",
        ));
    }

    // 每日 ingest 配额（license 进程内累计；社区版无 cap → 永远 true）。
    let under_cap = license.add_ingest_bytes(bytes);
    if !under_cap {
        return Err(Error::payment_required("daily ingest quota exceeded"));
    }

    // 首页运营视图需要按时间窗区分「原始摄入量」与「压缩后落盘量」。小时桶只做
    // best-effort 观测，不参与 license / quota 判定，OSS 也记录。
    if bytes > 0 {
        let usage = state.platform.usage.clone();
        let usage_org_id = org_id.clone();
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            if let Err(e) = usage
                .add_hourly_ingest_bytes(&usage_org_id, now_micros, bytes as i64)
                .await
            {
                tracing::warn!(
                    org_id = %usage_org_id.0,
                    error = %e,
                    "failed to record hourly ingest usage"
                );
            }
        });
    }

    // 记录 per-org 每日用量（仅计费部署；best-effort，不阻断 ingest）。
    if billing_on(state) && bytes > 0 {
        let day = utc_day(now_micros);
        if let Err(e) = state
            .platform
            .usage
            .add_ingest_bytes(org_id, &day, bytes as i64)
            .await
        {
            tracing::warn!(org_id = %org_id.0, error = %e, "failed to record daily ingest usage");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decide_blocked, ensure_not_expired, utc_day};
    use crate::{
        cloud_marketplace::SubscriptionState, domain::billing::TrialState, shared::LicenseGate,
    };

    #[test]
    fn decide_blocked_subscription_matrix() {
        use SubscriptionState::*;
        // 无订阅、无试用记录 → 宽限放行（新 org 尚未开通）。
        assert!(!decide_blocked(&[], None));
        // 有可服务订阅 → 放行（试用与否都不拦）。
        assert!(!decide_blocked(&[Active], None));
        assert!(!decide_blocked(&[Pending], Some(TrialState::Expired)));
        assert!(!decide_blocked(&[Cancelled, Active], None));
        // 订阅全部停服 / 取消 → 拦截（试用态无关）。
        assert!(decide_blocked(&[Suspended], None));
        assert!(decide_blocked(&[Cancelled], Some(TrialState::Active)));
        assert!(decide_blocked(&[Cancelled, Suspended], None));
    }

    #[test]
    fn decide_blocked_trial_only_matrix() {
        // 无订阅时由试用态决定。
        assert!(decide_blocked(&[], Some(TrialState::Expired)));
        assert!(!decide_blocked(&[], Some(TrialState::Active)));
        assert!(!decide_blocked(&[], Some(TrialState::Converted)));
    }

    /// 可控过期态的测试 license。
    struct FakeLicense {
        expires_at: i64,
    }
    impl LicenseGate for FakeLicense {
        fn has_feature(&self, _: &str) -> bool {
            false
        }
        fn add_ingest_bytes(&self, _: u64) -> bool {
            true
        }
        fn expired(&self, now_micros: i64) -> bool {
            now_micros >= self.expires_at
        }
        fn issued_to(&self) -> &str {
            "test"
        }
        fn reset_daily(&self) {}
    }

    #[test]
    fn allows_active_and_blocks_expired() {
        let lic = FakeLicense { expires_at: 1_000 };
        assert!(ensure_not_expired(&lic, 999).is_ok());
        let err = ensure_not_expired(&lic, 1_000).unwrap_err();
        assert_eq!(err.http_status_code(), 402);
    }

    #[test]
    fn utc_day_formats_epoch() {
        // 0 micros → 1970-01-01。
        assert_eq!(utc_day(0), "1970-01-01");
        // 1_700_000_000 秒 = 2023-11-14T22:13:20Z。
        assert_eq!(utc_day(1_700_000_000_000_000), "2023-11-14");
    }
}
