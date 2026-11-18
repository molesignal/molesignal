// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent in IM: receives messages and answers through a notify app connector (Feishu, Slack,
//! DingTalk, WeCom smart bot, Google Chat, or Microsoft Teams).
//!
//! - Bot ([`ImBot`]): an app connector with `Answer in IM` enabled. The bot receives messages over
//!   a persistent connection (a public callback for Teams), with only one node connected at a time
//!   (via a lease); status is written back to the bot.
//! - Thread ([`ImThread`]): a group thread, a person's conversation in a group (DingTalk), or a
//!   direct conversation, mapped to a session. `/new` and `/scope` switch to a new session.
//! - Message ([`ImMessage`]): each received message, deduplicated by the platform's message ID;
//!   records the Run when an answer run is queued and replies to the original location on
//!   completion.
//! - Link ([`ImLink`]): sends a one-time link privately when an IM account is not yet matched to a
//!   MoleSignal user, for confirmation after login.
//! - Outgoing reply ([`ImOutgoing`]): when replies can only be sent by the node connected to the
//!   bot (WeCom), other nodes record replies here for the connected node to send.
//! - Alert card button ([`CardAction`]): Feishu and Slack alert notifications include acknowledge
//!   and resolve buttons. Clicks arrive over the persistent connection and produce a message for
//!   the person who clicked and a replacement card ([`CardAnswer`]).
//!
//! Only verified notification identities are recognized: the IM account must match an enabled,
//! verified notification identity. No answer is sent if it matches multiple people.

pub mod commands;

use async_trait::async_trait;
use domain::notify::{
    buttons::{AlertButtonValue, AlertButtons},
    connector::NotifyMessage,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Platforms that support replies in IM, identified by connector type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImPlatform {
    Feishu,
    Slack,
    DingTalk,
    WeCom,
    GoogleChat,
    Teams,
    Telegram,
}

impl ImPlatform {
    pub const ALL: [Self; 7] = [
        Self::Feishu,
        Self::Slack,
        Self::DingTalk,
        Self::WeCom,
        Self::GoogleChat,
        Self::Teams,
        Self::Telegram,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Slack => "slack",
            Self::DingTalk => "dingtalk",
            Self::WeCom => "wecom",
            Self::GoogleChat => "google_chat",
            Self::Teams => "teams",
            Self::Telegram => "telegram",
        }
    }

    /// Whether this connector type supports replies in IM.
    pub fn of_connector(connector_type: &str) -> Option<Self> {
        match connector_type {
            "lark_app" => Some(Self::Feishu),
            "slack_app" => Some(Self::Slack),
            "dingtalk_app" => Some(Self::DingTalk),
            "wecom_bot" => Some(Self::WeCom),
            "google_chat_app" => Some(Self::GoogleChat),
            "teams_app" => Some(Self::Teams),
            "telegram_bot" => Some(Self::Telegram),
            _ => None,
        }
    }

    /// The human-readable platform name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Feishu => "Feishu",
            Self::Slack => "Slack",
            Self::DingTalk => "DingTalk",
            Self::WeCom => "WeCom",
            Self::GoogleChat => "Google Chat",
            Self::Teams => "Microsoft Teams",
            Self::Telegram => "Telegram",
        }
    }
}

/// The current state of the bot's persistent connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotStatus {
    /// Disabled, or no node has connected yet.
    #[default]
    Idle,
    Connecting,
    Connected,
    Error,
}

impl BotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "idle" => Ok(Self::Idle),
            "connecting" => Ok(Self::Connecting),
            "connected" => Ok(Self::Connected),
            "error" => Ok(Self::Error),
            other => Err(Error::internal(format!("unknown IM bot status: {other}"))),
        }
    }
}

/// An app connector with `Answer in IM` enabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImBot {
    pub connector_id: Id,
    pub org_id: Id,
    pub enabled: bool,
    /// New group threads become this team's team sessions when the asker is a member; if empty,
    /// they become the asker's personal sessions.
    pub default_team_id: Option<Id>,
    pub status: BotStatus,
    /// The reason the connection fails, or the connected node.
    pub status_detail: Option<String>,
    pub status_at: Option<TimestampMicros>,
    pub last_message_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// Whether the message comes from a direct conversation or a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Direct,
    Group,
}

/// The session mapped to an IM thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImThread {
    pub id: Id,
    pub org_id: Id,
    pub connector_id: Id,
    /// The group or direct conversation.
    pub conversation_id: String,
    /// The thread: its root message in a group, or the asker in a DingTalk group; empty for direct
    /// conversations.
    pub thread_key: String,
    pub chat_id: Id,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The sender's identity on the platform, matched against notification identities in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sender {
    /// Platform IDs, such as Feishu's open_id, user_id, and union_id; the first is the person's
    /// primary ID.
    pub ids: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Sender {
    pub fn primary(&self) -> &str {
        self.ids.first().map(String::as_str).unwrap_or_default()
    }
}

/// A received message with the bot's @ mention already removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inbound {
    pub message_id: String,
    pub conversation_id: String,
    pub kind: ConversationKind,
    pub thread_key: String,
    pub sender: Sender,
    pub text: String,
    /// Whether the bot is @ mentioned in a group; always true in direct conversations.
    pub addressed: bool,
    /// Platform-specific data needed to reply to the original location.
    pub reply: Value,
}

/// A button click on an alert card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardAction {
    pub value: AlertButtonValue,
    pub sender: Sender,
    pub conversation_id: String,
    /// Treats the conversation as a group if the platform does not specify whether it is direct:
    /// sends the linking URL privately.
    pub kind: ConversationKind,
    /// Data needed to reply where the card is located, as in [`Inbound::reply`].
    pub reply: Value,
    /// Platform-specific data needed to replace the card: a Feishu message ID or Slack
    /// `response_url`.
    pub card: Value,
}

/// The tone of the message to the person who clicks the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Info,
    Warning,
    Error,
}

impl NoticeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// The result of a button click: a message for the person who clicked (a Feishu notification or a
/// Slack message visible only to that person) and the replacement card.
#[derive(Debug, Clone)]
pub struct CardAnswer {
    pub notice: Option<(NoticeKind, String)>,
    pub card: Option<AlertCard>,
}

/// The replacement alert card: a notification rewritten to reflect the alert's current state,
/// buttons still available to click, and a line stating who did what.
#[derive(Debug, Clone)]
pub struct AlertCard {
    pub message: NotifyMessage,
    pub buttons: AlertButtons,
    pub note: Option<String>,
}

/// The current processing stage of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// Received and still being processed.
    Received,
    /// An answer run is queued, awaiting completion.
    Answering,
    /// Replied.
    Replied,
    /// No answer needed: the bot is not @ mentioned, the message is a command, or another reply
    /// has already been sent.
    Ignored,
    /// The reply cannot be sent.
    Failed,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Answering => "answering",
            Self::Replied => "replied",
            Self::Ignored => "ignored",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "received" => Ok(Self::Received),
            "answering" => Ok(Self::Answering),
            "replied" => Ok(Self::Replied),
            "ignored" => Ok(Self::Ignored),
            "failed" => Ok(Self::Failed),
            other => Err(Error::internal(format!(
                "unknown IM message status: {other}"
            ))),
        }
    }
}

/// A received message and its reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImMessage {
    pub id: Id,
    pub org_id: Id,
    pub connector_id: Id,
    pub external_message_id: String,
    pub conversation_id: String,
    pub thread_key: String,
    pub sender_id: String,
    pub user_id: Option<Id>,
    pub chat_id: Option<Id>,
    pub run_id: Option<Id>,
    pub reply: Value,
    /// The ID of the `Processing` reaction, removed after replying.
    pub reaction_id: Option<String>,
    pub status: MessageStatus,
    pub detail: Option<String>,
    pub created_at: TimestampMicros,
    pub replied_at: Option<TimestampMicros>,
}

impl ImMessage {
    pub fn received(org_id: &Id, connector_id: &Id, inbound: &Inbound) -> Self {
        Self {
            id: Id::new(),
            org_id: org_id.clone(),
            connector_id: connector_id.clone(),
            external_message_id: inbound.message_id.clone(),
            conversation_id: inbound.conversation_id.clone(),
            thread_key: inbound.thread_key.clone(),
            sender_id: inbound.sender.primary().to_string(),
            user_id: None,
            chat_id: None,
            run_id: None,
            reply: inbound.reply.clone(),
            reaction_id: None,
            status: MessageStatus::Received,
            detail: None,
            created_at: TimestampMicros::now(),
            replied_at: None,
        }
    }
}

/// A one-time linking URL sent privately to an IM user for confirmation after logging in to
/// MoleSignal, registering the IM account as that user's verified notification identity. Only the
/// token's digest is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImLink {
    pub token_hash: String,
    pub org_id: Id,
    pub connector_id: Id,
    pub external_identity: String,
    pub display_name: Option<String>,
    pub expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

/// How long the linking URL remains valid.
pub const LINK_LIFETIME_MICROS: i64 = 15 * 60 * 1_000_000;

/// A reply waiting for the node connected to the bot to send it, replacing `Processing` with the
/// answer or replying to the original location. It can also be a notification sent proactively by
/// the bot (`notification`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImOutgoing {
    pub id: Id,
    pub org_id: Id,
    pub connector_id: Id,
    /// The message this reply settles; its status is written back after the reply is sent.
    pub message_id: Option<Id>,
    /// Where to send the reply, as in [`Inbound::reply`].
    pub reply: Value,
    /// The `Processing` message to replace.
    pub working: Option<String>,
    pub markdown: String,
    /// A proactive bot notification with a platform message body that includes its destination;
    /// when present, `reply` and `markdown` are ignored.
    #[serde(default)]
    pub notification: Option<Value>,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait ImRepository: Send + Sync {
    async fn list_bots(&self, org_id: &Id) -> Result<Vec<ImBot>>;
    async fn get_bot(&self, org_id: &Id, connector_id: &Id) -> Result<Option<ImBot>>;
    /// Finds a bot by connector, regardless of organization: public callbacks only know the
    /// connector ID.
    async fn find_bot(&self, connector_id: &Id) -> Result<Option<ImBot>>;
    /// Creates or updates bot settings (without changing its status or lease).
    async fn put_bot(&self, bot: &ImBot) -> Result<ImBot>;
    async fn delete_bot(&self, org_id: &Id, connector_id: &Id) -> Result<()>;
    /// Enabled bots across all organizations, for the worker.
    async fn enabled_bots(&self) -> Result<Vec<ImBot>>;
    /// Acquires or renews a bot's lease; returns false if another node holds an unexpired lease.
    async fn lease_bot(
        &self,
        connector_id: &Id,
        node: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool>;
    async fn release_bot(&self, connector_id: &Id, node: &str) -> Result<()>;
    /// Writes the bot's status; only the `node` holding the lease can write it (during handoff, the
    /// old node cannot overwrite the new node's status).
    async fn set_bot_status(
        &self,
        connector_id: &Id,
        node: &str,
        status: BotStatus,
        detail: Option<&str>,
    ) -> Result<()>;
    async fn touch_bot(&self, connector_id: &Id, at: TimestampMicros) -> Result<()>;

    async fn thread(
        &self,
        connector_id: &Id,
        conversation_id: &str,
        thread_key: &str,
    ) -> Result<Option<ImThread>>;
    /// Records the session associated with a thread; replaces any existing association with this
    /// session.
    async fn put_thread(&self, thread: &ImThread) -> Result<ImThread>;

    /// Records a received message; returns false if the same message from the same connector is
    /// already recorded.
    async fn record_message(&self, message: &ImMessage) -> Result<bool>;
    async fn update_message(&self, message: &ImMessage) -> Result<()>;
    /// Moves a message from `from` to `to`; returns false if someone else moves it first (the reply
    /// is sent only once).
    async fn move_message(&self, id: &Id, from: MessageStatus, to: MessageStatus) -> Result<bool>;
    /// Records the status, reason, and time when a reply is sent (or cannot be sent).
    async fn settle_message(
        &self,
        id: &Id,
        status: MessageStatus,
        detail: Option<&str>,
        replied_at: TimestampMicros,
    ) -> Result<()>;
    async fn message_for_run(&self, run_id: &Id) -> Result<Option<ImMessage>>;
    /// Messages received before `before` that still await answers; the worker supplies replies if
    /// the node is gone.
    async fn answering(&self, before: TimestampMicros, limit: i64) -> Result<Vec<ImMessage>>;
    /// Purges messages and unsent replies from before `before`.
    async fn purge_messages(&self, before: TimestampMicros) -> Result<u64>;

    /// Records a reply awaiting delivery by the node connected to the bot.
    async fn enqueue_outgoing(&self, outgoing: &ImOutgoing) -> Result<()>;
    /// Takes a bot's earliest pending replies (removing them so other nodes cannot take them).
    async fn take_outgoing(&self, connector_id: &Id, limit: i64) -> Result<Vec<ImOutgoing>>;

    async fn create_link(&self, link: &ImLink) -> Result<()>;
    /// Takes an unexpired link (invalidating it upon retrieval).
    async fn take_link(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ImLink>>;
    async fn peek_link(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ImLink>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_connectors_answer_in_im_and_names_round_trip() {
        assert_eq!(
            ImPlatform::of_connector("lark_app"),
            Some(ImPlatform::Feishu)
        );
        assert_eq!(
            ImPlatform::of_connector("slack_app"),
            Some(ImPlatform::Slack)
        );
        assert_eq!(
            ImPlatform::of_connector("dingtalk_app"),
            Some(ImPlatform::DingTalk)
        );
        assert_eq!(
            ImPlatform::of_connector("wecom_bot"),
            Some(ImPlatform::WeCom)
        );
        assert_eq!(ImPlatform::of_connector("lark_webhook"), None);
        assert_eq!(
            ImPlatform::of_connector("google_chat_app"),
            Some(ImPlatform::GoogleChat)
        );
        assert_eq!(
            ImPlatform::of_connector("teams_app"),
            Some(ImPlatform::Teams)
        );
        assert_eq!(ImPlatform::of_connector("wecom_app"), None);
        assert_eq!(
            ImPlatform::of_connector("telegram_bot"),
            Some(ImPlatform::Telegram)
        );

        for status in [
            BotStatus::Idle,
            BotStatus::Connecting,
            BotStatus::Connected,
            BotStatus::Error,
        ] {
            assert_eq!(BotStatus::parse(status.as_str()).unwrap(), status);
        }

        for status in [
            MessageStatus::Received,
            MessageStatus::Answering,
            MessageStatus::Replied,
            MessageStatus::Ignored,
            MessageStatus::Failed,
        ] {
            assert_eq!(MessageStatus::parse(status.as_str()).unwrap(), status);
        }
    }
}
