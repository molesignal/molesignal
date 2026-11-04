// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信自建应用的回调：「接收消息」服务器的地址校验，与模板卡片按钮的点击事件。
//!
//! 企业微信的加解密方案：
//! - `msg_signature` 是 Token、时间戳、随机串与密文按字典序排好拼起来的 SHA1（十六进制）；
//! - 密文是 base64 的 AES-256-CBC：密钥是 EncodingAESKey 补上 `=` 再 base64 解码的 32 字节，IV 是
//!   密钥的前 16 字节，PKCS#7 按 32 字节补齐；
//! - 明文是 16 字节随机串、4 字节网络序的消息长度、消息、企业 ID。
//!
//! 事件是 XML：外层的 `<Encrypt>` 是密文，解开是事件本身（[`card_event`]）。

use aws_lc_rs::{
    cipher::{AES_256, DecryptingKey, DecryptionContext, UnboundCipherKey},
    digest,
    iv::FixedLength,
};
use base64::{
    Engine as _, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};
use serde_json::Value;

use super::WecomAppConfig;
use crate::shared::{Error, Result};

/// 企业微信按 32 字节补齐。
const PADDING_BLOCK: usize = 32;

/// EncodingAESKey 补上 `=` 以后，最后一个字符多出的两位不一定是 0（企业微信文档里的示例就不是）：
/// 解码时不管它们，和企业微信给的各语言示例一样。
const KEY_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

/// 一个配了「接收消息」的自建应用：校验签名、解开密文。
pub(crate) struct WecomCallback {
    token: String,
    key: [u8; 32],
    corp_id: String,
}

impl WecomCallback {
    /// 连接器里填了回调的 Token 与 EncodingAESKey 时有；没填时这个应用不收点击。
    pub(crate) fn of(config: &Value) -> Result<Option<Self>> {
        let config = WecomAppConfig::parse(config)?;
        Self::from_app(&config)
    }

    pub(super) fn from_app(config: &WecomAppConfig) -> Result<Option<Self>> {
        let token = config.callback_token.trim();
        let aes_key = config.callback_aes_key.trim();

        if token.is_empty() || aes_key.is_empty() {
            return Ok(None);
        }

        let key = KEY_BASE64
            .decode(format!("{aes_key}="))
            .ok()
            .and_then(|key| <[u8; 32]>::try_from(key).ok())
            .ok_or_else(|| {
                Error::invalid("wecom_app callback_aes_key must be the 43-character EncodingAESKey")
            })?;

        Ok(Some(Self {
            token: token.to_string(),
            key,
            corp_id: config.corp_id.trim().to_string(),
        }))
    }

    /// `signature` 是这些参数的签名。
    pub(crate) fn signed(
        &self,
        timestamp: &str,
        nonce: &str,
        encrypted: &str,
        signature: &str,
    ) -> bool {
        let mut parts = [self.token.as_str(), timestamp, nonce, encrypted];
        parts.sort_unstable();

        let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, parts.concat().as_bytes());
        let expected = digest
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        // 长度相同的十六进制串：逐字节比较，不提前退出。
        expected.len() == signature.len()
            && expected
                .bytes()
                .zip(signature.trim().to_ascii_lowercase().bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }

    /// 解开密文，返回消息；不是这个企业的不认。
    pub(crate) fn decrypt(&self, encrypted: &str) -> Result<String> {
        let invalid = || Error::invalid("the WeCom callback could not be decrypted");

        let mut data = STANDARD.decode(encrypted.trim()).map_err(|_| invalid())?;

        if data.is_empty() || data.len() % 16 != 0 {
            return Err(invalid());
        }

        let iv: [u8; 16] = self.key[..16].try_into().map_err(|_| invalid())?;
        let key = UnboundCipherKey::new(&AES_256, &self.key).map_err(|_| invalid())?;
        let plain = DecryptingKey::cbc(key)
            .map_err(|_| invalid())?
            .decrypt(&mut data, DecryptionContext::Iv128(FixedLength::from(iv)))
            .map_err(|_| invalid())?;

        let pad = usize::from(*plain.last().ok_or_else(invalid)?);

        if pad == 0 || pad > PADDING_BLOCK || pad > plain.len() {
            return Err(invalid());
        }

        let plain = &plain[..plain.len() - pad];

        if plain.len() < 20 {
            return Err(invalid());
        }

        let length = u32::from_be_bytes(plain[16..20].try_into().map_err(|_| invalid())?) as usize;
        let message = plain.get(20..20 + length).ok_or_else(invalid)?;
        let receiver = plain.get(20 + length..).ok_or_else(invalid)?;

        if receiver != self.corp_id.as_bytes() {
            return Err(Error::forbidden(
                "the WeCom callback is for another company",
            ));
        }

        String::from_utf8(message.to_vec()).map_err(|_| invalid())
    }

    /// 加密一段消息，测试里当企业微信用。
    #[cfg(test)]
    pub(crate) fn encrypt(&self, message: &str, random: &[u8; 16]) -> String {
        use aws_lc_rs::cipher::{EncryptingKey, EncryptionContext};

        let mut data = random.to_vec();
        data.extend_from_slice(&u32::try_from(message.len()).unwrap().to_be_bytes());
        data.extend_from_slice(message.as_bytes());
        data.extend_from_slice(self.corp_id.as_bytes());

        let pad = PADDING_BLOCK - data.len() % PADDING_BLOCK;
        data.extend(std::iter::repeat_n(u8::try_from(pad).unwrap(), pad));

        let iv: [u8; 16] = self.key[..16].try_into().unwrap();
        EncryptingKey::cbc(UnboundCipherKey::new(&AES_256, &self.key).unwrap())
            .unwrap()
            .less_safe_encrypt(&mut data, EncryptionContext::Iv128(FixedLength::from(iv)))
            .unwrap();

        STANDARD.encode(data)
    }
}

/// 回调正文里的密文（外层 XML 的 `<Encrypt>`）。
pub(crate) fn encrypted_of(xml: &str) -> Option<String> {
    let document = roxmltree::Document::parse(xml).ok()?;

    document
        .descendants()
        .find(|node| node.has_tag_name("Encrypt"))
        .and_then(|node| node.text())
        .map(|text| text.trim().to_string())
}

/// 点了模板卡片上的一个按钮。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WecomCardEvent {
    /// 点的成员的 userid。
    pub(crate) user: String,
    /// 按钮的 `key`。
    pub(crate) event_key: String,
    pub(crate) task_id: String,
    /// 换掉这张卡片要的码，只能用一次。
    pub(crate) response_code: String,
}

/// 解开的事件：模板卡片的点击（`template_card_event`）才有，别的事件与消息没有。
pub(crate) fn card_event(xml: &str) -> Option<WecomCardEvent> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let field = |name: &str| {
        document
            .descendants()
            .find(|node| node.has_tag_name(name))
            .and_then(|node| node.text())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    };

    if field("MsgType")? != "event" || field("Event")? != "template_card_event" {
        return None;
    }

    Some(WecomCardEvent {
        user: field("FromUserName")?,
        event_key: field("EventKey")?,
        task_id: field("TaskId")?,
        response_code: field("ResponseCode")?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// 企业微信文档里的示例 EncodingAESKey。
    const AES_KEY: &str = "jWmYm7qr5nMoAUwZRjGtBxmz3KA1tkAj3ykkR6q2B2C";

    fn callback() -> WecomCallback {
        WecomCallback::of(&json!({
            "corpId": "wx5823bf96d3bd56c7",
            "corpSecret": "s",
            "agentId": 1_000_002,
            "callbackToken": "QDG6eK",
            "callbackAesKey": AES_KEY,
        }))
        .unwrap()
        .unwrap()
    }

    #[test]
    fn a_message_comes_back_out_of_its_ciphertext_for_its_own_company_only() {
        let callback = callback();
        let encrypted = callback.encrypt("<xml><MsgType>event</MsgType></xml>", &[7; 16]);

        assert_eq!(
            callback.decrypt(&encrypted).unwrap(),
            "<xml><MsgType>event</MsgType></xml>"
        );

        let other = WecomCallback::of(&json!({
            "corpId": "ww-other",
            "corpSecret": "s",
            "agentId": 1,
            "callbackToken": "QDG6eK",
            "callbackAesKey": AES_KEY,
        }))
        .unwrap()
        .unwrap();
        assert!(other.decrypt(&encrypted).is_err());
        assert!(callback.decrypt("not base64!").is_err());
    }

    #[test]
    fn the_signature_covers_token_timestamp_nonce_and_ciphertext() {
        let callback = callback();
        let mut parts = ["QDG6eK", "1409659813", "1372623149", "cipher"];
        parts.sort_unstable();
        let signature =
            digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, parts.concat().as_bytes())
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();

        assert!(callback.signed("1409659813", "1372623149", "cipher", &signature));
        assert!(!callback.signed("1409659814", "1372623149", "cipher", &signature));
    }

    #[test]
    fn without_a_token_and_key_the_app_takes_no_presses() {
        assert!(
            WecomCallback::of(&json!({ "corpId": "c", "corpSecret": "s", "agentId": 1 }))
                .unwrap()
                .is_none()
        );
        assert!(
            WecomCallback::of(&json!({
                "corpId": "c",
                "corpSecret": "s",
                "agentId": 1,
                "callbackToken": "t",
                "callbackAesKey": "short",
            }))
            .is_err()
        );
    }

    #[test]
    fn a_template_card_press_is_read_and_other_events_are_not() {
        let press = "<xml><ToUserName><![CDATA[wx5823bf96d3bd56c7]]></ToUserName>\
            <FromUserName><![CDATA[ada]]></FromUserName><CreateTime>1700000000</CreateTime>\
            <MsgType><![CDATA[event]]></MsgType><Event><![CDATA[template_card_event]]></Event>\
            <EventKey><![CDATA[{\"action\":\"acknowledge\"}]]></EventKey>\
            <TaskId><![CDATA[molesignal_1]]></TaskId><CardType><![CDATA[button_interaction]]></CardType>\
            <ResponseCode><![CDATA[code-1]]></ResponseCode><AgentID>1000002</AgentID></xml>";

        assert_eq!(
            card_event(press).unwrap(),
            WecomCardEvent {
                user: "ada".into(),
                event_key: "{\"action\":\"acknowledge\"}".into(),
                task_id: "molesignal_1".into(),
                response_code: "code-1".into(),
            }
        );

        assert!(card_event("<xml><MsgType>text</MsgType><Content>hi</Content></xml>").is_none());
        assert_eq!(
            encrypted_of("<xml><ToUserName>c</ToUserName><Encrypt><![CDATA[abc]]></Encrypt></xml>")
                .as_deref(),
            Some("abc")
        );
    }
}
