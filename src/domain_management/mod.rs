// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Custom domain + ACME 证书管理（spec domain-management，付费版独占）。
//!
//! 提供：
//! - `Domain` 模型 + `DomainRepository` trait
//! - `hostname_valid()` —— DNS hostname 合法性校验（含国际化禁止）
//! - `AcmeClient` trait —— HTTP-01 / DNS-01 挑战；具体 ACME 实现（如 `instant-acme`）
//!   由 OSS 注入
//! - `RenewalSchedule` —— 30 天前触发续期；6h 重试间隔

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const DOMAIN_FEATURE: &str = "domain_management";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainState {
    /// 注册成功，等待 ACME 挑战完成
    Pending,
    /// 证书已签发并部署
    Active,
    /// 证书续期失败超过重试上限
    Failed,
    /// 用户删除（待对象存储清理）
    Cancelled,
}

impl DomainState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    pub id: Id,
    pub org_id: Id,
    pub hostname: String,
    pub state: DomainState,
    /// PEM 证书链；为空表示尚未签发
    pub cert_pem: Option<String>,
    pub cert_not_after: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub last_renewal_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait DomainRepository: Send + Sync {
    async fn create(&self, d: Domain) -> Result<Domain>;
    async fn get_by_hostname(&self, hostname: &str) -> Result<Option<Domain>>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Domain>>;
    async fn update_cert(
        &self,
        id: &Id,
        cert_pem: &str,
        not_after: TimestampMicros,
        state: DomainState,
    ) -> Result<()>;
    async fn list_renewals_due(&self, not_after_cutoff: TimestampMicros) -> Result<Vec<Domain>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

/// ACME 客户端抽象（HTTP-01 挑战即可，DNS-01 留 future）。
#[async_trait]
pub trait AcmeClient: Send + Sync {
    /// 请求新证书；调用方负责把返回的 `challenge_token` / `challenge_response`
    /// 暴露在 `/.well-known/acme-challenge/<token>` 路径让 ACME server 抓取。
    async fn issue_certificate(&self, hostname: &str) -> Result<IssuedCertificate>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedCertificate {
    pub cert_pem: String,
    pub private_key_pem: String,
    pub not_after_micros: i64,
}

/// 续期调度：返 30 天内即将过期的 cutoff 时间戳。
pub fn renewal_cutoff_micros(now: TimestampMicros) -> TimestampMicros {
    TimestampMicros(now.0 + 30 * 24 * 3600 * 1_000_000)
}

/// 重试间隔（6h）。
pub const RENEWAL_RETRY_SECS: i64 = 6 * 3600;

// RFC 1123-ish hostname：每段 1-63 字符，整体 ≤ 253，TLD 至少 2 字符。
static HOSTNAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap());

/// Hostname 合法性校验。返 ok 或 Error::Invalid。
pub fn hostname_valid(h: &str) -> Result<()> {
    if h.is_empty() {
        return Err(Error::invalid("hostname must not be empty"));
    }
    if h.len() > 253 {
        return Err(Error::invalid("hostname must be ≤ 253 chars (RFC 1035)"));
    }
    let lower = h.to_lowercase();
    if !HOSTNAME_RE.is_match(&lower) {
        return Err(Error::invalid(format!(
            "invalid hostname '{h}': must be DNS-compatible (RFC 1123)"
        )));
    }
    Ok(())
}

/// 决定一个 domain 是否需要续期：cert_not_after 在 cutoff 之前。
pub fn needs_renewal(domain: &Domain, now: TimestampMicros) -> bool {
    let cutoff = renewal_cutoff_micros(now);
    domain.cert_not_after.is_some_and(|t| t.0 < cutoff.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_hostnames_accepted() {
        assert!(hostname_valid("obs.acme.com").is_ok());
        assert!(hostname_valid("a-b.c-d.io").is_ok());
        assert!(hostname_valid("EXAMPLE.COM").is_ok()); // lowercase normalize
        assert!(hostname_valid("sub.domain.example.com").is_ok());
    }

    #[test]
    fn bad_hostnames_rejected() {
        assert!(hostname_valid("").is_err());
        assert!(hostname_valid("no_underscore.com").is_err());
        assert!(hostname_valid("trailing.dot.").is_err());
        assert!(hostname_valid("missing-tld").is_err());
        assert!(hostname_valid("-leading-hyphen.com").is_err());
        let long = "a".repeat(70);
        assert!(hostname_valid(&format!("{long}.com")).is_err()); // segment > 63
    }

    #[test]
    fn renewal_cutoff_30_days() {
        let now = TimestampMicros(0);
        let cutoff = renewal_cutoff_micros(now);
        assert_eq!(cutoff.0, 30 * 24 * 3600 * 1_000_000);
    }

    #[test]
    fn needs_renewal_when_cert_expires_within_30_days() {
        let now = TimestampMicros(100 * 86400 * 1_000_000);
        let d = Domain {
            id: Id::new(),
            org_id: Id("o".into()),
            hostname: "x.com".into(),
            state: DomainState::Active,
            cert_pem: Some("PEM".into()),
            // 还有 15 天就过期（now + 15d < cutoff = now + 30d）→ 需续
            cert_not_after: Some(TimestampMicros(now.0 + 15 * 86400 * 1_000_000)),
            created_at: now,
            last_renewal_at: None,
        };
        assert!(needs_renewal(&d, now));

        // 60 天后过期，不需续
        let d2 = Domain {
            cert_not_after: Some(TimestampMicros(now.0 + 60 * 86400 * 1_000_000)),
            ..d
        };
        assert!(!needs_renewal(&d2, now));
    }
}
