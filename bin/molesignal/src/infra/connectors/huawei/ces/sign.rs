// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Huawei Cloud's AK/SK signature for API Gateway (SDK-HMAC-SHA256), as its developer guide
//! lays it out: canonical request → string to sign → HMAC-SHA256 with the SK → `Authorization`.
//! The test checks the canonical request of the guide's worked example.

use aws_lc_rs::hmac;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "SDK-HMAC-SHA256";
const SIGNED_HEADERS: &str = "content-type;host;x-sdk-date";

/// Everything but letters, digits, `-`, `_`, `.` and `~` is percent-encoded.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// The parts of a request that are signed.
pub(super) struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// The path as sent, such as `/V1.0/<project>/metrics`.
    pub path: &'a str,
    /// The canonical query string, from [`canonical_query`]; the request sends the same.
    pub query: &'a str,
    pub content_type: &'a str,
    pub body: &'a [u8],
    /// `X-Sdk-Date`, from [`sdk_date`].
    pub date: &'a str,
}

/// `X-Sdk-Date` for a time in unix seconds: UTC, `YYYYMMDDTHHMMSSZ`.
pub(super) fn sdk_date(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .format("%Y%m%dT%H%M%SZ")
        .to_string()
}

fn encode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Names and values encoded, sorted by name, joined by `&`; `=` kept for an empty value.
pub(super) fn canonical_query(params: &[(&str, String)]) -> String {
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .map(|(name, value)| (encode(name), encode(value)))
        .collect();

    pairs.sort();

    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Each path segment encoded; the signed path always ends with `/`, the sent one need not.
fn canonical_uri(path: &str) -> String {
    let encoded = path.split('/').map(encode).collect::<Vec<_>>().join("/");

    if encoded.ends_with('/') {
        encoded
    } else {
        format!("{encoded}/")
    }
}

fn canonical_request(request: &Request<'_>) -> String {
    format!(
        "{}\n{}\n{}\ncontent-type:{}\nhost:{}\nx-sdk-date:{}\n\n{SIGNED_HEADERS}\n{}",
        request.method,
        canonical_uri(request.path),
        request.query,
        request.content_type.trim(),
        request.host.trim(),
        request.date,
        sha256_hex(request.body),
    )
}

/// The `Authorization` header of `request`, signed with the AK/SK pair.
pub(super) fn authorization(access_key: &str, secret_key: &str, request: &Request<'_>) -> String {
    let string_to_sign = format!(
        "{ALGORITHM}\n{}\n{}",
        request.date,
        sha256_hex(canonical_request(request).as_bytes())
    );

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_bytes());
    let signature = hex::encode(hmac::sign(&key, string_to_sign.as_bytes()));

    format!(
        "{ALGORITHM} Access={access_key}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example of the developer guide ("API签名认证机制示例").
    fn documented(query: &str) -> Request<'_> {
        Request {
            method: "GET",
            host: "service.region.example.com",
            path: "/v1/77b6a44cba5143ab91d13ab9a8ff44fd/vpcs",
            query,
            content_type: "application/json",
            body: b"",
            date: "20191115T033655Z",
        }
    }

    fn query() -> String {
        canonical_query(&[
            ("marker", "13551d6b-755d-4757-b956-536f674975c0".into()),
            ("limit", "2".into()),
        ])
    }

    #[test]
    fn the_canonical_request_is_the_one_in_the_guide() {
        let query = query();
        let canonical = canonical_request(&documented(&query));

        assert_eq!(
            canonical,
            "GET\n/v1/77b6a44cba5143ab91d13ab9a8ff44fd/vpcs/\n\
             limit=2&marker=13551d6b-755d-4757-b956-536f674975c0\n\
             content-type:application/json\nhost:service.region.example.com\n\
             x-sdk-date:20191115T033655Z\n\ncontent-type;host;x-sdk-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(canonical.as_bytes()),
            "b25362e603ee30f4f25e7858e8a7160fd36e803bb2dfe206278659d71a9bcd7a"
        );
    }

    /// The guide masks its keys; the signature was computed apart (Python's hmac) from the
    /// guide's string to sign with an example SK.
    #[test]
    fn the_signature_is_the_hmac_of_the_string_to_sign() {
        let query = query();

        assert_eq!(
            authorization("ExampleAccessKey", "ExampleSecretKey", &documented(&query)),
            "SDK-HMAC-SHA256 Access=ExampleAccessKey, SignedHeaders=content-type;host;x-sdk-date, \
             Signature=2981050939e5bb43f346a6621a220cc35236d9923e094fa738c6fce55c88a4e9"
        );
    }

    #[test]
    fn values_and_dates_are_written_as_signed() {
        assert_eq!(
            canonical_query(&[
                ("start", "SYS.ECS.cpu_util.instance_id:i-1".into()),
                ("a", String::new())
            ]),
            "a=&start=SYS.ECS.cpu_util.instance_id%3Ai-1"
        );
        assert_eq!(canonical_uri("/V1.0/p/metrics/"), "/V1.0/p/metrics/");
        assert_eq!(sdk_date(1_573_788_615), "20191115T033015Z");
    }
}
