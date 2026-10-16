// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Double-opt-in subscriptions to a whole page or to the components a visitor
//! chose. Every message carries the links that manage or end the
//! subscription; an address that asks again while subscribed is sent the link
//! that manages it.

use std::collections::HashSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

mod delivery;
mod import;
mod links;
mod preferences;
mod slack_app;
mod slack_message;

pub use import::{StatusPageImportProblem, StatusPageSubscriberImportReport};
use links::{SubscriptionMessage, subscription_message};
pub use slack_app::StatusPageSlackAppSetting;

use super::StatusPageService;
use crate::{
    domain::status_page::{
        ComponentLifecycle, ComponentVisibility, StatusPage, StatusPageComponent,
        StatusPageDeliveryPage, StatusPageLifecycle, StatusPageSubscriber,
        StatusPageSubscriberChannel, StatusPageSubscriberOrigin, StatusPageSubscriberPage,
        StatusPageSubscriberStatus, SubscriberComponents, SubscriptionRequestAction,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const CONFIRMATION_COOLDOWN_MICROS: i64 = 10 * 60 * 1_000_000;

/// The most components a subscriber can choose one by one.
const MAX_CHOSEN_COMPONENTS: usize = 500;

#[derive(Debug, Clone)]
pub struct StatusPageSubscriptionInput {
    pub channel: StatusPageSubscriberChannel,
    pub target: String,
    /// The components to hear about; none means the whole page.
    pub component_ids: Vec<Id>,
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
        component_ids: Vec<Id>,
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
                component_ids,
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
        let components = self
            .subscriber_components(&page, input.component_ids)
            .await?;
        let token = subscription_token();
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
                    target_label: None,
                    status: StatusPageSubscriberStatus::Pending,
                    token_hash: token_hash(&token),
                    token,
                    components,
                    origin: StatusPageSubscriberOrigin::Page,
                    confirmation_sent_at: Some(now),
                    confirmed_at: None,
                    unsubscribed_at: None,
                    created_at: now,
                    updated_at: now,
                },
                TimestampMicros(now.0.saturating_sub(CONFIRMATION_COOLDOWN_MICROS)),
            )
            .await?;

        let message = match pending.action {
            SubscriptionRequestAction::Confirm => Some(SubscriptionMessage::Confirm),
            SubscriptionRequestAction::Manage => Some(SubscriptionMessage::Manage),
            SubscriptionRequestAction::Nothing => None,
        };

        if let Some(message) = message {
            self.send_subscription_message(&page, &pending.subscriber, message)
                .await?;
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

        if existing.status != StatusPageSubscriberStatus::Pending
            || existing.origin == StatusPageSubscriberOrigin::SlackApp
        {
            return Err(Error::conflict(
                "only a pending subscription can resend confirmation",
            ));
        }

        let token = subscription_token();

        let subscriber = self
            .repository
            .refresh_pending_confirmation(
                org_id,
                page_id,
                subscriber_id,
                &token,
                &token_hash(&token),
                TimestampMicros::now(),
            )
            .await?;

        self.send_subscription_message(&page, &subscriber, SubscriptionMessage::Confirm)
            .await?;

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

    /// What a subscriber who chose `component_ids` hears about. Only the
    /// components the page shows can be chosen.
    pub(super) async fn subscriber_components(
        &self,
        page: &StatusPage,
        component_ids: Vec<Id>,
    ) -> Result<SubscriberComponents> {
        if component_ids.is_empty() {
            return Ok(SubscriberComponents::all());
        }

        let mut seen = HashSet::new();

        let chosen: Vec<Id> = component_ids
            .into_iter()
            .filter(|id| seen.insert(id.as_str().to_string()))
            .collect();

        if chosen.len() > MAX_CHOSEN_COMPONENTS {
            return Err(Error::invalid(
                "a subscription can choose at most 500 components",
            ));
        }

        let shown = self.shown_components(page).await?;

        if chosen
            .iter()
            .any(|id| !shown.iter().any(|component| &component.id == id))
        {
            return Err(Error::invalid(
                "a subscription can only choose components the page shows",
            ));
        }

        Ok(SubscriberComponents::chosen(chosen))
    }

    /// The components the page shows, in its order.
    pub(super) async fn shown_components(
        &self,
        page: &StatusPage,
    ) -> Result<Vec<StatusPageComponent>> {
        Ok(self
            .repository
            .list_components(&page.org_id, &page.id)
            .await?
            .into_iter()
            .filter(|component| {
                // A group has no status of its own: subscribers name its components.
                !component.is_group()
                    && component.visibility == ComponentVisibility::Enabled
                    && component.lifecycle == ComponentLifecycle::Active
            })
            .collect())
    }

    /// Sends a confirmation, or the link that manages an active subscription,
    /// straight away. A message that cannot be sent lets the visitor ask again
    /// at once.
    async fn send_subscription_message(
        &self,
        page: &StatusPage,
        subscriber: &StatusPageSubscriber,
        kind: SubscriptionMessage,
    ) -> Result<()> {
        let sent: Result<()> = async {
            let page_url = self.notification_page_url(page).await?;

            let pack = match subscriber.channel {
                StatusPageSubscriberChannel::Webhook => None,
                StatusPageSubscriberChannel::Email | StatusPageSubscriberChannel::Slack => {
                    Some(self.language_pack(&page.language).await?)
                }
            };

            let names = self.component_names(page, &subscriber.components).await?;

            let message =
                subscription_message(kind, page, &page_url, subscriber, &names, pack.as_ref())?;

            self.notification_sender()?
                .send(subscriber.channel, &subscriber.target, &message)
                .await
        }
        .await;

        if let Err(error) = sent {
            self.repository
                .reset_pending_confirmation(&subscriber.id, &subscriber.token_hash)
                .await?;

            return Err(error);
        }

        Ok(())
    }

    /// The names of the components a subscriber chose, in page order; none
    /// for a subscriber of the whole page.
    pub(super) async fn component_names(
        &self,
        page: &StatusPage,
        components: &SubscriberComponents,
    ) -> Result<Vec<String>> {
        if components.component_ids.is_empty() {
            return Ok(Vec::new());
        }

        Ok(self
            .shown_components(page)
            .await?
            .into_iter()
            .filter(|component| components.component_ids.contains(&component.id))
            .map(|component| component.name)
            .collect())
    }
}

fn normalize_target(channel: StatusPageSubscriberChannel, raw: &str) -> Result<String> {
    let target = raw.trim();

    if !(3..=2048).contains(&target.len()) || target.chars().any(char::is_control) {
        return Err(Error::invalid("subscription target is invalid"));
    }

    Ok(match channel {
        StatusPageSubscriberChannel::Email => target.to_ascii_lowercase(),
        StatusPageSubscriberChannel::Webhook | StatusPageSubscriberChannel::Slack => {
            target.to_string()
        }
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

    #[test]
    fn email_targets_are_lowercased_and_urls_kept_as_given() {
        assert_eq!(
            normalize_target(StatusPageSubscriberChannel::Email, " Ops@Example.COM ").unwrap(),
            "ops@example.com"
        );
        assert_eq!(
            normalize_target(
                StatusPageSubscriberChannel::Slack,
                "https://hooks.slack.com/services/T0/B0/AbC"
            )
            .unwrap(),
            "https://hooks.slack.com/services/T0/B0/AbC"
        );
        assert!(normalize_target(StatusPageSubscriberChannel::Webhook, "a\nb").is_err());
    }
}
