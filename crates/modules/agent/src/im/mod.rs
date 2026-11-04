// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 在 IM 里：用一个 notify 应用连接器（飞书、Slack、钉钉、企业微信智能机器人、
//! Google Chat、Microsoft Teams）收消息、回答。
//!
//! - 机器人（[`ImBot`]）：一个应用连接器开了「在 IM 里回答」。机器人用长连接收消息（Teams 是
//!   公网回调），同一时间只有一个节点连着（租约）；状态写回机器人。
//! - 话题（[`ImThread`]）：群里的一个话题、一个人在群里（钉钉）或私聊里的对话，对应一个会话。
//!   `/new` 与 `/scope` 换成新会话。
//! - 消息（[`ImMessage`]）：收到的每一条，按平台的消息 ID 去重；排了一轮回答的记下 Run，结束
//!   后回复到原处。
//! - 绑定（[`ImLink`]）：IM 账号还没对上 MoleSignal 用户时私下发一个一次性链接，登录后确认。
//! - 待发的回复（[`ImOutgoing`]）：回复只能从连着机器人的节点发出时（企业微信），别的节点把回复
//!   记在这里，由连着的节点发出去。
//! - 告警卡片上的按钮（[`CardAction`]）：飞书与 Slack 的告警通知带着确认、解决按钮，点了从长连接
//!   交回来，结果是给点的人的一句话和换上的卡片（[`CardAnswer`]）。
//!
//! 只认已验证的通知身份：IM 账号对上的必须是一个启用、已验证的通知身份，对上多个人时不答。

pub mod commands;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::notify::{
        buttons::{AlertButtonValue, AlertButtons},
        connector::NotifyMessage,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 能在 IM 里回答的平台，按连接器类型认。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImPlatform {
    Feishu,
    Slack,
    DingTalk,
    WeCom,
    GoogleChat,
    Teams,
}

impl ImPlatform {
    pub const ALL: [Self; 6] = [
        Self::Feishu,
        Self::Slack,
        Self::DingTalk,
        Self::WeCom,
        Self::GoogleChat,
        Self::Teams,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Slack => "slack",
            Self::DingTalk => "dingtalk",
            Self::WeCom => "wecom",
            Self::GoogleChat => "google_chat",
            Self::Teams => "teams",
        }
    }

    /// 这种连接器能不能在 IM 里回答。
    pub fn of_connector(connector_type: &str) -> Option<Self> {
        match connector_type {
            "lark_app" => Some(Self::Feishu),
            "slack_app" => Some(Self::Slack),
            "dingtalk_app" => Some(Self::DingTalk),
            "wecom_bot" => Some(Self::WeCom),
            "google_chat_app" => Some(Self::GoogleChat),
            "teams_app" => Some(Self::Teams),
            _ => None,
        }
    }

    /// 写给人看的平台名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Feishu => "Feishu",
            Self::Slack => "Slack",
            Self::DingTalk => "DingTalk",
            Self::WeCom => "WeCom",
            Self::GoogleChat => "Google Chat",
            Self::Teams => "Microsoft Teams",
        }
    }
}

/// 机器人的长连接现在怎么样。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotStatus {
    /// 没开，或还没有节点连上。
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

/// 一个开了「在 IM 里回答」的应用连接器。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImBot {
    pub connector_id: Id,
    pub org_id: Id,
    pub enabled: bool,
    /// 群里新开的话题记成这个团队的团队会话（提问人是成员时）；空就是提问人的个人会话。
    pub default_team_id: Option<Id>,
    pub status: BotStatus,
    /// 连不上的原因，或连上的节点。
    pub status_detail: Option<String>,
    pub status_at: Option<TimestampMicros>,
    pub last_message_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 消息来自私聊还是群。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Direct,
    Group,
}

/// 一个 IM 话题对应的会话。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImThread {
    pub id: Id,
    pub org_id: Id,
    pub connector_id: Id,
    /// 群或私聊。
    pub conversation_id: String,
    /// 话题：群里话题的根消息、钉钉群里的提问人；私聊是空的。
    pub thread_key: String,
    pub chat_id: Id,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 发消息的人在平台上的身份：按顺序去对通知身份。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sender {
    /// 平台上的 ID，例如飞书的 open_id、user_id、union_id。第一个当作这个人的主 ID。
    pub ids: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Sender {
    pub fn primary(&self) -> &str {
        self.ids.first().map(String::as_str).unwrap_or_default()
    }
}

/// 收到的一条消息，已经去掉了对机器人的 @。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inbound {
    pub message_id: String,
    pub conversation_id: String,
    pub kind: ConversationKind,
    pub thread_key: String,
    pub sender: Sender,
    pub text: String,
    /// 群里 @ 了机器人；私聊总是。
    pub addressed: bool,
    /// 回复到原处要的东西，各平台不同。
    pub reply: Value,
}

/// 点了告警卡片上的一个按钮。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardAction {
    pub value: AlertButtonValue,
    pub sender: Sender,
    pub conversation_id: String,
    /// 平台不说是不是私聊时当作群：绑定链接私下发。
    pub kind: ConversationKind,
    /// 回复到卡片所在处要的东西，同 [`Inbound::reply`]。
    pub reply: Value,
    /// 换掉卡片要的东西，各平台不同：飞书的消息 ID，Slack 的 `response_url`。
    pub card: Value,
}

/// 给点按钮的人的一句话是什么口气。
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

/// 点了按钮以后：给点的人的一句话（飞书是一个提示，Slack 是只有他看得到的消息），与换上的卡片。
#[derive(Debug, Clone)]
pub struct CardAnswer {
    pub notice: Option<(NoticeKind, String)>,
    pub card: Option<AlertCard>,
}

/// 换上的告警卡片：按告警现在的样子重写的通知、还能点的按钮，与一句谁做了什么。
#[derive(Debug, Clone)]
pub struct AlertCard {
    pub message: NotifyMessage,
    pub buttons: AlertButtons,
    pub note: Option<String>,
}

/// 一条消息走到哪一步了。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// 收到了，还在处理。
    Received,
    /// 排了一轮回答，等它结束。
    Answering,
    /// 回复了。
    Replied,
    /// 不用回答：没 @ 机器人、是命令，或者回过了别的话。
    Ignored,
    /// 回复不出去。
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

/// 收到的一条消息与它的回复。
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
    /// 「处理中」表情的 ID，回复后撤掉。
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

/// 一次性的绑定链接：私下发给 IM 里的人，登录 MoleSignal 后确认，把这个 IM 账号记成本人的已验证
/// 通知身份。只存令牌的摘要。
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

/// 绑定链接多久有效。
pub const LINK_LIFETIME_MICROS: i64 = 15 * 60 * 1_000_000;

/// 一条等连着机器人的节点发出去的回复：把「处理中」换成回答，或回复到原处。也可以是机器人主动发的一条
/// 通知（`notification`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImOutgoing {
    pub id: Id,
    pub org_id: Id,
    pub connector_id: Id,
    /// 这条回复了结的消息；发出去后写回它的状态。
    pub message_id: Option<Id>,
    /// 回复到哪里，同 [`Inbound::reply`]。
    pub reply: Value,
    /// 要换掉的「处理中」。
    pub working: Option<String>,
    pub markdown: String,
    /// 机器人主动发的一条通知，平台的消息体，发往哪里写在里面；有它时不看 `reply` 与 `markdown`。
    #[serde(default)]
    pub notification: Option<Value>,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait ImRepository: Send + Sync {
    async fn list_bots(&self, org_id: &Id) -> Result<Vec<ImBot>>;
    async fn get_bot(&self, org_id: &Id, connector_id: &Id) -> Result<Option<ImBot>>;
    /// 按连接器找机器人，不看组织：公网回调只知道连接器 ID。
    async fn find_bot(&self, connector_id: &Id) -> Result<Option<ImBot>>;
    /// 新建或改机器人的设置（状态与租约不动）。
    async fn put_bot(&self, bot: &ImBot) -> Result<ImBot>;
    async fn delete_bot(&self, org_id: &Id, connector_id: &Id) -> Result<()>;
    /// 所有组织里开着的机器人，给 worker 用。
    async fn enabled_bots(&self) -> Result<Vec<ImBot>>;
    /// 领到或续上一个机器人的租约；别的节点持有且没过期时返回 false。
    async fn lease_bot(
        &self,
        connector_id: &Id,
        node: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool>;
    async fn release_bot(&self, connector_id: &Id, node: &str) -> Result<()>;
    /// 写下机器人的状态；只有持有租约的 `node` 写得进去（交接时旧节点不会盖掉新节点的）。
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
    /// 记下话题对应的会话；已经有了就换成这个会话。
    async fn put_thread(&self, thread: &ImThread) -> Result<ImThread>;

    /// 记下收到的消息；同一个连接器的同一条消息已经记过时返回 false。
    async fn record_message(&self, message: &ImMessage) -> Result<bool>;
    async fn update_message(&self, message: &ImMessage) -> Result<()>;
    /// 把一条消息从 `from` 换到 `to`；别人先换了时返回 false（回复只发一次）。
    async fn move_message(&self, id: &Id, from: MessageStatus, to: MessageStatus) -> Result<bool>;
    /// 回复发出去了（或发不出去）：写下状态、原因与时间。
    async fn settle_message(
        &self,
        id: &Id,
        status: MessageStatus,
        detail: Option<&str>,
        replied_at: TimestampMicros,
    ) -> Result<()>;
    async fn message_for_run(&self, run_id: &Id) -> Result<Option<ImMessage>>;
    /// 还在等回答、早于 `before` 收到的消息：节点没了时由 worker 补上回复。
    async fn answering(&self, before: TimestampMicros, limit: i64) -> Result<Vec<ImMessage>>;
    /// 清掉早于 `before` 的消息与没发出去的回复。
    async fn purge_messages(&self, before: TimestampMicros) -> Result<u64>;

    /// 记下一条等连着机器人的节点发的回复。
    async fn enqueue_outgoing(&self, outgoing: &ImOutgoing) -> Result<()>;
    /// 取走一个机器人最早的几条待发回复（取走就不在了，别的节点拿不到）。
    async fn take_outgoing(&self, connector_id: &Id, limit: i64) -> Result<Vec<ImOutgoing>>;

    async fn create_link(&self, link: &ImLink) -> Result<()>;
    /// 取走一个没过期的链接（取走就作废）。
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
