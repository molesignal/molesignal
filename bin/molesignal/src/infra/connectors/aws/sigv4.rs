// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS Signature Version 4 —— 仅覆盖对 `/`、不带 query 的 POST 所需的最小实装：AWS JSON 协议
//! （CloudWatch Logs）与 Query 协议（STS）。
//!
//! 不引入 AWS SDK（仓库既定）。HMAC-SHA256 用 `aws-lc-rs`，摘要用 `sha2`，hex 自写。
//! 签名算法严格按 AWS 文档四步：canonical request → string to sign → signing key（HMAC 链）→
//! signature。运行期由 AWS 端最终校验；单测覆盖 HMAC/SHA256 已知向量与签名结构。

use aws_lc_rs::hmac;
use sha2::{Digest, Sha256};

use crate::shared::time::TimestampMicros;

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);

    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }

    s
}

fn sha256_hex(data: &[u8]) -> String {
    hex_lower(&Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&k, data).as_ref().to_vec()
}

/// 签名结果：附到请求上的 `Authorization` 与 `X-Amz-Date` 头。
pub struct SignedHeaders {
    pub authorization: String,
    pub amz_date: String,
}

/// 签名用的密钥；`session_token` 非空时纳入签名，并需作 `X-Amz-Security-Token` 头发送。
pub struct SigningKey<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub session_token: Option<&'a str>,
}

/// 现在的 `YYYYMMDDTHHMMSSZ`（UTC），即 `X-Amz-Date`。
pub fn amz_date_now() -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_micros(TimestampMicros::now().0)
        .map(|dt| dt.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_else(|| "19700101T000000Z".to_string())
}

/// 给一个 POST（路径 `/`、无 query）签 SigV4。
///
/// `headers` 是除 `host` / `x-amz-date` / `x-amz-security-token` 外请求自带、需签入的头（小写名，
/// 如 `content-type`、`x-amz-target`），须原样随请求发送。`amz_date` 为 `YYYYMMDDTHHMMSSZ`
/// （须与最终发送的 `X-Amz-Date` 头一致）。
pub fn sign_post(
    key: &SigningKey<'_>,
    region: &str,
    service: &str,
    host: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    amz_date: &str,
) -> SignedHeaders {
    let (access_key, secret_key) = (key.access_key, key.secret_key);
    let date = &amz_date[..8]; // YYYYMMDD
    let payload_hash = sha256_hex(body);

    // canonical headers（按小写头名排序）。
    let mut headers: Vec<(&str, String)> = headers
        .iter()
        .map(|(name, value)| (*name, (*value).to_string()))
        .chain([
            ("host", host.to_string()),
            ("x-amz-date", amz_date.to_string()),
        ])
        .collect();

    if let Some(tok) = key.session_token.filter(|t| !t.is_empty()) {
        headers.push(("x-amz-security-token", tok.to_string()));
    }

    headers.sort_by(|a, b| a.0.cmp(b.0));

    let signed_headers = headers
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(";");

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();

    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let scope = format!("{date}/{region}/{service}/aws4_request");

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_lower(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    SignedHeaders {
        authorization,
        amz_date: amz_date.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case1() {
        // RFC 4231 Test Case 1: key = 0x0b*20, data = "Hi There".
        let key = [0x0b_u8; 20];
        let tag = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex_lower(&tag),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    const JSON_HEADERS: &[(&str, &str)] = &[
        ("content-type", "application/x-amz-json-1.1"),
        ("x-amz-target", "Logs_20140328.FilterLogEvents"),
    ];

    fn key(session_token: Option<&str>) -> SigningKey<'_> {
        SigningKey {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            session_token,
        }
    }

    #[test]
    fn sign_has_expected_structure_and_is_deterministic() {
        let a = sign_post(
            &key(None),
            "us-east-1",
            "logs",
            "logs.us-east-1.amazonaws.com",
            JSON_HEADERS,
            b"{}",
            "20150830T123600Z",
        );
        assert!(a.authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/logs/aws4_request"
        ));
        assert!(
            a.authorization
                .contains("SignedHeaders=content-type;host;x-amz-date;x-amz-target")
        );
        // signature 是 64 位小写 hex。
        let sig = a.authorization.rsplit("Signature=").next().unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
        // 同输入确定性。
        let b = sign_post(
            &key(None),
            "us-east-1",
            "logs",
            "logs.us-east-1.amazonaws.com",
            JSON_HEADERS,
            b"{}",
            "20150830T123600Z",
        );
        assert_eq!(a.authorization, b.authorization);
    }

    #[test]
    fn session_token_adds_signed_header() {
        let a = sign_post(
            &key(Some("tok")),
            "eu-west-1",
            "logs",
            "logs.eu-west-1.amazonaws.com",
            JSON_HEADERS,
            b"{}",
            "20150830T123600Z",
        );
        assert!(a.authorization.contains(
            "SignedHeaders=content-type;host;x-amz-date;x-amz-security-token;x-amz-target"
        ));
    }

    #[test]
    fn a_query_protocol_request_signs_only_the_headers_it_sends() {
        let a = sign_post(
            &key(None),
            "us-east-1",
            "sts",
            "sts.us-east-1.amazonaws.com",
            &[(
                "content-type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )],
            b"Action=AssumeRole&Version=2011-06-15",
            "20150830T123600Z",
        );

        assert!(a.authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/sts/aws4_request"
        ));
        assert!(
            a.authorization
                .contains("SignedHeaders=content-type;host;x-amz-date,")
        );
    }

    #[test]
    fn amz_date_is_basic_iso8601_in_utc() {
        let now = amz_date_now();

        assert_eq!(now.len(), 16);
        assert_eq!(&now[8..9], "T");
        assert!(now.ends_with('Z'));
    }
}
