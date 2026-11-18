// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Custom domain + ACME certificate management (spec domain-management, paid edition only).
//!
//! Provides:
//! - `Domain` model + `DomainRepository` trait
//! - `hostname_valid()`: DNS hostname validity check (internationalized names are forbidden)
//! - `AcmeClient` trait: HTTP-01 / DNS-01 challenges; the concrete ACME implementation (such as
//!   `instant-acme`) is injected by OSS
//! - `RenewalSchedule`: renewal is triggered 30 days ahead; 6h retry interval

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainState {
    /// Registration succeeded; waiting for the ACME challenge to complete.
    Pending,
    /// The certificate has been issued and deployed.
    Active,
    /// Certificate renewal failed and exceeded the retry limit.
    Failed,
    /// Deleted by the user (awaiting object storage cleanup).
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
#[serde(rename_all = "camelCase")]
pub struct Domain {
    pub id: Id,
    pub org_id: Id,
    pub hostname: String,
    pub state: DomainState,
    /// PEM certificate chain; empty means not yet issued.
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

/// ACME client abstraction (HTTP-01 challenges are enough; DNS-01 is left for the future).
#[async_trait]
pub trait AcmeClient: Send + Sync {
    /// Requests a new certificate; the caller is responsible for exposing the returned
    /// `challenge_token` / `challenge_response` at the `/.well-known/acme-challenge/<token>` path
    /// so the ACME server can fetch them.
    async fn issue_certificate(&self, hostname: &str) -> Result<IssuedCertificate>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedCertificate {
    pub cert_pem: String,
    pub private_key_pem: String,
    pub not_after: i64,
}

/// Renewal scheduling: returns the cutoff timestamp for certificates expiring within 30 days.
pub fn renewal_cutoff_micros(now: TimestampMicros) -> TimestampMicros {
    TimestampMicros(now.0 + 30 * 24 * 3600 * 1_000_000)
}

/// Retry interval (6h).
pub const RENEWAL_RETRY_SECS: i64 = 6 * 3600;

// RFC 1123-ish hostname: each label is 1-63 characters, total length at most 253, and the TLD
// is at least 2 characters.
static HOSTNAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap());

/// Hostname validity check. Returns ok or Error::Invalid.
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

/// Decides whether a domain needs renewal: cert_not_after is before the cutoff.
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
            // Only 15 days left until expiry (now + 15d < cutoff = now + 30d) -> needs renewal
            cert_not_after: Some(TimestampMicros(now.0 + 15 * 86400 * 1_000_000)),
            created_at: now,
            last_renewal_at: None,
        };
        assert!(needs_renewal(&d, now));

        // Expires in 60 days, no renewal needed
        let d2 = Domain {
            cert_not_after: Some(TimestampMicros(now.0 + 60 * 86400 * 1_000_000)),
            ..d
        };
        assert!(!needs_renewal(&d2, now));
    }
}
