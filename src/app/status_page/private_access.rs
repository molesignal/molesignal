// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use base64::Engine as _;
use rand::TryRng as _;
use serde::Serialize;
use url::Url;

use super::{
    MICROS_PER_DAY, StatusPageAccessRuleInput, StatusPageService,
    validation::normalize_domain_lookup,
};
use crate::{
    domain::status_page::{
        StatusPage, StatusPageAccessRule, StatusPageAccessSession, StatusPageDomainState,
        StatusPageMagicLinkExchange, StatusPageNotificationMessage, StatusPageSubscriberChannel,
        StatusPageVisibility,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAGIC_LINK_TTL_MICROS: i64 = 15 * 60 * 1_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct StatusPageAccessRequestOutcome {
    pub accepted: bool,
}

#[derive(Debug, Clone)]
pub struct StatusPageAccessSessionOutcome {
    pub page: StatusPage,
    pub session: StatusPageAccessSession,
    pub raw_session_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPageAccessMetadata {
    pub name: String,
    pub slug: String,
    pub logo_url: Option<String>,
    pub brand_color: String,
    pub language: String,
    pub languages: Vec<String>,
    pub visibility: StatusPageVisibility,
}

impl StatusPageService {
    pub async fn access_metadata(&self, slug: &str) -> Result<StatusPageAccessMetadata> {
        let page = self.get_customer_page(slug).await?;
        Ok(StatusPageAccessMetadata {
            name: page.name,
            slug: page.slug,
            logo_url: page.logo_url,
            brand_color: page.brand_color,
            language: page.language,
            languages: page.languages,
            visibility: page.visibility,
        })
    }

    pub async fn access_cookie_name_for_slug(&self, slug: &str) -> Result<String> {
        let page = self.get_customer_page(slug).await?;
        Ok(access_cookie_name(&page.id))
    }

    pub async fn create_access_rule(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: StatusPageAccessRuleInput,
    ) -> Result<StatusPageAccessRule> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != crate::domain::status_page::StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "access rules cannot be changed on an archived status page",
            ));
        }
        let value = normalize_access_rule(input.kind, &input.value)?;
        let now = TimestampMicros::now();
        self.repository
            .create_access_rule(StatusPageAccessRule {
                id: Id::new(),
                org_id: org_id.clone(),
                status_page_id: page_id.clone(),
                kind: input.kind,
                value,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    pub async fn list_access_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<StatusPageAccessRule>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository.list_access_rules(org_id, page_id).await
    }

    pub async fn delete_access_rule(&self, org_id: &Id, page_id: &Id, rule_id: &Id) -> Result<()> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != crate::domain::status_page::StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "access rules cannot be changed on an archived status page",
            ));
        }
        self.repository
            .delete_access_rule(org_id, page_id, rule_id)
            .await
    }

    pub async fn request_private_access(
        &self,
        slug: &str,
        email: &str,
        requested_origin: &str,
    ) -> Result<StatusPageAccessRequestOutcome> {
        let page = self.get_customer_page(slug).await?;
        if page.visibility != StatusPageVisibility::Private {
            return Err(Error::conflict("status page is not private"));
        }
        let origin_host = self.allowed_origin_host(&page, requested_origin).await?;
        let email = normalize_email(email)?;
        let domain = email
            .rsplit_once('@')
            .map(|(_, domain)| domain)
            .ok_or_else(|| Error::invalid("email is invalid"))?;
        let Some(rule) = self
            .repository
            .find_access_rule_for_email(&page.org_id, &page.id, &email, domain)
            .await?
        else {
            return Ok(StatusPageAccessRequestOutcome { accepted: true });
        };
        let raw_token = opaque_token(32);
        let now = TimestampMicros::now();
        let created = self
            .repository
            .create_magic_link(crate::domain::status_page::StatusPageMagicLink {
                id: Id::new(),
                org_id: page.org_id.clone(),
                status_page_id: page.id.clone(),
                access_rule_id: rule.id,
                email: email.clone(),
                token_hash: token_hash(&raw_token),
                origin_host: origin_host.clone(),
                expires_at: TimestampMicros(now.0.saturating_add(MAGIC_LINK_TTL_MICROS)),
                consumed_at: None,
                created_at: now,
            })
            .await?;
        if !created {
            return Ok(StatusPageAccessRequestOutcome { accepted: true });
        }
        let Some(sender) = self.notification_sender.as_ref() else {
            tracing::error!(
                status_page_id = %page.id,
                "private status-page access email could not be sent because notifications are not configured"
            );
            return Ok(StatusPageAccessRequestOutcome { accepted: true });
        };
        let access_url = match self.magic_link_url(&page, &origin_host, &raw_token) {
            Ok(url) => url,
            Err(error) => {
                tracing::error!(status_page_id = %page.id, error = %error, "private status-page access URL could not be built");
                return Ok(StatusPageAccessRequestOutcome { accepted: true });
            }
        };
        if let Err(error) = sender
            .send(
                StatusPageSubscriberChannel::Email,
                &email,
                &StatusPageNotificationMessage {
                    title: format!("Access {}", page.name),
                    text: format!(
                        "Use this one-time link within 15 minutes to access {name}:\n{url}",
                        name = page.name,
                        url = access_url
                    ),
                    payload: serde_json::json!({
                        "type": "status_page_magic_link",
                        "page_slug": page.slug,
                        "access_url": access_url,
                    }),
                },
            )
            .await
        {
            tracing::error!(status_page_id = %page.id, error = %error, "private status-page access email delivery failed");
        }
        Ok(StatusPageAccessRequestOutcome { accepted: true })
    }

    pub async fn consume_private_access(
        &self,
        slug: &str,
        raw_token: &str,
        requested_origin: &str,
    ) -> Result<StatusPageAccessSessionOutcome> {
        validate_access_token(raw_token)?;
        let page = self.get_customer_page(slug).await?;
        if page.visibility != StatusPageVisibility::Private {
            return Err(Error::conflict("status page is not private"));
        }
        let origin_host = self.allowed_origin_host(&page, requested_origin).await?;
        let raw_session_token = opaque_token(32);
        let now = TimestampMicros::now();
        let consumed = self
            .repository
            .consume_magic_link(StatusPageMagicLinkExchange {
                page_id: page.id.clone(),
                token_hash: token_hash(raw_token),
                origin_host,
                session_id: Id::new(),
                session_token_hash: token_hash(&raw_session_token),
                session_expires_at: TimestampMicros(now.0.saturating_add(
                    i64::from(page.private_session_days).saturating_mul(MICROS_PER_DAY),
                )),
                now,
            })
            .await?;
        Ok(StatusPageAccessSessionOutcome {
            page,
            session: consumed.session,
            raw_session_token,
        })
    }

    pub async fn get_customer_snapshot(
        &self,
        slug: &str,
        raw_session_token: Option<&str>,
        requested_origin: &str,
    ) -> Result<crate::domain::status_page::StatusPageSnapshot> {
        let page = self.get_customer_page(slug).await?;
        self.authorize_customer_page(&page, raw_session_token, requested_origin)
            .await?;
        self.build_snapshot(page, true).await
    }

    pub async fn get_customer_snapshot_by_domain(
        &self,
        domain: &str,
        raw_session_token: Option<&str>,
    ) -> Result<Option<crate::domain::status_page::StatusPageSnapshot>> {
        let Some(page) = self.resolve_customer_page_by_domain(domain).await? else {
            return Ok(None);
        };
        self.authorize_customer_page(&page, raw_session_token, domain)
            .await?;
        self.build_snapshot(page, true).await.map(Some)
    }

    pub(super) async fn authorize_customer_page(
        &self,
        page: &StatusPage,
        raw_session_token: Option<&str>,
        requested_origin: &str,
    ) -> Result<Option<StatusPageAccessSession>> {
        if page.visibility == StatusPageVisibility::Public {
            return Ok(None);
        }
        let raw_session_token = raw_session_token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| Error::unauthorized("private status page access is required"))?;
        let origin_host = self.allowed_origin_host(page, requested_origin).await?;
        self.repository
            .find_access_session(
                &page.id,
                &token_hash(raw_session_token),
                &origin_host,
                TimestampMicros::now(),
            )
            .await?
            .map(Some)
            .ok_or_else(|| Error::unauthorized("private status page session is invalid"))
    }

    pub async fn list_access_sessions(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<StatusPageAccessSession>> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .list_access_sessions(org_id, page_id, TimestampMicros::now())
            .await
    }

    pub async fn revoke_access_session(
        &self,
        org_id: &Id,
        page_id: &Id,
        session_id: &Id,
    ) -> Result<()> {
        self.repository
            .revoke_access_session(org_id, page_id, session_id, TimestampMicros::now())
            .await
    }

    pub async fn revoke_all_access_sessions(&self, org_id: &Id, page_id: &Id) -> Result<u64> {
        self.repository
            .revoke_all_access_sessions(org_id, page_id, TimestampMicros::now())
            .await
    }

    pub async fn purge_expired_access_artifacts(&self, limit: u32) -> Result<u64> {
        self.repository
            .purge_expired_access_artifacts(TimestampMicros::now(), limit.min(1000))
            .await
    }

    async fn allowed_origin_host(
        &self,
        page: &StatusPage,
        requested_origin: &str,
    ) -> Result<String> {
        let requested = requested_origin
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let platform = Url::parse(self.external_url.trim().trim_end_matches('/'))
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .ok_or_else(|| Error::unavailable("http.external_url is not configured"))?;
        if requested.is_empty() || requested == platform {
            return Ok(platform);
        }
        if normalize_domain_lookup(&requested).is_some() {
            let custom = self
                .repository
                .get_domain_config(&page.org_id, &page.id)
                .await?;
            if custom.is_some_and(|config| {
                config.hostname == requested
                    && matches!(
                        config.state,
                        StatusPageDomainState::Active | StatusPageDomainState::Degraded
                    )
            }) {
                return Ok(requested);
            }
        }
        Err(Error::unauthorized(
            "request host is not allowed for this private status page",
        ))
    }

    fn magic_link_url(
        &self,
        page: &StatusPage,
        origin_host: &str,
        raw_token: &str,
    ) -> Result<String> {
        let platform = Url::parse(self.external_url.trim().trim_end_matches('/'))
            .map_err(|_| Error::unavailable("http.external_url is not configured"))?;
        let platform_host = platform.host_str().unwrap_or_default();
        let mut url = if origin_host == platform_host {
            platform
                .join(&format!("/status/{}", page.slug))
                .map_err(|_| Error::internal("status-page access URL could not be built"))?
        } else {
            Url::parse(&format!("https://{origin_host}/"))
                .map_err(|_| Error::internal("custom-domain access URL could not be built"))?
        };
        url.query_pairs_mut().append_pair("access_token", raw_token);
        Ok(url.to_string())
    }
}

pub fn access_cookie_name(page_id: &Id) -> String {
    let digest = blake3::hash(page_id.as_str().as_bytes()).to_hex();
    format!("ms_sp_{}", &digest.as_str()[..16])
}

pub fn token_hash(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}

pub(super) fn opaque_token(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::rngs::SysRng
        .try_fill_bytes(&mut value)
        .expect("operating-system random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
}

fn validate_access_token(token: &str) -> Result<()> {
    if !(32..=160).contains(&token.len())
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::invalid("status-page access token is invalid"));
    }
    Ok(())
}

fn normalize_email(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    let Some((local, domain)) = value.rsplit_once('@') else {
        return Err(Error::invalid("email is invalid"));
    };
    if local.is_empty()
        || local.len() > 64
        || normalize_domain_lookup(domain).is_none()
        || value.len() > 254
        || value.chars().any(char::is_control)
    {
        return Err(Error::invalid("email is invalid"));
    }
    Ok(value)
}

fn normalize_access_rule(
    kind: crate::domain::status_page::StatusPageAccessRuleKind,
    value: &str,
) -> Result<String> {
    match kind {
        crate::domain::status_page::StatusPageAccessRuleKind::Email => normalize_email(value),
        crate::domain::status_page::StatusPageAccessRuleKind::Domain => {
            let domain = value.trim().trim_start_matches('@').to_ascii_lowercase();
            normalize_domain_lookup(&domain)
                .map(|domain| format!("@{domain}"))
                .ok_or_else(|| Error::invalid("access-rule domain is invalid"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status_page::StatusPageAccessRuleKind;

    #[test]
    fn access_rules_are_exact_and_normalized() {
        assert_eq!(
            normalize_access_rule(StatusPageAccessRuleKind::Email, " User@Example.com ").unwrap(),
            "user@example.com"
        );
        assert_eq!(
            normalize_access_rule(StatusPageAccessRuleKind::Domain, "@Example.com").unwrap(),
            "@example.com"
        );
        assert!(normalize_access_rule(StatusPageAccessRuleKind::Domain, "*.example.com").is_err());
    }

    #[test]
    fn page_cookie_names_are_stable_and_scoped() {
        let a = access_cookie_name(&Id("page-a".into()));
        let b = access_cookie_name(&Id("page-b".into()));
        assert_ne!(a, b);
        assert!(a.starts_with("ms_sp_"));
    }
}
