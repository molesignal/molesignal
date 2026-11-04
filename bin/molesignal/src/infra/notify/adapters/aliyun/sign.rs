// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 阿里云 OpenAPI 的 V3 签名（ACS3-HMAC-SHA256），按官方文档四步：规范化请求 → 待签名字符串 →
//! 用 AccessKey Secret 做 HMAC-SHA256 → `Authorization` 头。单测用文档里的固定参数示例核对。

use aws_lc_rs::hmac;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "ACS3-HMAC-SHA256";

/// 除字母、数字与 `-`、`_`、`.`、`~` 外都编码：空格是 `%20`，`*` 是 `%2A`。
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub(super) fn encode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

pub(super) fn sha256_hex(data: &[u8]) -> String {
    hex::encode(&Sha256::digest(data)[..])
}

/// 规范化查询串：名称与值分别编码，按名称升序，以 `&` 相连。发出去的查询串就是它。
pub(super) fn canonical_query(params: &[(&str, String)]) -> String {
    let mut pairs = params
        .iter()
        .map(|(name, value)| (encode(name), encode(value)))
        .collect::<Vec<_>>();

    pairs.sort();

    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// 一个请求要签的部分。`headers` 是要签进去的头（`host`、`x-acs-*`、`content-type`），小写名。
pub(super) struct Request<'a> {
    pub method: &'a str,
    pub query: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
}

/// `Authorization` 头的值。RPC 风格接口的路径固定是 `/`。
pub(super) fn authorization(key_id: &str, key_secret: &str, request: &Request<'_>) -> String {
    let mut headers = request.headers.to_vec();
    headers.sort_by(|a, b| a.0.cmp(b.0));

    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect::<String>();

    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n/\n{}\n{canonical_headers}\n{signed_headers}\n{}",
        request.method, request.query, request.payload_hash
    );

    let string_to_sign = format!("{ALGORITHM}\n{}", sha256_hex(canonical_request.as_bytes()));

    let key = hmac::Key::new(hmac::HMAC_SHA256, key_secret.as_bytes());
    let signature = hex::encode(hmac::sign(&key, string_to_sign.as_bytes()));

    format!("{ALGORITHM} Credential={key_id},SignedHeaders={signed_headers},Signature={signature}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_encoded_as_the_signature_expects() {
        assert_eq!(encode("a b*c~d"), "a%20b%2Ac~d");
        assert_eq!(
            encode("{\"k\":\"告警\"}"),
            "%7B%22k%22%3A%22%E5%91%8A%E8%AD%A6%22%7D"
        );
        assert_eq!(
            canonical_query(&[("b", "2".into()), ("a", "x y".into())]),
            "a=x%20y&b=2"
        );
    }

    /// 阿里云文档「V3 版本请求体 & 签名机制」的固定参数示例。
    #[test]
    fn the_documented_example_signs_the_same() {
        let empty = sha256_hex(b"");
        let query = canonical_query(&[
            ("RegionId", "cn-shanghai".into()),
            (
                "ImageId",
                "win2019_1809_x64_dtc_zh-cn_40G_alibase_20230811.vhd".into(),
            ),
        ]);

        let authorization = authorization(
            "YourAccessKeyId",
            "YourAccessKeySecret",
            &Request {
                method: "POST",
                query: &query,
                headers: &[
                    ("x-acs-version", "2014-05-26"),
                    ("host", "ecs.cn-shanghai.aliyuncs.com"),
                    ("x-acs-action", "RunInstances"),
                    ("x-acs-content-sha256", &empty),
                    ("x-acs-date", "2023-10-26T10:22:32Z"),
                    ("x-acs-signature-nonce", "3156853299f313e23d1673dc12e1703d"),
                ],
                payload_hash: &empty,
            },
        );

        assert_eq!(
            authorization,
            "ACS3-HMAC-SHA256 Credential=YourAccessKeyId,\
             SignedHeaders=host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version,\
             Signature=06563a9e1b43f5dfe96b81484da74bceab24a1d853912eee15083a6f0f3283c0"
        );
    }
}
