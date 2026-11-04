// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯会议：企业自建应用的 REST API，`POST https://api.meeting.qq.com/v1/meetings` 预约一场会。请求头
//! 带 AppId、SdkId（有才带）、SecretId、时间戳和随机数；签名是用 SecretKey 对「方法、签名头、路径、请求体」
//! 算 HMAC-SHA256，取十六进制再 Base64。

use aws_lc_rs::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::CONTENT_TYPE;
use serde_json::json;

use super::{MEETING_SECS, client, send, unix_seconds};
use crate::{
    domain::alerting::meeting::TencentMeetingSettings,
    shared::{Error, Result},
};

const API: &str = "https://api.meeting.qq.com";
const PATH: &str = "/v1/meetings";

pub(super) async fn book(
    settings: &TencentMeetingSettings,
    secret_key: &str,
    topic: &str,
) -> Result<String> {
    let now = unix_seconds()?;
    let start = now + 60;

    let body = json!({
        "userid": settings.userid,
        "instanceid": 1,
        "subject": topic.chars().take(120).collect::<String>(),
        "type": 0,
        "start_time": start.to_string(),
        "end_time": (start + MEETING_SECS).to_string(),
    })
    .to_string();

    let nonce = nonce()?;
    let signature = sign(
        secret_key,
        &settings.secret_id,
        nonce,
        now,
        "POST",
        PATH,
        &body,
    );
    let http = client(API).await?;

    let mut request = http
        .post(format!("{API}{PATH}"))
        .header(CONTENT_TYPE, "application/json")
        .header("X-TC-Key", &settings.secret_id)
        .header("X-TC-Timestamp", now.to_string())
        .header("X-TC-Nonce", nonce.to_string())
        .header("X-TC-Signature", signature)
        .header("AppId", &settings.app_id)
        .header("X-TC-Registered", "1")
        .body(body);

    if !settings.sdk_id.is_empty() {
        request = request.header("SdkId", &settings.sdk_id);
    }

    let booked = send(request, "Tencent Meeting").await?;

    booked["meeting_info_list"][0]["join_url"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Tencent Meeting did not return the meeting link"))
}

/// 随机的正整数。
fn nonce() -> Result<u32> {
    let mut bytes = [0; 4];

    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("no random nonce for Tencent Meeting"))?;

    Ok(u32::from_be_bytes(bytes).max(1))
}

/// 签名：`方法\n签名头\n路径\n请求体` 的 HMAC-SHA256，十六进制（小写）再 Base64。签名头按名字排好：
/// `X-TC-Key`、`X-TC-Nonce`、`X-TC-Timestamp`。
fn sign(
    secret_key: &str,
    secret_id: &str,
    nonce: u32,
    timestamp: i64,
    method: &str,
    path: &str,
    body: &str,
) -> String {
    let headers = format!("X-TC-Key={secret_id}&X-TC-Nonce={nonce}&X-TC-Timestamp={timestamp}");
    let text = format!("{method}\n{headers}\n{path}\n{body}");
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_bytes());
    let tag = hmac::sign(&key, text.as_bytes());

    STANDARD.encode(hex::encode(tag.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_is_the_base64_of_the_hex_hmac_of_what_is_sent() {
        let signature = sign("secret", "AKID", 7, 1_700_000_000, "POST", PATH, "{}");
        let text = "POST\nX-TC-Key=AKID&X-TC-Nonce=7&X-TC-Timestamp=1700000000\n/v1/meetings\n{}";
        let tag = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, b"secret"),
            text.as_bytes(),
        );

        assert_eq!(signature, STANDARD.encode(hex::encode(tag.as_ref())));
        assert_eq!(
            String::from_utf8(STANDARD.decode(&signature).expect("base64"))
                .expect("hex")
                .len(),
            64
        );
    }
}
