// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! "Add to Slack": a visitor of a public page adds it to a channel of their
//! own Slack workspace through the Slack App connector the page names. Choosing
//! the channel in Slack is the consent, so no confirmation follows.

use std::sync::Arc;

use serde::Serialize;

use super::{
    super::StatusPageService,
    links::{page_action_url, slack_welcome_message},
    subscription_token, token_hash, validate_token,
};
use crate::{
    domain::status_page::{
        SlackAppCompletion, SlackAppConnectorChoice, SlackIncomingWebhook, StatusPage,
        StatusPageLifecycle, StatusPageSlackApp, StatusPageSlackApps, StatusPageSubscriber,
        StatusPageSubscriberChannel, StatusPageSubscriberOrigin, StatusPageSubscriberStatus,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// How long a visitor has to choose a channel in Slack.
const SLACK_APP_WAIT_MICROS: i64 = 10 * 60 * 1_000_000;

/// When a request that never came back from Slack is removed.
const ABANDONED_SLACK_APP_MICROS: i64 = 60 * 60 * 1_000_000;

/// The Slack setting of a page: the Slack App connector it names, the ones it
/// can name, and the redirect URL they need in Slack.
#[derive(Debug, Clone, Serialize)]
pub struct StatusPageSlackAppSetting {
    pub connector_id: Option<Id>,
    pub redirect_url: Option<String>,
    pub connectors: Vec<SlackAppConnectorChoice>,
}

impl StatusPageService {
    pub async fn page_slack_app_setting(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<StatusPageSlackAppSetting> {
        let page = self.repository.get_page(org_id, page_id).await?;
        self.slack_app_setting(&page).await
    }

    /// Chooses the Slack App connector visitors add the page to their channels
    /// with, or stops offering Slack with `None`. Channels already added keep
    /// receiving updates: they post to their own incoming webhooks.
    pub async fn set_page_slack_app(
        &self,
        org_id: &Id,
        page_id: &Id,
        connector_id: Option<Id>,
    ) -> Result<StatusPageSlackAppSetting> {
        let page = self.repository.get_page(org_id, page_id).await?;

        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict("an archived status page cannot be updated"));
        }

        if let Some(connector_id) = &connector_id {
            self.slack_apps()?.slack_app(org_id, connector_id).await?;
        }

        let page = self
            .repository
            .set_page_slack_connector(
                org_id,
                page_id,
                connector_id.as_ref(),
                TimestampMicros::now(),
            )
            .await?;

        self.slack_app_setting(&page).await
    }

    /// Whether visitors of the page can add it to Slack: it names a Slack App
    /// connector that is on and holds a client ID and secret.
    pub async fn slack_app_available(&self, page: &StatusPage) -> bool {
        page.slack_connector_id.is_some() && self.page_slack_app(page).await.is_ok()
    }

    /// Starts "Add to Slack" on a public page: returns where in Slack the
    /// visitor chooses the channel.
    pub async fn begin_slack_app_subscription(
        &self,
        slug: &str,
        component_ids: Vec<Id>,
    ) -> Result<String> {
        let page = self.get_public_page(slug).await?;
        let slack_app = self.page_slack_app(&page).await?;
        let components = self.subscriber_components(&page, component_ids).await?;
        let state = subscription_token();
        let now = TimestampMicros::now();

        self.repository
            .begin_slack_app_subscription(StatusPageSubscriber {
                id: Id::new(),
                org_id: page.org_id.clone(),
                status_page_id: page.id.clone(),
                channel: StatusPageSubscriberChannel::Slack,
                target: format!("slack-app:{}", Id::new()),
                target_label: None,
                status: StatusPageSubscriberStatus::Pending,
                token_hash: token_hash(&state),
                token: state.clone(),
                components,
                origin: StatusPageSubscriberOrigin::SlackApp,
                confirmation_sent_at: Some(now),
                confirmed_at: None,
                unsubscribed_at: None,
                created_at: now,
                updated_at: now,
            })
            .await?;

        slack_app.authorize_url(&state)
    }

    /// Finishes "Add to Slack" when Slack sends the visitor back, and returns
    /// the page address to send them on to. A visitor who did not add Slack
    /// goes back to the page too, which says so.
    pub async fn complete_slack_app_subscription(
        &self,
        state: &str,
        code: Option<&str>,
    ) -> Result<String> {
        validate_token(state)?;

        let now = TimestampMicros::now();

        let waiting = self
            .repository
            .waiting_slack_app_subscription(
                &token_hash(state),
                TimestampMicros(now.0.saturating_sub(SLACK_APP_WAIT_MICROS)),
            )
            .await?
            .ok_or_else(|| {
                Error::invalid("this Slack request has expired; start again from the status page")
            })?;

        let page = self
            .repository
            .get_page(&waiting.org_id, &waiting.status_page_id)
            .await?;

        let page_url = self.notification_page_url(&page).await?;

        let webhook = match code {
            Some(code) => match self.page_slack_app(&page).await {
                Ok(slack_app) => slack_app.exchange_code(code).await,
                Err(error) => Err(error),
            },
            None => Err(Error::invalid("Slack was not added")),
        };

        let webhook = match webhook {
            Ok(webhook) => webhook,
            Err(error) => {
                tracing::info!(
                    status_page_id = %page.id,
                    error = %error,
                    "a visitor did not finish adding a status page to Slack"
                );

                self.repository
                    .cancel_slack_app_subscription(&waiting)
                    .await?;

                return page_action_url(&page_url, "slack_failed");
            }
        };

        let token = subscription_token();

        let subscriber = self
            .repository
            .complete_slack_app_subscription(
                &waiting,
                SlackAppCompletion {
                    label: channel_label(&webhook),
                    webhook_url: webhook.url,
                    token_hash: token_hash(&token),
                    token,
                    completed_at: now,
                },
            )
            .await?;

        if let Err(error) = self
            .welcome_slack_channel(&page, &page_url, &subscriber)
            .await
        {
            tracing::warn!(
                status_page_id = %page.id,
                error = %error,
                "the Slack channel a status page was added to could not be greeted"
            );
        }

        page_action_url(&page_url, "slack_connected")
    }

    pub async fn purge_abandoned_slack_app_subscriptions(&self) -> Result<u64> {
        let before = TimestampMicros::now()
            .0
            .saturating_sub(ABANDONED_SLACK_APP_MICROS);

        self.repository
            .purge_abandoned_slack_app_subscriptions(TimestampMicros(before))
            .await
    }

    async fn slack_app_setting(&self, page: &StatusPage) -> Result<StatusPageSlackAppSetting> {
        let slack_apps = self.slack_apps()?;

        Ok(StatusPageSlackAppSetting {
            connector_id: page.slack_connector_id.clone(),
            redirect_url: slack_apps.redirect_url(),
            connectors: slack_apps.connectors(&page.org_id).await?,
        })
    }

    fn slack_apps(&self) -> Result<&Arc<dyn StatusPageSlackApps>> {
        self.slack_apps
            .as_ref()
            .ok_or_else(|| Error::unavailable("Slack apps are not configured"))
    }

    /// The Slack app of the connector the page names.
    async fn page_slack_app(&self, page: &StatusPage) -> Result<Arc<dyn StatusPageSlackApp>> {
        let connector_id = page
            .slack_connector_id
            .as_ref()
            .ok_or_else(|| Error::unavailable("this status page cannot be added to Slack"))?;

        self.slack_apps()?
            .slack_app(&page.org_id, connector_id)
            .await
    }

    async fn welcome_slack_channel(
        &self,
        page: &StatusPage,
        page_url: &str,
        subscriber: &StatusPageSubscriber,
    ) -> Result<()> {
        let pack = self.language_pack(&page.language).await?;
        let names = self.component_names(page, &subscriber.components).await?;
        let message = slack_welcome_message(page, page_url, subscriber, &names, &pack)?;

        self.notification_sender()?
            .send(
                StatusPageSubscriberChannel::Slack,
                &subscriber.target,
                &message,
            )
            .await
    }
}

/// How the channel is named in the list of subscribers: `#ops · Acme`.
fn channel_label(webhook: &SlackIncomingWebhook) -> String {
    let channel = webhook.channel.trim();

    let channel = if channel.starts_with('#') || channel.is_empty() {
        channel.to_string()
    } else {
        format!("#{channel}")
    };

    let label = match webhook.team.trim() {
        "" => channel,
        team => format!("{channel} · {team}"),
    };

    label.chars().take(255).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_is_named_with_its_workspace() {
        let webhook = |channel: &str, team: &str| SlackIncomingWebhook {
            url: "https://hooks.slack.com/services/T/B/C".into(),
            channel: channel.into(),
            team: team.into(),
        };

        assert_eq!(channel_label(&webhook("ops", "Acme")), "#ops · Acme");
        assert_eq!(channel_label(&webhook("#status", "")), "#status");
    }
}
