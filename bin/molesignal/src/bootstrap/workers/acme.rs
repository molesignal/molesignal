// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ACME runner（change `domain-acme-tls`）。
//!
//! 起两个 tokio task：
//! - `issue_loop` 每 `issue_poll_secs` 秒扫 `state="pending"` 的 domain → `AcmeClient::issue`
//! - `renewal_loop` 每 `renewal_retry_secs` 秒扫 `state="active"` 且 `cert_not_after < now + 30d`
//!
//! 进程内 cooldown DashMap：同 domain 60s 内不重试（防 LE rate limit）。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;

use crate::{
    bootstrap::{acme::AcmeClient, tls::SniCertResolver},
    infra::persistence::repositories::domains::{DomainRepository, DomainRow},
    shared::time::TimestampMicros,
};

const COOLDOWN: Duration = Duration::from_secs(60);
const RENEWAL_LEAD_MICROS: i64 = 30 * 24 * 3600 * 1_000_000;

pub struct AcmeRunner {
    client: AcmeClient,
    domains: Arc<dyn DomainRepository>,
    resolver: Arc<SniCertResolver>,
    cooldown: Arc<DashMap<String, Instant>>,
    issue_interval: Duration,
    renewal_interval: Duration,
}

impl AcmeRunner {
    pub fn new(
        client: AcmeClient,
        domains: Arc<dyn DomainRepository>,
        resolver: Arc<SniCertResolver>,
        issue_poll_secs: u64,
        renewal_retry_secs: u64,
    ) -> Self {
        Self {
            client,
            domains,
            resolver,
            cooldown: Arc::new(DashMap::new()),
            issue_interval: Duration::from_secs(issue_poll_secs),
            renewal_interval: Duration::from_secs(renewal_retry_secs),
        }
    }

    /// 起两个 background loop。返回的 `JoinHandle` 由 caller 保存以保证 task 不被立即 drop。
    pub fn spawn(self: Arc<Self>) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
        let issue_handle = {
            let this = self.clone();
            tokio::spawn(async move { this.issue_loop().await })
        };
        let renewal_handle = {
            let this = self.clone();
            tokio::spawn(async move { this.renewal_loop().await })
        };
        (issue_handle, renewal_handle)
    }

    async fn issue_loop(&self) {
        let mut tick = tokio::time::interval(self.issue_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(e) = self.scan_pending().await {
                tracing::warn!(error = %e, "acme_runner: pending scan failed");
            }
        }
    }

    async fn renewal_loop(&self) {
        let mut tick = tokio::time::interval(self.renewal_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(e) = self.scan_renewals().await {
                tracing::warn!(error = %e, "acme_runner: renewal scan failed");
            }
        }
    }

    /// 扫 `state="pending"` 的 domain，逐个尝试签发（cooldown 自动防抖、失败仅记日志续跑）。
    async fn scan_pending(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let rows = self.domains.list_by_state("pending").await?;
        for row in &rows {
            if let Err(e) = self.issue_one(row).await {
                tracing::warn!(hostname = %row.hostname, error = %e, "acme_runner: issue failed");
            }
        }
        Ok(())
    }

    /// 扫 `state="active"` 的 domain，对进入续期窗口（≤30d 或缺证）的逐个续签。
    async fn scan_renewals(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let rows = self.domains.list_by_state("active").await?;
        let now = TimestampMicros::now().0;
        for row in due_for_renewal(&rows, now) {
            if let Err(e) = self.issue_one(row).await {
                tracing::warn!(hostname = %row.hostname, error = %e, "acme_runner: renewal failed");
            }
        }
        Ok(())
    }

    /// 单条 issue 路径，供 unit test + 手动 trigger。
    /// 写 cert 到 `domains.cert_pem`，invalidate resolver cache。
    #[tracing::instrument(
        name = "worker.acme_issue",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "acme_issue")
    )]
    pub async fn issue_one(
        &self,
        row: &DomainRow,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.is_in_cooldown(&row.hostname) {
            return Ok(());
        }
        self.cooldown.insert(row.hostname.clone(), Instant::now());

        match self.client.issue(&row.hostname, &row.id).await {
            Ok(issued) => {
                self.domains
                    .update_state(
                        &row.id,
                        "active",
                        Some(&issued.cert_pem),
                        Some(TimestampMicros(issued.not_after_micros)),
                        None,
                    )
                    .await?;
                self.resolver.invalidate(&row.hostname);
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                self.domains
                    .update_state(
                        &row.id,
                        if row.cert_pem.is_some() {
                            "active"
                        } else {
                            "failed"
                        },
                        row.cert_pem.as_deref(),
                        row.cert_not_after,
                        Some(&msg),
                    )
                    .await?;
                Err(e.into())
            }
        }
    }

    fn is_in_cooldown(&self, hostname: &str) -> bool {
        self.cooldown
            .get(hostname)
            .map(|t| t.elapsed() < COOLDOWN)
            .unwrap_or(false)
    }
}

/// 给外部 schedule trigger 用：判断一个 cert 是否到 renewal 窗口内。
pub fn needs_renewal(row: &DomainRow, now_micros: i64) -> bool {
    match row.cert_not_after {
        Some(TimestampMicros(t)) => t < now_micros + RENEWAL_LEAD_MICROS,
        None => true,
    }
}

/// 从一批 active domain 里筛出进入续期窗口的（纯函数，便于单测）。
pub fn due_for_renewal(rows: &[DomainRow], now_micros: i64) -> Vec<&DomainRow> {
    rows.iter()
        .filter(|row| needs_renewal(row, now_micros))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn mk_row(hostname: &str, not_after: Option<i64>) -> DomainRow {
        DomainRow {
            id: Id::new(),
            org_id: Id("org".into()),
            hostname: hostname.into(),
            state: "active".into(),
            cert_pem: Some("dummy".into()),
            cert_not_after: not_after.map(TimestampMicros),
            last_error: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    #[test]
    fn needs_renewal_when_cert_within_30_days() {
        let now = 1_700_000_000_000_000_i64;
        let twenty_days_from_now = now + 20 * 24 * 3600 * 1_000_000;
        let sixty_days_from_now = now + 60 * 24 * 3600 * 1_000_000;
        assert!(needs_renewal(&mk_row("a", Some(twenty_days_from_now)), now));
        assert!(!needs_renewal(&mk_row("b", Some(sixty_days_from_now)), now));
    }

    #[test]
    fn needs_renewal_when_cert_missing() {
        let row = mk_row("x", None);
        assert!(needs_renewal(&row, 1_700_000_000_000_000));
    }

    #[test]
    fn due_for_renewal_selects_only_expiring_or_missing() {
        let now = 1_700_000_000_000_000_i64;
        let rows = vec![
            mk_row("soon", Some(now + 20 * 24 * 3600 * 1_000_000)), // within 30d → due
            mk_row("later", Some(now + 60 * 24 * 3600 * 1_000_000)), // 60d out → not due
            mk_row("missing", None),                                // no cert → due
        ];
        let due: Vec<&str> = due_for_renewal(&rows, now)
            .iter()
            .map(|r| r.hostname.as_str())
            .collect();
        assert_eq!(due, vec!["soon", "missing"]);
    }
}
