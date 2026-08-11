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
}

impl StatusPageSubscriberChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Webhook => "webhook",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "email" => Some(Self::Email),
            "webhook" => Some(Self::Webhook),
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
    pub status: StatusPageSubscriberStatus,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub confirmation_sent_at: Option<TimestampMicros>,
    pub confirmed_at: Option<TimestampMicros>,
    pub unsubscribed_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct PendingStatusPageSubscription {
    pub subscriber: StatusPageSubscriber,
    pub should_send_confirmation: bool,
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

    async fn refresh_pending_confirmation(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
        token_hash: &str,
        sent_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber>;
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
    StatusPageSubscriberRepository + StatusPageDeliveryRepository
{
}

impl<T> StatusPageSubscriptionRepository for T where
    T: StatusPageSubscriberRepository + StatusPageDeliveryRepository
{
}
