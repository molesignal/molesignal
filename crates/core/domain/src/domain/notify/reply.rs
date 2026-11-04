// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 回复短信、电话按键处理告警。
//!
//! 发出带告警的短信或电话时记一条回复引用：发往哪个连接器、哪个号码（只记摘要）、通知了谁、里面有
//! 哪些告警。短信末尾写「回复 31 确认，32 解决」：十位是这个号码轮换使用的槽位（0–9），个位 1 是
//! 确认、2 是解决；只回一位数时按这个号码最近的一条处理。电话按 1 确认、按 2 解决，按键的回调带着
//! 引用的 ID。
//!
//! 回复或按键以引用里记的接收人的身份处理告警，要有 `alerts.incidents.acknowledge`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 回复引用多久有效：一天。
pub const REPLY_TTL_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// 一个号码轮换使用的槽位数。
pub const REPLY_SLOTS: u8 = 10;

/// 引用 ID 的长度：阿里云 IvrCall 的 `OutId` 最多 15 个字节。
pub const REPLY_REF_LEN: usize = 12;

/// 消息元数据里回复引用的 ID。
pub const REPLY_REF_KEY: &str = "reply.ref";

/// 消息元数据里电话按键回调的地址。
pub const REPLY_KEYPRESS_URL_KEY: &str = "reply.keypress_url";

/// 消息元数据里电话末尾念的那句：按哪个键。
pub const REPLY_VOICE_HINT_KEY: &str = "reply.voice_hint";

/// 回复要做的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyAction {
    Acknowledge,
    Resolve,
}

impl ReplyAction {
    /// 按键或代码个位上的数字。
    pub fn from_digit(digit: char) -> Option<Self> {
        match digit {
            '1' => Some(Self::Acknowledge),
            '2' => Some(Self::Resolve),
            _ => None,
        }
    }
}

/// 发出去的一条短信或一通电话：回复它时处理里面的告警。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRef {
    /// 短的随机 ID：电话按键的回调带着它。
    pub id: String,
    pub organization_id: Id,
    pub connector_id: Id,
    /// 号码的摘要，与合并线路用的一样。
    pub line_key: String,
    /// 这个号码上第几个槽位。
    pub slot: u8,
    /// 通知了谁：回复以他的身份处理。
    pub recipient_user_id: Option<Id>,
    pub alert_ids: Vec<Id>,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
}

impl ReplyRef {
    /// 短信末尾写的两个代码：确认、解决。
    pub fn codes(&self) -> (String, String) {
        (format!("{}1", self.slot), format!("{}2", self.slot))
    }
}

/// 短信回复的意思：哪个槽位（只回一位数时没有），做什么。认不出时为 `None`。
///
/// 只看开头的数字：`31`、`3 1`、`31 收到` 都是槽位 3 确认，`1` 是最近一条确认。
pub fn parse_sms_reply(text: &str) -> Option<(Option<u8>, ReplyAction)> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || c.is_whitespace())
        .filter(char::is_ascii_digit)
        .collect();

    let mut chars = digits.chars();

    match (chars.next(), chars.next(), chars.next()) {
        (Some(action), None, None) => Some((None, ReplyAction::from_digit(action)?)),
        (Some(slot), Some(action), None) => Some((
            Some(slot.to_digit(10)? as u8),
            ReplyAction::from_digit(action)?,
        )),
        _ => None,
    }
}

/// 一个随机的引用 ID：字母与数字，[`REPLY_REF_LEN`] 位，取 KSUID 随机的后半段。
pub fn new_reply_ref_id() -> String {
    let id = Id::new().to_string();
    let skip = id.chars().count().saturating_sub(REPLY_REF_LEN);

    id.chars().skip(skip).collect()
}

#[async_trait]
pub trait NotifyReplyRepository: Send + Sync {
    async fn record(&self, reply: ReplyRef) -> Result<ReplyRef>;
    /// 按 ID 找（电话按键）；过期的不算。
    async fn get(&self, id: &str, now: TimestampMicros) -> Result<Option<ReplyRef>>;
    /// 这个号码在这个槽位上最近的一条；`slot` 为空时是这个号码最近的一条。过期的不算。
    async fn find(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        line_key: &str,
        slot: Option<u8>,
        now: TimestampMicros,
    ) -> Result<Option<ReplyRef>>;
    /// 这个号码下一条用哪个槽位：最近一条的下一个，没有时从 1 开始。
    async fn next_slot(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        line_key: &str,
    ) -> Result<u8>;
    /// 把服务商的通话 ID 记到引用上：有的服务商（华为云）按键结果只带它。
    async fn bind_call(&self, id: &str, call_id: &str) -> Result<()> {
        let _ = (id, call_id);
        Ok(())
    }
    /// 这个连接器上按通话 ID 找；过期的不算。
    async fn find_by_call(
        &self,
        connector_id: &Id,
        call_id: &str,
        now: TimestampMicros,
    ) -> Result<Option<ReplyRef>> {
        let _ = (connector_id, call_id, now);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_reads_its_leading_digits() {
        assert_eq!(
            parse_sms_reply("31"),
            Some((Some(3), ReplyAction::Acknowledge))
        );
        assert_eq!(
            parse_sms_reply(" 3 2 "),
            Some((Some(3), ReplyAction::Resolve))
        );
        assert_eq!(
            parse_sms_reply("31 收到"),
            Some((Some(3), ReplyAction::Acknowledge))
        );
        assert_eq!(parse_sms_reply("1"), Some((None, ReplyAction::Acknowledge)));
        assert_eq!(parse_sms_reply("2 ok"), Some((None, ReplyAction::Resolve)));
    }

    #[test]
    fn anything_else_is_not_a_reply() {
        assert_eq!(parse_sms_reply("好的"), None);
        assert_eq!(parse_sms_reply("33"), None);
        assert_eq!(parse_sms_reply("312"), None);
        assert_eq!(parse_sms_reply(""), None);
        assert_eq!(parse_sms_reply("ok 31"), None);
    }

    #[test]
    fn a_ref_has_a_short_id_and_two_codes() {
        let id = new_reply_ref_id();
        assert_eq!(id.len(), REPLY_REF_LEN);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));

        let reply = ReplyRef {
            id,
            organization_id: Id::from_string("org"),
            connector_id: Id::from_string("conn"),
            line_key: "line".into(),
            slot: 3,
            recipient_user_id: None,
            alert_ids: Vec::new(),
            created_at: TimestampMicros(0),
            expires_at: TimestampMicros(REPLY_TTL_MICROS),
        };

        assert_eq!(reply.codes(), ("31".to_string(), "32".to_string()));
    }
}
