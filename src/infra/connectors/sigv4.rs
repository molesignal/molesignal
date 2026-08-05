// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS Signature Version 4 —— 仅覆盖 AWS JSON 协议 POST（CloudWatch Logs 等）所需的最小实装。
//!
//! 不引入 AWS SDK（仓库既定）。HMAC-SHA256 用 `aws-lc-rs`，摘要用 `sha2`，hex 自写。
//! 签名算法严格按 AWS 文档四步：canonical request → string to sign → signing key（HMAC 链）→
//! signature。运行期由 AWS 端最终校验；单测覆盖 HMAC/SHA256 已知向量与签名结构。

use aws_lc_rs::hmac;
use sha2::{Digest, Sha256};

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

/// 给一个 AWS JSON 协议 POST（路径 `/`、无 query）签 SigV4。
///
/// `amz_date` 为 `YYYYMMDDTHHMMSSZ`（须与最终发送的 `X-Amz-Date` 头一致）。`session_token`
/// 非空时纳入签名并需作 `X-Amz-Security-Token` 头发送。
#[allow(clippy::too_many_arguments)]
pub fn sign_post_json(
    access_key: &str,
    secret_key: &str,
    session_token: Option<&str>,
    region: &str,
    service: &str,
    host: &str,
    target: &str,
    body: &[u8],
    amz_date: &str,
) -> SignedHeaders {
    let date = &amz_date[..8]; // YYYYMMDD
    let payload_hash = sha256_hex(body);

    // canonical headers（按小写头名排序）。
    let mut headers: Vec<(&str, String)> = vec![
        ("content-type", "application/x-amz-json-1.1".to_string()),
        ("host", host.to_string()),
        ("x-amz-date", amz_date.to_string()),
        ("x-amz-target", target.to_string()),
    ];
    if let Some(tok) = session_token.filter(|t| !t.is_empty()) {
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

    #[test]
    fn sign_has_expected_structure_and_is_deterministic() {
        let a = sign_post_json(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            None,
            "us-east-1",
            "logs",
            "logs.us-east-1.amazonaws.com",
            "Logs_20140328.FilterLogEvents",
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
        let b = sign_post_json(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            None,
            "us-east-1",
            "logs",
            "logs.us-east-1.amazonaws.com",
            "Logs_20140328.FilterLogEvents",
            b"{}",
            "20150830T123600Z",
        );
        assert_eq!(a.authorization, b.authorization);
    }

    #[test]
    fn session_token_adds_signed_header() {
        let a = sign_post_json(
            "AKID",
            "secret",
            Some("tok"),
            "eu-west-1",
            "logs",
            "logs.eu-west-1.amazonaws.com",
            "Logs_20140328.FilterLogEvents",
            b"{}",
            "20150830T123600Z",
        );
        assert!(a.authorization.contains(
            "SignedHeaders=content-type;host;x-amz-date;x-amz-security-token;x-amz-target"
        ));
    }
}
