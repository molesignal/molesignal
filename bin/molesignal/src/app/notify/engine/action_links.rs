// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One-time action links on alert notifications sent to one person by connectors that draw no
//! buttons (see `domain::notify::action_links`): an open alert gets an acknowledge and a resolve
//! link, an acknowledged one a resolve link. The words are the button words in the recipient's
//! language.

use std::sync::{Arc, atomic::Ordering};

use domain::notify::{
    action_links::{
        ACTION_LINK_TTL_MICROS, ActionGrant, ActionLink, ActionLinks, NotifyActionLinkRepository,
    },
    buttons::{AlertAction, AlertButtons},
    connector::{ConnectorAdapter, NotifyChannel, NotifyMessage, NotifyTargetType},
    policy::NotifyEvent,
    recipient::NotifyRecipient,
    reply::ReplyAction,
};
use kernel::{ids::Id, time::TimestampMicros};

use super::{NotifyEngine, model::ResolvedRoute};
use crate::app::notify::action_links::{action_token_hash, action_url, new_action_token};

/// How often expired links are removed: at most once an hour per node.
const PURGE_EVERY_MICROS: i64 = 60 * 60 * 1_000_000;

impl NotifyEngine {
    /// Where issued action links are kept; without it no links are issued.
    pub fn with_action_links(mut self, links: Arc<dyn NotifyActionLinkRepository>) -> Self {
        self.action_links = Some(links);
        self
    }

    /// Adds the action links when this route takes them; otherwise returns the message as is.
    pub(super) async fn with_action_links_for(
        &self,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
        route: &ResolvedRoute,
        adapter: &dyn ConnectorAdapter,
        mut message: NotifyMessage,
    ) -> NotifyMessage {
        let (Some(repository), Some(source)) = (&self.action_links, self.alert_buttons.source())
        else {
            return message;
        };

        let takes_links = route.target.target_type == NotifyTargetType::DirectUser
            && matches!(adapter.channel(), NotifyChannel::Im | NotifyChannel::Email)
            && AlertButtons::read(&message).is_none();

        let actions = linkable_actions(event);

        if !takes_links || actions.is_empty() {
            return message;
        }

        let Some(labels) = source
            .link_labels(&event.organization_id, &recipient.user_id)
            .await
        else {
            return message;
        };

        let Some(alert_id) = event.attributes["alert_id"].as_str() else {
            return message;
        };

        let now = TimestampMicros::now();
        let mut links = ActionLinks::default();

        for action in actions {
            let Ok(token) = new_action_token() else {
                return message;
            };

            let Some(url) = action_url(&self.external_url, &token) else {
                return message;
            };

            let grant = ActionGrant {
                organization_id: event.organization_id.clone(),
                recipient_user_id: recipient.user_id.clone(),
                alert_id: Id::from_string(alert_id),
                action,
                created_at: now,
                expires_at: TimestampMicros(now.0.saturating_add(ACTION_LINK_TTL_MICROS)),
                used_at: None,
            };

            if let Err(error) = repository.record(&action_token_hash(&token), &grant).await {
                tracing::warn!(%error, "an action link could not be recorded; sending without links");
                return message;
            }

            match action {
                ReplyAction::Acknowledge => {
                    links.acknowledge = Some(ActionLink {
                        label: labels.acknowledge.clone(),
                        url,
                    });
                }
                ReplyAction::Resolve => {
                    links.resolve = Some(ActionLink {
                        label: labels.resolve.clone(),
                        url,
                    });
                }
            }
        }

        self.purge_expired_links(repository.as_ref(), now).await;

        links.apply(&mut message);
        message
    }

    /// Removes expired links, at most once an hour.
    async fn purge_expired_links(
        &self,
        repository: &dyn NotifyActionLinkRepository,
        now: TimestampMicros,
    ) {
        let last = self.action_links_purged_at.load(Ordering::Relaxed);

        if now.0.saturating_sub(last) < PURGE_EVERY_MICROS
            || self
                .action_links_purged_at
                .compare_exchange(last, now.0, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }

        if let Err(error) = repository.purge_expired(now).await {
            tracing::debug!(%error, "expired action links could not be purged");
        }
    }
}

/// What the alert in an alert notification can still have done to it by a link.
fn linkable_actions(event: &NotifyEvent) -> Vec<ReplyAction> {
    if !event.event_type.starts_with("alert.") {
        return Vec::new();
    }

    let Some(status) = event.attributes["status"].as_str() else {
        return Vec::new();
    };

    AlertAction::available(status)
        .iter()
        .filter_map(|action| match action {
            AlertAction::Acknowledge => Some(ReplyAction::Acknowledge),
            AlertAction::Resolve => Some(ReplyAction::Resolve),
            AlertAction::Snooze
            | AlertAction::AcknowledgeAll
            | AlertAction::Approve
            | AlertAction::Reject => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(event_type: &str, status: &str) -> NotifyEvent {
        NotifyEvent {
            id: "event-1".into(),
            event_type: event_type.into(),
            organization_id: Id::from_string("org-1"),
            occurred_at: TimestampMicros(0),
            attributes: json!({ "alert_id": "incident-1", "status": status }),
        }
    }

    #[test]
    fn an_open_alert_can_be_acknowledged_or_resolved_and_an_acknowledged_one_resolved() {
        assert_eq!(
            linkable_actions(&event("alert.triggered", "open")),
            [ReplyAction::Acknowledge, ReplyAction::Resolve]
        );
        assert_eq!(
            linkable_actions(&event("alert.acknowledged", "acknowledged")),
            [ReplyAction::Resolve]
        );
        assert!(linkable_actions(&event("alert.resolved", "resolved")).is_empty());
        assert!(linkable_actions(&event("oncall.shift_starting", "open")).is_empty());
    }
}
