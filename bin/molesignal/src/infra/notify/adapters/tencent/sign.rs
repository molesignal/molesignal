// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯云 API 3.0 的签名：TC3-HMAC-SHA256。

use aws_lc_rs::hmac;
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "TC3-HMAC-SHA256";
const SIGNED_HEADERS: &str = "content-type;host;x-tc-action";

/// 要签的一次请求。
pub(super) struct SignedCall<'a> {
    pub secret_id: &'a str,
    pub secret_key: &'a str,
    pub service: &'a str,
    pub host: &'a str,
    pub action: &'a str,
    pub content_type: &'a str,
    pub payload: &'a str,
    /// 请求时间（Unix 秒）。
    pub timestamp: i64,
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);

    hmac::sign(&key, data.as_bytes()).as_ref().to_vec()
}

/// 规范请求串：方法、路径、查询串（POST 为空）、规范头部、签名头部、请求体的哈希。
fn canonical_request(call: &SignedCall<'_>) -> String {
    format!(
        "POST\n/\n\ncontent-type:{}\nhost:{}\nx-tc-action:{}\n\n{SIGNED_HEADERS}\n{}",
        call.content_type.to_ascii_lowercase(),
        call.host,
        call.action.to_ascii_lowercase(),
        sha256_hex(call.payload.as_bytes()),
    )
}

/// `Authorization` 的值。日期是 UTC 的，和时间戳一致。
pub(super) fn authorization(call: &SignedCall<'_>) -> String {
    let date = chrono::DateTime::from_timestamp(call.timestamp, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string();

    let scope = format!("{date}/{}/tc3_request", call.service);

    let string_to_sign = format!(
        "{ALGORITHM}\n{}\n{scope}\n{}",
        call.timestamp,
        sha256_hex(canonical_request(call).as_bytes())
    );

    let secret_date = hmac_sha256(format!("TC3{}", call.secret_key).as_bytes(), &date);
    let secret_service = hmac_sha256(&secret_date, call.service);
    let secret_signing = hmac_sha256(&secret_service, "tc3_request");
    let signature = hex::encode(hmac_sha256(&secret_signing, &string_to_sign));

    format!(
        "{ALGORITHM} Credential={}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}",
        call.secret_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 腾讯云文档「签名方法 v3」的例子：请求体和规范请求串的哈希取自文档。文档把密钥打了码，最后的签名
    /// 用同样的算法在别处（Python 的 hmac）算出来对照。
    fn documented() -> SignedCall<'static> {
        SignedCall {
            secret_id: "AKIDEXAMPLE",
            secret_key: "Gu5t9xGARNpq86cd98joQYCN3EXAMPLE",
            service: "cvm",
            host: "cvm.tencentcloudapi.com",
            action: "DescribeInstances",
            content_type: "application/json; charset=utf-8",
            payload: r#"{"Limit": 1, "Filters": [{"Values": ["\u672a\u547d\u540d"], "Name": "instance-name"}]}"#,
            timestamp: 1_551_113_065,
        }
    }

    #[test]
    fn the_canonical_request_is_the_one_in_the_documentation() {
        assert_eq!(
            sha256_hex(canonical_request(&documented()).as_bytes()),
            "7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84"
        );
    }

    #[test]
    fn the_signature_comes_from_the_derived_key() {
        assert_eq!(
            authorization(&documented()),
            "TC3-HMAC-SHA256 Credential=AKIDEXAMPLE/2019-02-25/cvm/tc3_request, \
             SignedHeaders=content-type;host;x-tc-action, \
             Signature=644be983de9a8a3f00db8eadaba61467c3b429e2215758ba897b738ca469fd26"
        );
    }
}
