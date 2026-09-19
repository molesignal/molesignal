// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;

use super::{
    StatusPageDomainInput, StatusPageService, private_access::opaque_token,
    validation::normalize_custom_domain,
};
use crate::{
    domain::status_page::{
        StatusPageDomainCheckUpdate, StatusPageDomainConfig, StatusPageDomainHealthAlert,
        StatusPageDomainState, StatusPageLifecycle,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const VERIFY_COOLDOWN_MICROS: i64 = 30 * 1_000_000;
const BACKGROUND_INTERVAL_MICROS: i64 = 5 * 60 * 1_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct StatusPageDomainInstructions {
    pub config: StatusPageDomainConfig,
    pub txt_name: String,
    pub txt_value: String,
    pub routing_target: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StatusPageDomainCapability {
    pub available: bool,
}

impl StatusPageService {
    pub async fn get_custom_domain_capability(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<StatusPageDomainCapability> {
        self.repository.get_page(org_id, page_id).await?;
        Ok(StatusPageDomainCapability {
            available: self.domain_verifier.is_some() && self.managed_domain_tls_configured,
        })
    }

    pub async fn configure_custom_domain(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: StatusPageDomainInput,
    ) -> Result<StatusPageDomainInstructions> {
        if self.domain_verifier.is_none() {
            return Err(Error::unavailable(
                "status-page domain verification is not configured",
            ));
        }
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "an archived status page cannot configure a custom domain",
            ));
        }
        let hostname = normalize_custom_domain(Some(input.hostname))?
            .ok_or_else(|| Error::invalid("custom domain is required"))?;
        if let Some(existing) = self.repository.get_domain_config(org_id, page_id).await?
            && existing.hostname == hostname
        {
            return self.domain_instructions(existing);
        }
        let now = TimestampMicros::now();
        let config = self
            .repository
            .create_domain_config(StatusPageDomainConfig {
                org_id: org_id.clone(),
                status_page_id: page_id.clone(),
                hostname,
                verification_token: format!("msv_{}", opaque_token(24)),
                state: StatusPageDomainState::PendingDns,
                domain_id: None,
                routing_valid: false,
                last_checked_at: None,
                last_error: None,
                cert_not_after: None,
                last_alerted_state: None,
                last_alerted_at: None,
                created_at: now,
                updated_at: now,
            })
            .await?;
        self.domain_instructions(config)
    }

    pub async fn get_custom_domain(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<StatusPageDomainInstructions>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .get_domain_config(org_id, page_id)
            .await?
            .map(|config| self.domain_instructions(config))
            .transpose()
    }

    pub async fn verify_custom_domain(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<StatusPageDomainInstructions> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "an archived status page cannot verify a custom domain",
            ));
        }
        let config = self
            .repository
            .get_domain_config(org_id, page_id)
            .await?
            .ok_or_else(|| Error::not_found("custom domain is not configured"))?;
        if config.last_checked_at.is_some_and(|checked| {
            TimestampMicros::now().0.saturating_sub(checked.0) < VERIFY_COOLDOWN_MICROS
        }) {
            return Err(Error::conflict(
                "custom domain verification is cooling down; try again shortly",
            ));
        }
        let config = self.check_domain(config).await?;
        self.domain_instructions(config)
    }

    pub async fn verify_domains_due(&self, limit: u32) -> Result<u64> {
        let now = TimestampMicros::now();
        let due = self
            .repository
            .list_domain_configs_due(
                TimestampMicros(now.0.saturating_sub(BACKGROUND_INTERVAL_MICROS)),
                limit.min(100),
            )
            .await?;
        let count = u64::try_from(due.len()).unwrap_or(u64::MAX);
        for config in due {
            if let Err(error) = self.check_domain(config.clone()).await {
                tracing::warn!(
                    status_page_id = %config.status_page_id,
                    error = %error,
                    "status-page domain verification failed"
                );
            }
        }
        Ok(count)
    }

    async fn check_domain(&self, config: StatusPageDomainConfig) -> Result<StatusPageDomainConfig> {
        let verifier = self.domain_verifier.as_ref().ok_or_else(|| {
            Error::unavailable("status-page domain verification is not configured")
        })?;
        let now = TimestampMicros::now();
        self.repository
            .record_domain_check(StatusPageDomainCheckUpdate {
                org_id: config.org_id.clone(),
                status_page_id: config.status_page_id.clone(),
                state: StatusPageDomainState::Verifying,
                routing_valid: config.routing_valid,
                last_error: None,
                checked_at: now,
                provision_tls: false,
            })
            .await?;
        let check = match verifier
            .verify(&config.hostname, &config.verification_token)
            .await
        {
            Ok(check) => check,
            Err(error) => {
                let failed_state = if matches!(
                    config.state,
                    StatusPageDomainState::Active | StatusPageDomainState::Degraded
                ) {
                    StatusPageDomainState::Degraded
                } else {
                    StatusPageDomainState::Failed
                };
                let updated = self
                    .repository
                    .record_domain_check(StatusPageDomainCheckUpdate {
                        org_id: config.org_id.clone(),
                        status_page_id: config.status_page_id.clone(),
                        state: failed_state,
                        routing_valid: config.routing_valid,
                        last_error: Some(error.to_string()),
                        checked_at: now,
                        provision_tls: false,
                    })
                    .await?;
                self.notify_domain_health_transition(&updated).await;
                return Ok(updated);
            }
        };
        let (state, provision_tls) = if check.ownership_verified && check.routing_valid {
            (StatusPageDomainState::Verified, config.domain_id.is_none())
        } else if matches!(
            config.state,
            StatusPageDomainState::Active | StatusPageDomainState::Degraded
        ) {
            (StatusPageDomainState::Degraded, false)
        } else {
            (StatusPageDomainState::PendingDns, false)
        };
        let updated = self
            .repository
            .record_domain_check(StatusPageDomainCheckUpdate {
                org_id: config.org_id.clone(),
                status_page_id: config.status_page_id.clone(),
                state,
                routing_valid: check.routing_valid,
                last_error: check.error,
                checked_at: now,
                provision_tls,
            })
            .await?;
        self.notify_domain_health_transition(&updated).await;
        Ok(updated)
    }

    async fn notify_domain_health_transition(&self, config: &StatusPageDomainConfig) {
        let unhealthy = matches!(
            config.state,
            StatusPageDomainState::Failed | StatusPageDomainState::Degraded
        );
        if !unhealthy {
            if config.last_alerted_state.is_some() {
                let _ = self
                    .repository
                    .mark_domain_health_alerted(
                        &config.org_id,
                        &config.status_page_id,
                        None,
                        TimestampMicros::now(),
                    )
                    .await;
            }
            return;
        }
        if config.last_alerted_state == Some(config.state) {
            return;
        }
        let Some(notifier) = self.domain_health_notifier.as_ref() else {
            tracing::warn!(
                status_page_id = %config.status_page_id,
                hostname = %config.hostname,
                state = config.state.as_str(),
                "status-page domain is unhealthy but administrator notifications are unavailable"
            );
            return;
        };
        let page = match self
            .repository
            .get_page(&config.org_id, &config.status_page_id)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!(error = %error, "status-page domain alert page lookup failed");
                return;
            }
        };
        let alert = StatusPageDomainHealthAlert {
            org_id: config.org_id.clone(),
            status_page_id: config.status_page_id.clone(),
            page_name: page.name,
            hostname: config.hostname.clone(),
            state: config.state,
            error: config.last_error.clone(),
        };
        match notifier.notify(&alert).await {
            Ok(()) => {
                let _ = self
                    .repository
                    .mark_domain_health_alerted(
                        &config.org_id,
                        &config.status_page_id,
                        Some(config.state),
                        TimestampMicros::now(),
                    )
                    .await;
            }
            Err(error) => tracing::warn!(
                status_page_id = %config.status_page_id,
                error = %error,
                "status-page domain administrator notification failed"
            ),
        }
    }

    pub async fn retry_custom_domain_tls(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<StatusPageDomainInstructions> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "an archived status page cannot retry custom-domain TLS",
            ));
        }
        let config = self
            .repository
            .retry_domain_tls(org_id, page_id, TimestampMicros::now())
            .await?;
        self.domain_instructions(config)
    }

    pub async fn remove_custom_domain(&self, org_id: &Id, page_id: &Id) -> Result<()> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "an archived status page cannot remove a custom domain",
            ));
        }
        self.repository.delete_domain_config(org_id, page_id).await
    }

    fn domain_instructions(
        &self,
        config: StatusPageDomainConfig,
    ) -> Result<StatusPageDomainInstructions> {
        let verifier = self.domain_verifier.as_ref().ok_or_else(|| {
            Error::unavailable("status-page domain verification is not configured")
        })?;
        Ok(StatusPageDomainInstructions {
            txt_name: format!("_molesignal-verification.{}", config.hostname),
            txt_value: config.verification_token.clone(),
            routing_target: verifier.routing_target().to_string(),
            config,
        })
    }
}
