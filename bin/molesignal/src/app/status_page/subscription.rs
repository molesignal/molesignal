// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Double-opt-in subscriptions and durable notification delivery processing.

use std::collections::HashMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

mod links;

use links::{confirmation_message, custom_page_url, delivery_message, platform_page_url};

use super::StatusPageService;
use crate::{
    domain::status_page::{
        StatusPage, StatusPageDeliveryPage, StatusPageDeliveryStatus, StatusPageLifecycle,
        StatusPageSubscriber, StatusPageSubscriberChannel, StatusPageSubscriberPage,
        StatusPageSubscriberStatus,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const CONFIRMATION_COOLDOWN_MICROS: i64 = 10 * 60 * 1_000_000;
const MAX_DELIVERY_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone)]
pub struct StatusPageSubscriptionInput {
    pub channel: StatusPageSubscriberChannel,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPageSubscriptionOutcome {
    pub accepted: bool,
    pub confirmation_required: bool,
}

impl StatusPageService {
    pub async fn subscribe_public(
        &self,
        slug: &str,
        input: StatusPageSubscriptionInput,
    ) -> Result<StatusPageSubscriptionOutcome> {
        let page = self.get_public_page(slug).await?;
        self.subscribe_to_page(page, input).await
    }

    pub async fn subscribe_private_email(
        &self,
        slug: &str,
        raw_session_token: &str,
        origin_host: &str,
    ) -> Result<StatusPageSubscriptionOutcome> {
        let page = self.get_customer_page(slug).await?;
        let session = self
            .authorize_customer_page(&page, Some(raw_session_token), origin_host)
            .await?
            .ok_or_else(|| Error::conflict("status page is not private"))?;
        self.subscribe_to_page(
            page,
            StatusPageSubscriptionInput {
                channel: StatusPageSubscriberChannel::Email,
                target: session.email,
            },
        )
        .await
    }

    async fn subscribe_to_page(
        &self,
        page: StatusPage,
        input: StatusPageSubscriptionInput,
    ) -> Result<StatusPageSubscriptionOutcome> {
        let sender = self.notification_sender()?;
        let target = normalize_target(input.channel, &input.target)?;
        sender.validate_target(input.channel, &target).await?;
        let token = subscription_token();
        let token_hash = token_hash(&token);
        let now = TimestampMicros::now();
        let pending = self
            .repository
            .upsert_pending_subscriber(
                StatusPageSubscriber {
                    id: Id::new(),
                    org_id: page.org_id.clone(),
                    status_page_id: page.id.clone(),
                    channel: input.channel,
                    target,
                    status: StatusPageSubscriberStatus::Pending,
                    token_hash: token_hash.clone(),
                    confirmation_sent_at: Some(now),
                    confirmed_at: None,
                    unsubscribed_at: None,
                    created_at: now,
                    updated_at: now,
                },
                TimestampMicros(now.0.saturating_sub(CONFIRMATION_COOLDOWN_MICROS)),
            )
            .await?;

        if pending.should_send_confirmation {
            let message = match self
                .notification_page_url(&page)
                .await
                .and_then(|page_url| confirmation_message(&page, &page_url, &token))
            {
                Ok(message) => message,
                Err(error) => {
                    self.repository
                        .reset_pending_confirmation(&pending.subscriber.id, &token_hash)
                        .await?;
                    return Err(error);
                }
            };
            if let Err(error) = sender
                .send(
                    pending.subscriber.channel,
                    &pending.subscriber.target,
                    &message,
                )
                .await
            {
                self.repository
                    .reset_pending_confirmation(&pending.subscriber.id, &token_hash)
                    .await?;
                return Err(error);
            }
        }

        Ok(StatusPageSubscriptionOutcome {
            accepted: true,
            confirmation_required: true,
        })
    }

    pub async fn confirm_public_subscription(
        &self,
        slug: &str,
        token: &str,
    ) -> Result<StatusPageSubscriptionOutcome> {
        let page = self.get_customer_page(slug).await?;
        validate_token(token)?;
        self.repository
            .confirm_subscriber(
                &page.org_id,
                &page.id,
                &token_hash(token),
                TimestampMicros::now(),
            )
            .await?;
        Ok(StatusPageSubscriptionOutcome {
            accepted: true,
            confirmation_required: false,
        })
    }

    pub async fn unsubscribe_public(
        &self,
        slug: &str,
        token: &str,
    ) -> Result<StatusPageSubscriptionOutcome> {
        let page = self.get_customer_page(slug).await?;
        validate_token(token)?;
        self.repository
            .unsubscribe_subscriber(
                &page.org_id,
                &page.id,
                &token_hash(token),
                TimestampMicros::now(),
            )
            .await?;
        Ok(StatusPageSubscriptionOutcome {
            accepted: true,
            confirmation_required: false,
        })
    }

    pub async fn list_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
    ) -> Result<StatusPageSubscriberPage> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .list_subscribers(org_id, page_id, page.max(1), 25)
            .await
    }

    pub async fn revoke_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
    ) -> Result<StatusPageSubscriber> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .revoke_subscriber(org_id, page_id, subscriber_id, TimestampMicros::now())
            .await
    }

    pub async fn resend_pending_confirmation(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
    ) -> Result<StatusPageSubscriber> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "subscription confirmations cannot be resent for an archived status page",
            ));
        }
        let existing = self
            .repository
            .get_subscriber(org_id, page_id, subscriber_id)
            .await?;
        if existing.status != StatusPageSubscriberStatus::Pending {
            return Err(Error::conflict(
                "only a pending subscription can resend confirmation",
            ));
        }
        let token = subscription_token();
        let now = TimestampMicros::now();
        let subscriber = self
            .repository
            .refresh_pending_confirmation(org_id, page_id, subscriber_id, &token_hash(&token), now)
            .await?;
        let message = match self
            .notification_page_url(&page)
            .await
            .and_then(|page_url| confirmation_message(&page, &page_url, &token))
        {
            Ok(message) => message,
            Err(error) => {
                self.repository
                    .reset_pending_confirmation(&subscriber.id, &subscriber.token_hash)
                    .await?;
                return Err(error);
            }
        };
        if let Err(error) = self
            .notification_sender()?
            .send(subscriber.channel, &subscriber.target, &message)
            .await
        {
            self.repository
                .reset_pending_confirmation(&subscriber.id, &subscriber.token_hash)
                .await?;
            return Err(error);
        }
        Ok(subscriber)
    }

    pub async fn list_notification_deliveries(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
    ) -> Result<StatusPageDeliveryPage> {
        self.repository.get_page(org_id, page_id).await?;
        self.repository
            .list_notification_deliveries(org_id, page_id, page.max(1), 25)
            .await
    }

    pub async fn process_pending_notifications(&self, limit: u32) -> Result<u32> {
        let sender = self.notification_sender()?;
        let now = TimestampMicros::now();
        let deliveries = self
            .repository
            .claim_notification_deliveries(now, limit.min(500))
            .await?;
        let count = u32::try_from(deliveries.len()).unwrap_or(u32::MAX);
        let mut page_urls: HashMap<Id, String> = HashMap::new();
        for delivery in deliveries {
            let page_url = if let Some(page_url) = page_urls.get(&delivery.status_page_id) {
                Ok(page_url.clone())
            } else {
                match self
                    .repository
                    .get_page(&delivery.org_id, &delivery.status_page_id)
                    .await
                {
                    Ok(page) => self.notification_page_url(&page).await.inspect(|page_url| {
                        page_urls.insert(delivery.status_page_id.clone(), page_url.clone());
                    }),
                    Err(error) => Err(error),
                }
            };
            let message = page_url.and_then(|page_url| delivery_message(&delivery, &page_url));
            let result = match message {
                Ok(message) => {
                    sender
                        .send(delivery.channel, &delivery.target, &message)
                        .await
                }
                Err(error) => Err(error),
            };
            let completed_at = TimestampMicros::now();
            let (status, next_attempt_at, last_error) = match result {
                Ok(()) => (StatusPageDeliveryStatus::Delivered, completed_at, None),
                Err(error) if delivery.attempts >= MAX_DELIVERY_ATTEMPTS => (
                    StatusPageDeliveryStatus::Failed,
                    completed_at,
                    Some(truncate_error(&error.to_string())),
                ),
                Err(error) => (
                    StatusPageDeliveryStatus::Pending,
                    TimestampMicros(
                        completed_at
                            .0
                            .saturating_add(retry_delay_micros(delivery.attempts)),
                    ),
                    Some(truncate_error(&error.to_string())),
                ),
            };
            if let Err(error) = self
                .repository
                .complete_notification_delivery(
                    &delivery.id,
                    status,
                    next_attempt_at,
                    last_error,
                    completed_at,
                )
                .await
            {
                tracing::warn!(
                    delivery_id = %delivery.id,
                    error = %error,
                    "status-page notification completion failed"
                );
            }
        }
        Ok(count)
    }

    pub async fn purge_terminal_notification_deliveries(&self, limit: u32) -> Result<u64> {
        self.repository
            .purge_terminal_notification_deliveries(TimestampMicros::now(), limit.min(1000))
            .await
    }

    fn notification_sender(
        &self,
    ) -> Result<&std::sync::Arc<dyn crate::domain::status_page::StatusPageNotificationSender>> {
        self.notification_sender.as_ref().ok_or_else(|| {
            Error::unavailable("status-page subscriber notifications are not configured")
        })
    }

    async fn notification_page_url(&self, page: &StatusPage) -> Result<String> {
        let domain = self
            .repository
            .get_domain_config(&page.org_id, &page.id)
            .await?;
        if let Some(domain) = domain.filter(|domain| {
            domain.state == crate::domain::status_page::StatusPageDomainState::Active
                && domain.routing_valid
        }) {
            return custom_page_url(&domain.hostname);
        }
        platform_page_url(&self.external_url, &page.slug)
    }
}

fn normalize_target(channel: StatusPageSubscriberChannel, raw: &str) -> Result<String> {
    let target = raw.trim();
    if !(3..=2048).contains(&target.len()) || target.chars().any(char::is_control) {
        return Err(Error::invalid("subscription target is invalid"));
    }
    Ok(match channel {
        StatusPageSubscriberChannel::Email => target.to_ascii_lowercase(),
        StatusPageSubscriberChannel::Webhook => target.to_string(),
    })
}

fn subscription_token() -> String {
    format!("{}.{}", Id::new(), Id::new())
}

fn validate_token(token: &str) -> Result<()> {
    if !(20..=160).contains(&token.len())
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err(Error::invalid("subscription token is invalid"));
    }
    Ok(())
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

const fn retry_delay_micros(attempt: i32) -> i64 {
    match attempt {
        0 | 1 => 60 * 1_000_000,
        2 => 5 * 60 * 1_000_000,
        3 => 30 * 60 * 1_000_000,
        _ => 2 * 60 * 60 * 1_000_000,
    }
}

fn truncate_error(value: &str) -> String {
    value.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_hash_only_and_validation_rejects_query_injection() {
        let token = subscription_token();
        assert_ne!(token_hash(&token), token);
        assert_eq!(token_hash(&token).len(), 64);
        assert!(validate_token(&token).is_ok());
        assert!(validate_token("bad&subscription_action=unsubscribe").is_err());
    }
}
