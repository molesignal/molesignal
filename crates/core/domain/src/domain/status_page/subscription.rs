// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Public subscribers and durable status-update delivery records.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageSubscriberChannel {
    Email,
    Webhook,
    /// A Slack incoming webhook: messages are written for Slack.
    Slack,
}

impl StatusPageSubscriberChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Webhook => "webhook",
            Self::Slack => "slack",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "email" => Some(Self::Email),
            "webhook" => Some(Self::Webhook),
            "slack" => Some(Self::Slack),
            _ => None,
        }
    }
}

/// Whether a subscriber hears about the whole page or the components it chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriberComponentScope {
    All,
    Selected,
}

impl SubscriberComponentScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Selected => "selected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "selected" => Some(Self::Selected),
            _ => None,
        }
    }
}

/// What a subscriber hears about. A subscriber of the whole page hears about
/// components added later too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriberComponents {
    #[serde(rename = "component_scope")]
    pub scope: SubscriberComponentScope,
    pub component_ids: Vec<Id>,
}

impl SubscriberComponents {
    pub fn all() -> Self {
        Self {
            scope: SubscriberComponentScope::All,
            component_ids: Vec::new(),
        }
    }

    /// The components chosen; none chosen means the whole page.
    pub fn chosen(component_ids: Vec<Id>) -> Self {
        if component_ids.is_empty() {
            return Self::all();
        }

        Self {
            scope: SubscriberComponentScope::Selected,
            component_ids,
        }
    }

    /// Whether an event that names these components reaches the subscriber.
    pub fn hears(&self, named: &[Id]) -> bool {
        self.scope == SubscriberComponentScope::All
            || named.iter().any(|id| self.component_ids.contains(id))
    }
}

/// How a subscriber came to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageSubscriberOrigin {
    /// The page's own subscription form.
    Page,
    /// A list an administrator imported.
    Import,
    /// "Add to Slack".
    SlackApp,
}

impl StatusPageSubscriberOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Import => "import",
            Self::SlackApp => "slack_app",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "page" => Some(Self::Page),
            "import" => Some(Self::Import),
            "slack_app" => Some(Self::SlackApp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageSubscriberStatus {
    Pending,
    Active,
    Unsubscribed,
}

impl StatusPageSubscriberStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Unsubscribed => "unsubscribed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "unsubscribed" => Some(Self::Unsubscribed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageSubscriber {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub channel: StatusPageSubscriberChannel,
    pub target: String,
    /// The Slack channel and workspace the Slack app was added to.
    pub target_label: Option<String>,
    pub status: StatusPageSubscriberStatus,
    #[serde(skip_serializing)]
    pub token_hash: String,
    /// The token itself, which the links of every message carry. It is kept
    /// sealed and never leaves the backend except inside those links.
    #[serde(skip)]
    pub token: String,
    #[serde(flatten)]
    pub components: SubscriberComponents,
    pub origin: StatusPageSubscriberOrigin,
    /// When the last confirmation, or the last link to manage an active
    /// subscription, was sent.
    pub confirmation_sent_at: Option<TimestampMicros>,
    pub confirmed_at: Option<TimestampMicros>,
    pub unsubscribed_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// What a request to subscribe leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionRequestAction {
    /// Send a confirmation, which activates the subscription.
    Confirm,
    /// The address is already subscribed: send it the link that manages the
    /// subscription, where the components it hears about can be changed.
    Manage,
    /// A message went out a short while ago; send nothing.
    Nothing,
}

#[derive(Debug, Clone)]
pub struct PendingStatusPageSubscription {
    pub subscriber: StatusPageSubscriber,
    pub action: SubscriptionRequestAction,
}

#[derive(Debug, Clone)]
pub struct StatusPageSubscriberPage {
    pub items: Vec<StatusPageSubscriber>,
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusPageDeliveryStatus {
    Pending,
    Processing,
    Delivered,
    Failed,
}

impl StatusPageDeliveryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "processing" => Some(Self::Processing),
            "delivered" => Some(Self::Delivered),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatusPageNotificationDelivery {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub subscriber_id: Id,
    pub channel: StatusPageSubscriberChannel,
    pub target: String,
    /// The subscriber's token, for the links that manage or end the
    /// subscription.
    pub subscriber_token: String,
    pub subscriber_components: SubscriberComponents,
    pub event_key: String,
    pub payload: Value,
    pub status: StatusPageDeliveryStatus,
    pub attempts: i32,
    pub next_attempt_at: TimestampMicros,
    pub last_error: Option<String>,
    pub delivered_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct StatusPageDeliveryPage {
    pub items: Vec<StatusPageNotificationDelivery>,
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct StatusPageNotificationMessage {
    pub title: String,
    pub text: String,
    pub payload: Value,
}

#[async_trait]
pub trait StatusPageNotificationSender: Send + Sync {
    /// Whether this installation can deliver to the channel at all, such as
    /// email only with an SMTP server configured.
    fn delivers(&self, _channel: StatusPageSubscriberChannel) -> bool {
        true
    }

    async fn validate_target(
        &self,
        channel: StatusPageSubscriberChannel,
        target: &str,
    ) -> Result<()>;

    async fn send(
        &self,
        channel: StatusPageSubscriberChannel,
        target: &str,
        message: &StatusPageNotificationMessage,
    ) -> Result<()>;
}

#[async_trait]
pub trait StatusPageSubscriberRepository: Send + Sync {
    /// Records a request to subscribe `subscriber.target`. A new or lapsed
    /// subscriber becomes pending with the token and components given and is
    /// sent a confirmation; an active one keeps its token and components and is
    /// sent the link that manages them. Nothing is sent while the last message
    /// went out after `resend_before`.
    async fn upsert_pending_subscriber(
        &self,
        subscriber: StatusPageSubscriber,
        resend_before: TimestampMicros,
    ) -> Result<PendingStatusPageSubscription>;

    async fn confirm_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        confirmed_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;

    async fn reset_pending_confirmation(&self, subscriber_id: &Id, token_hash: &str) -> Result<()>;

    async fn unsubscribe_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        unsubscribed_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;

    async fn list_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
        per_page: u32,
    ) -> Result<StatusPageSubscriberPage>;

    async fn get_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
    ) -> Result<StatusPageSubscriber>;

    async fn revoke_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;

    /// Gives a pending subscriber a new token, `token` with its `token_hash`.
    async fn refresh_pending_confirmation(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
        token: &str,
        token_hash: &str,
        sent_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;

    /// The pending or active subscriber the token belongs to.
    async fn get_subscriber_by_token(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
    ) -> Result<StatusPageSubscriber>;

    /// Changes what a pending or active subscriber hears about.
    async fn update_subscriber_components(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        components: &SubscriberComponents,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;

    async fn import_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        import: &super::StatusPageSubscriberImport,
    ) -> Result<super::StatusPageSubscriberImportOutcome>;
}

/// "Add to Slack": the subscriber waits, pending with a placeholder target and
/// the OAuth state as its token, until the visitor comes back from Slack.
#[async_trait]
pub trait StatusPageSlackAppRepository: Send + Sync {
    async fn begin_slack_app_subscription(
        &self,
        subscriber: StatusPageSubscriber,
    ) -> Result<StatusPageSubscriber>;

    /// The subscriber waiting with the state `state_hash`, begun after
    /// `begun_after`.
    async fn waiting_slack_app_subscription(
        &self,
        state_hash: &str,
        begun_after: TimestampMicros,
    ) -> Result<Option<StatusPageSubscriber>>;

    /// Gives the waiting subscriber the incoming webhook Slack created and a
    /// new token, and activates it.
    async fn complete_slack_app_subscription(
        &self,
        waiting: &StatusPageSubscriber,
        completion: SlackAppCompletion,
    ) -> Result<StatusPageSubscriber>;

    /// Removes a waiting subscriber whose visitor did not add Slack.
    async fn cancel_slack_app_subscription(&self, waiting: &StatusPageSubscriber) -> Result<()>;

    /// Removes the subscribers of "Add to Slack" requests begun before
    /// `begun_before` that never came back.
    async fn purge_abandoned_slack_app_subscriptions(
        &self,
        begun_before: TimestampMicros,
    ) -> Result<u64>;
}

/// The Slack apps of an organization: Slack App message connectors. A status
/// page names the one its visitors add it to Slack with.
#[async_trait]
pub trait StatusPageSlackApps: Send + Sync {
    /// The Slack app of the connector named. Fails unless it is a Slack App
    /// connector of the organization that is turned on and has a client ID and
    /// a client secret.
    async fn slack_app(
        &self,
        org_id: &Id,
        connector_id: &Id,
    ) -> Result<std::sync::Arc<dyn StatusPageSlackApp>>;

    /// The Slack App connectors of the organization, by name.
    async fn connectors(&self, org_id: &Id) -> Result<Vec<SlackAppConnectorChoice>>;

    /// The redirect URL to register in the Slack app; `None` until the
    /// installation has an HTTPS public address.
    fn redirect_url(&self) -> Option<String>;
}

/// A Slack App connector, as the settings of a page offer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlackAppConnectorChoice {
    pub id: Id,
    pub name: String,
    pub enabled: bool,
    /// Whether it holds a client ID and secret, which adding channels needs.
    pub adds_channels: bool,
}

/// A Slack app through which a visitor adds a status page to a channel of
/// their own workspace.
#[async_trait]
pub trait StatusPageSlackApp: Send + Sync {
    /// Where the visitor chooses the channel; Slack comes back to the fixed
    /// callback of this installation with `state`.
    fn authorize_url(&self, state: &str) -> Result<String>;

    /// Exchanges the code Slack sent back for the incoming webhook it created.
    async fn exchange_code(&self, code: &str) -> Result<SlackIncomingWebhook>;
}

/// The incoming webhook Slack created for the channel a visitor chose.
#[derive(Debug, Clone)]
pub struct SlackIncomingWebhook {
    pub url: String,
    pub channel: String,
    pub team: String,
}

/// What Slack returned for an "Add to Slack" request.
#[derive(Debug, Clone)]
pub struct SlackAppCompletion {
    pub webhook_url: String,
    pub label: String,
    pub token: String,
    pub token_hash: String,
    pub completed_at: TimestampMicros,
}

#[async_trait]
pub trait StatusPageDeliveryRepository: Send + Sync {
    async fn list_notification_deliveries(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
        per_page: u32,
    ) -> Result<StatusPageDeliveryPage>;

    async fn claim_notification_deliveries(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPageNotificationDelivery>>;

    async fn complete_notification_delivery(
        &self,
        delivery_id: &Id,
        status: StatusPageDeliveryStatus,
        next_attempt_at: TimestampMicros,
        last_error: Option<String>,
        updated_at: TimestampMicros,
    ) -> Result<()>;

    async fn purge_terminal_notification_deliveries(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u64>;
}

pub trait StatusPageSubscriptionRepository:
    StatusPageSubscriberRepository + StatusPageDeliveryRepository + StatusPageSlackAppRepository
{
}

impl<T> StatusPageSubscriptionRepository for T where
    T: StatusPageSubscriberRepository + StatusPageDeliveryRepository + StatusPageSlackAppRepository
{
}
