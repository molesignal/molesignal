// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Sending the queued messages to subscribers, with retries.

use std::collections::HashMap;

use super::{
    super::StatusPageService,
    links::{NotificationPage, custom_page_url, delivery_message, platform_page_url},
};
use crate::{
    domain::{
        i18n_language::LanguagePack,
        status_page::{
            StatusPage, StatusPageDeliveryStatus, StatusPageDomainState,
            StatusPageNotificationDelivery, StatusPageNotificationMessage,
            StatusPagePublicationState, StatusPageSubscriberChannel,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_DELIVERY_ATTEMPTS: i32 = 5;

/// What a sweep reads once: each page, the copy of each language, and the
/// text of each postmortem announced.
#[derive(Default)]
struct SweepCache {
    pages: HashMap<Id, NotificationPage>,
    packs: HashMap<String, LanguagePack>,
    postmortems: HashMap<(Id, Id), Option<String>>,
}

impl StatusPageService {
    pub async fn process_pending_notifications(&self, limit: u32) -> Result<u32> {
        let sender = self.notification_sender()?;
        let now = TimestampMicros::now();

        let deliveries = self
            .repository
            .claim_notification_deliveries(now, limit.min(500))
            .await?;

        let count = u32::try_from(deliveries.len()).unwrap_or(u32::MAX);
        let mut cache = SweepCache::default();

        for delivery in deliveries {
            let result = match self.write_delivery(&delivery, &mut cache).await {
                Ok(Some(message)) => {
                    sender
                        .send(delivery.channel, &delivery.target, &message)
                        .await
                }
                // Nothing is left to tell: the postmortem went back to a draft.
                Ok(None) => Err(Error::conflict("the postmortem is no longer published")),
                Err(error) => Err(error),
            };

            let completed_at = TimestampMicros::now();

            let (status, next_attempt_at, last_error) = match result {
                Ok(()) => (StatusPageDeliveryStatus::Delivered, completed_at, None),
                Err(error)
                    if delivery.attempts >= MAX_DELIVERY_ATTEMPTS
                        || matches!(error, Error::Conflict(_)) =>
                {
                    (
                        StatusPageDeliveryStatus::Failed,
                        completed_at,
                        Some(truncate_error(&error.to_string())),
                    )
                }
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

    /// Writes the message of one delivery; `None` when what it announces is
    /// gone.
    async fn write_delivery(
        &self,
        delivery: &StatusPageNotificationDelivery,
        cache: &mut SweepCache,
    ) -> Result<Option<StatusPageNotificationMessage>> {
        let page = match cache.pages.get(&delivery.status_page_id) {
            Some(page) => page.clone(),
            None => {
                let page = self
                    .repository
                    .get_page(&delivery.org_id, &delivery.status_page_id)
                    .await?;

                let page = self.notification_page(page).await?;
                cache
                    .pages
                    .insert(delivery.status_page_id.clone(), page.clone());
                page
            }
        };

        let postmortem = match announced_postmortem(delivery) {
            Some(incident_id) => {
                let key = (delivery.status_page_id.clone(), incident_id.clone());

                if !cache.postmortems.contains_key(&key) {
                    let text = self.published_postmortem(delivery, &incident_id).await?;
                    cache.postmortems.insert(key.clone(), text);
                }

                match cache.postmortems.get(&key).cloned().flatten() {
                    Some(text) => Some(text),
                    None => return Ok(None),
                }
            }
            None => None,
        };

        // A webhook receives the payload alone; an email and a Slack message
        // are written from the copy.
        let pack = match delivery.channel {
            StatusPageSubscriberChannel::Webhook => None,
            StatusPageSubscriberChannel::Email | StatusPageSubscriberChannel::Slack => {
                if !cache.packs.contains_key(&page.language) {
                    let pack = self.language_pack(&page.language).await?;
                    cache.packs.insert(page.language.clone(), pack);
                }

                cache.packs.get(&page.language)
            }
        };

        delivery_message(delivery, &page, pack, postmortem.as_deref()).map(Some)
    }

    async fn published_postmortem(
        &self,
        delivery: &StatusPageNotificationDelivery,
        incident_id: &Id,
    ) -> Result<Option<String>> {
        let incident = self
            .repository
            .get_status_incident(&delivery.org_id, &delivery.status_page_id, incident_id)
            .await?;

        Ok(incident
            .postmortem
            .filter(|postmortem| {
                postmortem.publication_state == StatusPagePublicationState::Published
            })
            .map(|postmortem| postmortem.body))
    }

    pub(super) async fn notification_page(&self, page: StatusPage) -> Result<NotificationPage> {
        let url = self.notification_page_url(&page).await?;

        let components = self
            .shown_components(&page)
            .await?
            .into_iter()
            .map(|component| (component.id, component.name))
            .collect();

        Ok(NotificationPage {
            url,
            timezone: page.timezone,
            language: page.language,
            components,
        })
    }

    pub(in crate::app::status_page) async fn notification_page_url(
        &self,
        page: &StatusPage,
    ) -> Result<String> {
        let domain = self
            .repository
            .get_domain_config(&page.org_id, &page.id)
            .await?;

        if let Some(domain) = domain
            .filter(|domain| domain.state == StatusPageDomainState::Active && domain.routing_valid)
        {
            return custom_page_url(&domain.hostname);
        }

        platform_page_url(&self.external_url, &page.slug)
    }
}

/// The incident whose postmortem a delivery announces.
fn announced_postmortem(delivery: &StatusPageNotificationDelivery) -> Option<Id> {
    let payload = &delivery.payload;

    (payload.get("type").and_then(serde_json::Value::as_str) == Some("postmortem_published"))
        .then(|| {
            payload
                .get("incident_id")
                .and_then(serde_json::Value::as_str)
        })
        .flatten()
        .map(|id| Id(id.to_string()))
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
    fn retries_slow_down_and_errors_stay_bounded() {
        assert!(retry_delay_micros(1) < retry_delay_micros(2));
        assert!(retry_delay_micros(3) < retry_delay_micros(9));
        assert_eq!(truncate_error(&"x".repeat(900)).len(), 500);
    }
}
