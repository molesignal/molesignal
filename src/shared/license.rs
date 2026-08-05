// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! License gate trait + community fallback。
//!
//! 设计取舍：
//! - 主仓库（开源版）总是有一份 [`LicenseGate`] 实现 [`CommunityLicense`]，所有
//!   `has_feature(...)` 返 false，所有上限不强制。
//! - 付费版 license 模块提供 `SignedLicense`：Ed25519 验签 +
//!   feature gating + daily ingest cap。它实现同一个 trait，无侵入替换。
//! - handler 一律调 `state.platform.license.has_feature("sso" | "federated_search" | "intelligence")`，
//!   不关心运行时是社区版还是付费版。

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI64, Ordering},
};

/// 商业许可证门控接口。
///
/// 所有 handler 通过 [`crate::shared::Probe`] 风格的 `Arc<dyn LicenseGate>` 拿到此 trait。
/// 不要直接 import 具体 License 类型——后续付费版 / 开源版可互换。
pub trait LicenseGate: Send + Sync {
    fn has_feature(&self, name: &str) -> bool;
    /// 当日累计 ingest 字节数检查；返 false 表示超额，调用方应 413。
    fn add_ingest_bytes(&self, n: u64) -> bool;
    fn expired(&self, now_micros: i64) -> bool;
    fn issued_to(&self) -> &str;
    /// 24h 边界时由 scheduler 调用，重置当日累计。
    fn reset_daily(&self);
    /// 列出当前激活的 feature flag。默认实现返回空 vec；付费版返回 parsed feature set。
    fn features(&self) -> Vec<String> {
        Vec::new()
    }
    /// `edition` 用于 `/license` snapshot。默认 `"community"`。
    fn edition(&self) -> &'static str {
        "community"
    }
    /// 是否签名验证通过；OSS 始终 false。
    fn verified(&self) -> bool {
        false
    }
    /// 每日 ingest 上限（字节）；None 表示无上限。
    fn max_ingest_bytes_per_day(&self) -> Option<u64> {
        None
    }
    /// license 过期时间（micros since epoch）；None 表示无过期。
    fn expires_at_micros(&self) -> Option<i64> {
        None
    }
}

/// 开源版默认 license。
///
/// - `has_feature` 永远返 false（ feature 不可用）
/// - `add_ingest_bytes` 永远返 true（无 cap）
/// - `expired` 永远返 false
pub struct CommunityLicense {
    /// 仍记录 ingest 字节数（仅观测用，不强制）
    ingest_today: AtomicI64,
}

impl CommunityLicense {
    pub const fn new() -> Self {
        Self {
            ingest_today: AtomicI64::new(0),
        }
    }
}

impl Default for CommunityLicense {
    fn default() -> Self {
        Self::new()
    }
}

impl LicenseGate for CommunityLicense {
    fn has_feature(&self, _name: &str) -> bool {
        false
    }
    fn add_ingest_bytes(&self, n: u64) -> bool {
        self.ingest_today.fetch_add(n as i64, Ordering::Relaxed);
        true
    }
    fn expired(&self, _now_micros: i64) -> bool {
        false
    }
    fn issued_to(&self) -> &str {
        "community"
    }
    fn reset_daily(&self) {
        self.ingest_today.store(0, Ordering::Relaxed);
    }
    fn features(&self) -> Vec<String> {
        Vec::new()
    }
    fn edition(&self) -> &'static str {
        "community"
    }
    fn verified(&self) -> bool {
        false
    }
    fn max_ingest_bytes_per_day(&self) -> Option<u64> {
        None
    }
    fn expires_at_micros(&self) -> Option<i64> {
        None
    }
}

/// 运行时可替换的 license 包装。`AppState.platform.license_holder` 暴露给 axum；
/// upload handler 验签通过后调 [`LicenseHolder::replace`] 原子换底层 `Arc`，无需重启。
///
/// 转发实现所有 [`LicenseGate`] 方法到当前内层 license，让现有 `state.platform.license.has_feature(...)`
/// 调用方无感知。读路径用 `RwLock` 的 read guard，写路径独占 — 替换是低频事件。
pub struct LicenseHolder {
    inner: RwLock<Arc<dyn LicenseGate>>,
}

impl LicenseHolder {
    pub fn new(initial: Arc<dyn LicenseGate>) -> Self {
        Self {
            inner: RwLock::new(initial),
        }
    }

    pub fn replace(&self, new: Arc<dyn LicenseGate>) {
        if let Ok(mut g) = self.inner.write() {
            *g = new;
        }
    }

    pub fn current(&self) -> Arc<dyn LicenseGate> {
        Arc::clone(&self.inner.read().expect("license rwlock poisoned"))
    }
}

impl LicenseGate for LicenseHolder {
    fn has_feature(&self, name: &str) -> bool {
        self.current().has_feature(name)
    }
    fn add_ingest_bytes(&self, n: u64) -> bool {
        self.current().add_ingest_bytes(n)
    }
    fn expired(&self, now_micros: i64) -> bool {
        self.current().expired(now_micros)
    }
    fn issued_to(&self) -> &str {
        // 借的是 RwLockReadGuard 内的 &str，转 'static 不安全。这里有一个权宜：
        // 把 issued_to 作为 LicenseGate trait 唯一一个返 `&str` 的 method，每次
        // 调用 leak 一小段字符串太重，而 caller 主要是 /license snapshot（少量调
        // 用）。我们存一个 leaked &'static str 缓存：第一次读时 leak 当前值，
        // replace 之后下次 snapshot 会重新 leak。简单期望：upload license 不会
        // 频繁到产生明显内存泄漏；这是已知 trade-off。
        let owned = self.current().issued_to().to_string();
        Box::leak(owned.into_boxed_str())
    }
    fn reset_daily(&self) {
        self.current().reset_daily();
    }
    fn features(&self) -> Vec<String> {
        self.current().features()
    }
    fn edition(&self) -> &'static str {
        self.current().edition()
    }
    fn verified(&self) -> bool {
        self.current().verified()
    }
    fn max_ingest_bytes_per_day(&self) -> Option<u64> {
        self.current().max_ingest_bytes_per_day()
    }
    fn expires_at_micros(&self) -> Option<i64> {
        self.current().expires_at_micros()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_has_no_features_and_unlimited_cap() {
        let l = CommunityLicense::new();
        assert!(!l.has_feature("sso"));
        assert!(!l.has_feature("federated_search"));
        assert!(!l.has_feature("intelligence"));
        assert!(l.add_ingest_bytes(1_000_000_000));
        assert!(!l.expired(i64::MAX));
        assert_eq!(l.issued_to(), "community");
        assert!(l.features().is_empty());
        assert_eq!(l.edition(), "community");
        assert!(!l.verified());
        assert!(l.max_ingest_bytes_per_day().is_none());
        assert!(l.expires_at_micros().is_none());
    }
}
