// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 短信回复、电话按键的回调：认出是哪条短信或哪通电话、要做什么。处理告警（以接收人的身份确认或
//! 解决）在接口层，那里有权限检查。
//!
//! 阿里云和通用短信网关的回调不签名，地址里带一个回调 Token：由服务端的根密钥按连接器派生，不存库，
//! 连接器页随时能显示带 Token 的地址。

use std::sync::Arc;

use aws_lc_rs::{constant_time, hmac};

use super::reply_line_key;
use crate::{
    domain::notify::{
        connector::NotifyConnector,
        reply::{NotifyReplyRepository, ReplyAction, ReplyRef, parse_sms_reply},
        repositories::NotifyConnectorRepository,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 回调 Token 取 HMAC 的前多少个十六进制字符。
const TOKEN_CHARS: usize = 32;

pub struct ReplyCallbacks {
    key: hmac::Key,
    replies: Arc<dyn NotifyReplyRepository>,
    connectors: Arc<dyn NotifyConnectorRepository>,
}

impl ReplyCallbacks {
    /// `key` 由服务端的根密钥为这个用途派生（`derive_key`）。
    pub fn new(
        key: [u8; 32],
        replies: Arc<dyn NotifyReplyRepository>,
        connectors: Arc<dyn NotifyConnectorRepository>,
    ) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
            replies,
            connectors,
        }
    }

    /// 一个连接器的回调 Token。
    pub fn token(&self, connector_id: &Id) -> String {
        let tag = hmac::sign(&self.key, connector_id.as_str().as_bytes());
        hex::encode(tag.as_ref())[..TOKEN_CHARS].to_string()
    }

    /// 回调带来的 Token 对不对。
    pub fn token_matches(&self, connector_id: &Id, presented: &str) -> bool {
        let expected = self.token(connector_id);
        constant_time::verify_slices_are_equal(expected.as_bytes(), presented.trim().as_bytes())
            .is_ok()
    }

    /// 回调地址里的连接器。
    pub async fn connector(&self, connector_id: &Id) -> Result<Option<NotifyConnector>> {
        self.connectors.find(connector_id).await
    }

    /// 一条短信回复：`from` 这个号码回了 `text`。认不出代码、找不到它回的那条时为 `None`。
    pub async fn sms_reply(
        &self,
        connector: &NotifyConnector,
        from: &str,
        text: &str,
    ) -> Result<Option<(ReplyRef, ReplyAction)>> {
        let Some((slot, action)) = parse_sms_reply(text) else {
            return Ok(None);
        };

        let reference = self
            .replies
            .find(
                &connector.organization_id,
                &connector.id,
                &reply_line_key(from),
                slot,
                TimestampMicros::now(),
            )
            .await?;

        Ok(reference.map(|reference| (reference, action)))
    }

    /// 这个号码最近一通电话上按了一个键：腾讯云的按键推送只带被叫号码。
    pub async fn keypress_on_line(
        &self,
        connector: &NotifyConnector,
        number: &str,
        digits: &str,
    ) -> Result<Option<(ReplyRef, ReplyAction)>> {
        let Some(action) = pressed(digits) else {
            return Ok(None);
        };

        let reference = self
            .replies
            .find(
                &connector.organization_id,
                &connector.id,
                &reply_line_key(number),
                None,
                TimestampMicros::now(),
            )
            .await?;

        Ok(reference.map(|reference| (reference, action)))
    }

    /// 把服务商的通话 ID 记到引用上。引用不是这个连接器的就不记。
    pub async fn bind_call(
        &self,
        connector: &NotifyConnector,
        reference: &str,
        call_id: &str,
    ) -> Result<()> {
        let known = self
            .replies
            .get(reference.trim(), TimestampMicros::now())
            .await?
            .is_some_and(|reference| reference.connector_id == connector.id);

        if known {
            self.replies.bind_call(reference.trim(), call_id).await?;
        }

        Ok(())
    }

    /// 这通电话上按了一个键：华为云的按键推送只带通话 ID。
    pub async fn keypress_on_call(
        &self,
        connector: &NotifyConnector,
        call_id: &str,
        digits: &str,
    ) -> Result<Option<(ReplyRef, ReplyAction)>> {
        let Some(action) = pressed(digits) else {
            return Ok(None);
        };

        let reference = self
            .replies
            .find_by_call(&connector.id, call_id, TimestampMicros::now())
            .await?;

        Ok(reference.map(|reference| (reference, action)))
    }

    /// 电话上按了一个键：`reference` 是拨出时记的引用。
    pub async fn keypress(
        &self,
        connector: &NotifyConnector,
        reference: &str,
        digits: &str,
    ) -> Result<Option<(ReplyRef, ReplyAction)>> {
        let Some(action) = pressed(digits) else {
            return Ok(None);
        };

        let reference = self
            .replies
            .get(reference.trim(), TimestampMicros::now())
            .await?
            .filter(|reference| {
                reference.connector_id == connector.id
                    && reference.organization_id == connector.organization_id
            });

        Ok(reference.map(|reference| (reference, action)))
    }
}

/// 按的第一个键。
fn pressed(digits: &str) -> Option<ReplyAction> {
    digits
        .trim()
        .chars()
        .next()
        .and_then(ReplyAction::from_digit)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    struct NoReplies;

    #[async_trait]
    impl NotifyReplyRepository for NoReplies {
        async fn record(&self, reply: ReplyRef) -> Result<ReplyRef> {
            Ok(reply)
        }

        async fn get(&self, _: &str, _: TimestampMicros) -> Result<Option<ReplyRef>> {
            Ok(None)
        }

        async fn find(
            &self,
            _: &Id,
            _: &Id,
            _: &str,
            _: Option<u8>,
            _: TimestampMicros,
        ) -> Result<Option<ReplyRef>> {
            Ok(None)
        }

        async fn next_slot(&self, _: &Id, _: &Id, _: &str) -> Result<u8> {
            Ok(1)
        }
    }

    struct NoConnectors;

    #[async_trait]
    impl NotifyConnectorRepository for NoConnectors {
        async fn create(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
            Ok(connector)
        }

        async fn update(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
            Ok(connector)
        }

        async fn get(&self, _: &Id, _: &Id) -> Result<NotifyConnector> {
            Err(crate::shared::Error::not_found("connector"))
        }

        async fn list(&self, _: &Id) -> Result<Vec<NotifyConnector>> {
            Ok(Vec::new())
        }

        async fn record_test_result(
            &self,
            _: &Id,
            _: &Id,
            _: TimestampMicros,
            _: crate::domain::notify::connector::ConnectorTestStatus,
            _: Option<String>,
        ) -> Result<NotifyConnector> {
            Err(crate::shared::Error::not_found("connector"))
        }

        async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn each_connector_has_its_own_token_and_only_it_matches() {
        let callbacks = ReplyCallbacks::new([7; 32], Arc::new(NoReplies), Arc::new(NoConnectors));
        let one = Id::from_string("conn-1");
        let two = Id::from_string("conn-2");

        let token = callbacks.token(&one);
        assert_eq!(token.len(), TOKEN_CHARS);
        assert_ne!(token, callbacks.token(&two));
        assert!(callbacks.token_matches(&one, &token));
        assert!(callbacks.token_matches(&one, &format!(" {token} ")));
        assert!(!callbacks.token_matches(&two, &token));
        assert!(!callbacks.token_matches(&one, "nope"));

        let other = ReplyCallbacks::new([8; 32], Arc::new(NoReplies), Arc::new(NoConnectors));
        assert_ne!(
            other.token(&one),
            token,
            "another server key, another token"
        );
    }
}
