// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 回复短信、电话按键：发出带告警的短信或电话前记一条回复引用（[`ReplyRef`]），消息带上代码与提示。
//!
//! - 短信：末尾加一句「回复 31 确认，32 解决」。正文里写了 `{{reply.ack}}`、`{{reply.resolve}}` 的
//!   （含审核过的服务商模板的变量）就填上代码，不再另加。
//! - 电话：元数据带上按键回调的地址与「按 1 确认，按 2 解决」，由打电话的连接器念出、收键。
//!
//! 回复以引用里记的接收人的身份处理告警；发到群、固定号码时没有接收人，不记引用。

use std::sync::Arc;

use super::NotifyEngine;
use crate::{
    domain::notify::{
        connector::{
            ConnectorAdapter, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
        },
        reply::{
            NotifyReplyRepository, REPLY_KEYPRESS_URL_KEY, REPLY_REF_KEY, REPLY_TTL_MICROS,
            REPLY_VOICE_HINT_KEY, ReplyRef, new_reply_ref_id,
        },
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 号码的摘要：只看最后 11 位数字，同一个号码写成 `+8613800138000` 还是 `13800138000` 都一样。
pub fn reply_line_key(number: &str) -> String {
    let digits: Vec<char> = number.chars().filter(char::is_ascii_digit).collect();
    let tail: String = digits[digits.len().saturating_sub(11)..].iter().collect();

    blake3::hash(format!("reply|{tail}").as_bytes())
        .to_hex()
        .to_string()
}

/// 按键回调的地址：在对外地址下；没有对外地址时收不到按键。
pub fn keypress_url(external_url: &str, connector_id: &Id, reference: &str) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    (!base.is_empty())
        .then(|| format!("{base}/api/v1/public/notify/{connector_id}/keypress?ref={reference}"))
}

impl NotifyEngine {
    /// 回复引用记在哪里；没有时不收回复。
    pub fn with_replies(mut self, replies: Arc<dyn NotifyReplyRepository>) -> Self {
        self.replies = Some(replies);
        self
    }

    /// 发往 `target` 的这条短信或电话收得到回复时，记一条引用并在消息里带上代码与提示；否则原样返回。
    pub(super) async fn with_reply(
        &self,
        connector: &NotifyConnector,
        adapter: &dyn ConnectorAdapter,
        target: &NotifyTarget,
        recipient: Option<&Id>,
        alert_ids: Vec<Id>,
        mut message: NotifyMessage,
    ) -> NotifyMessage {
        let channel = adapter.channel();

        let (Some(replies), Some(recipient)) = (&self.replies, recipient) else {
            return message;
        };

        if alert_ids.is_empty()
            || !matches!(channel, NotifyChannel::Sms | NotifyChannel::Phone)
            || !adapter.takes_replies(&connector.config)
        {
            return message;
        }

        let line_key = reply_line_key(&target.value);
        let now = TimestampMicros::now();

        let slot = match replies
            .next_slot(&connector.organization_id, &connector.id, &line_key)
            .await
        {
            Ok(slot) => slot,
            Err(error) => {
                tracing::warn!(%error, "reply slot could not be read; sending without reply codes");
                return message;
            }
        };

        let reference = ReplyRef {
            id: new_reply_ref_id(),
            organization_id: connector.organization_id.clone(),
            connector_id: connector.id.clone(),
            line_key,
            slot,
            recipient_user_id: Some(recipient.clone()),
            alert_ids,
            created_at: now,
            expires_at: TimestampMicros(now.0.saturating_add(REPLY_TTL_MICROS)),
        };

        if let Err(error) = replies.record(reference.clone()).await {
            tracing::warn!(%error, "reply reference could not be recorded; sending without reply codes");
            return message;
        }

        let (ack, resolve) = reference.codes();

        let hints = match self.alert_buttons.source() {
            Some(source) => {
                source
                    .reply_hints(&connector.organization_id, Some(recipient), &ack, &resolve)
                    .await
            }
            None => None,
        };

        message
            .metadata
            .insert(REPLY_REF_KEY.into(), reference.id.clone());

        if channel == NotifyChannel::Phone {
            if let Some(url) = keypress_url(&self.external_url, &connector.id, &reference.id) {
                message.metadata.insert(REPLY_KEYPRESS_URL_KEY.into(), url);
            }

            if let Some(hints) = hints {
                message
                    .metadata
                    .insert(REPLY_VOICE_HINT_KEY.into(), hints.voice);
            }

            return message;
        }

        fill_codes(&mut message, &ack, &resolve, hints.map(|hints| hints.sms))
    }
}

/// 短信里填上代码：写了 `{{reply.ack}}`、`{{reply.resolve}}` 的地方填上，都没写时在末尾加提示。
fn fill_codes(
    message: &mut NotifyMessage,
    ack: &str,
    resolve: &str,
    hint: Option<String>,
) -> NotifyMessage {
    let written =
        |value: &str| value.contains("{{reply.ack}}") || value.contains("{{reply.resolve}}");
    let fill = |value: &str| {
        value
            .replace("{{reply.ack}}", ack)
            .replace("{{reply.resolve}}", resolve)
    };

    let mut placed = written(&message.text);
    message.text = fill(&message.text);

    for template in message.provider_templates.values_mut() {
        for value in template.params.values_mut() {
            placed |= written(value);
            *value = fill(value);
        }
    }

    if !placed
        && let Some(hint) = hint
        && message.provider_templates.is_empty()
    {
        message.text = match message.text.trim() {
            "" => hint,
            text => format!("{text}\n{hint}"),
        };
    }

    message.clone()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::notify::template::ProviderTemplate;

    fn message(text: &str) -> NotifyMessage {
        NotifyMessage {
            title: String::new(),
            text: text.into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    #[test]
    fn one_number_written_two_ways_is_one_line() {
        assert_eq!(
            reply_line_key("+86 138-0013-8000"),
            reply_line_key("13800138000")
        );
        assert_ne!(reply_line_key("13800138000"), reply_line_key("13800138001"));
    }

    #[test]
    fn the_hint_goes_at_the_end_unless_the_template_places_the_codes() {
        let hint = Some("回复 31 确认，32 解决".to_string());

        let plain = fill_codes(
            &mut message("[CRITICAL] Disk full"),
            "31",
            "32",
            hint.clone(),
        );
        assert_eq!(plain.text, "[CRITICAL] Disk full\n回复 31 确认，32 解决");

        let placed = fill_codes(
            &mut message("Disk full, reply {{reply.ack}} to take it"),
            "31",
            "32",
            hint.clone(),
        );
        assert_eq!(placed.text, "Disk full, reply 31 to take it");

        let mut approved = message("");
        approved.provider_templates.insert(
            "aliyun".into(),
            ProviderTemplate {
                code: "SMS_1".into(),
                params: BTreeMap::from([("ack".to_string(), "{{reply.ack}}".to_string())]),
                max_param_chars: None,
            },
        );

        let approved = fill_codes(&mut approved, "31", "32", hint);
        assert_eq!(approved.provider_templates["aliyun"].params["ack"], "31");
        assert_eq!(approved.text, "", "an approved template gets no extra text");
    }

    #[test]
    fn a_keypress_needs_the_address_from_outside() {
        assert_eq!(
            keypress_url("https://ms.example.com/", &Id::from_string("conn-1"), "abc"),
            Some("https://ms.example.com/api/v1/public/notify/conn-1/keypress?ref=abc".into())
        );
        assert_eq!(keypress_url(" ", &Id::from_string("conn-1"), "abc"), None);
    }
}
