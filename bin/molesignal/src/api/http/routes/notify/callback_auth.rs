// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 服务商回调的签名：Twilio 的 `X-Twilio-Signature`，AWS SNS 消息的签名。
//!
//! - Twilio：对请求地址加上按名字排好的表单参数（名字紧跟值，不加分隔）算 HMAC-SHA1，密钥是账号的
//!   Auth Token，Base64 后与请求头比较。
//! - SNS：从 `SigningCertURL` 取签名证书（只认 `sns.<region>.amazonaws.com` 下的 `.pem`），按消息类型
//!   拼出待签内容验 RSA 签名（`SignatureVersion` 1 是 SHA1，2 是 SHA256）。订阅确认验过后访问
//!   `SubscribeURL` 完成订阅。连接器设了 `replyTopicArn` 时只认这个主题。

use std::{collections::BTreeMap, time::Duration};

use aws_lc_rs::{
    constant_time, hmac,
    signature::{
        RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey,
    },
};
use base64::Engine as _;
use serde_json::Value;
use url::Url;

use crate::{
    domain::notify::connector::NotifyConnector,
    shared::{Error, Result},
};

/// 取证书、确认订阅最多等多久。
const SNS_TIMEOUT: Duration = Duration::from_secs(5);

/// Twilio 的签名对不对。
pub(super) fn twilio_signature_matches(
    auth_token: &str,
    url: &str,
    form: &BTreeMap<String, String>,
    presented: &str,
) -> bool {
    if auth_token.trim().is_empty() || presented.trim().is_empty() {
        return false;
    }

    let mut signed = url.to_string();

    for (name, value) in form {
        signed.push_str(name);
        signed.push_str(value);
    }

    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, auth_token.as_bytes());
    let expected =
        base64::engine::general_purpose::STANDARD.encode(hmac::sign(&key, signed.as_bytes()));

    constant_time::verify_slices_are_equal(expected.as_bytes(), presented.trim().as_bytes()).is_ok()
}

/// 验过签名的 SNS 消息：通知时返回 `Message` 里的内容；订阅确认时确认订阅、返回 `None`。签名不对、
/// 主题不是设的那个时拒绝。
pub(super) async fn sns_verified(
    connector: &NotifyConnector,
    notice: &Value,
) -> Result<Option<Value>> {
    if let Some(topic) = connector
        .config
        .get("replyTopicArn")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        && text(notice, "TopicArn") != topic
    {
        return Err(Error::unauthorized("the SNS message is from another topic"));
    }

    let Some(message) = sns_checked(notice).await? else {
        return Ok(None);
    };

    let message = serde_json::from_str(message)
        .map_err(|_| Error::invalid("the SNS message is not a two-way SMS"))?;

    Ok(Some(message))
}

/// 验过签名的 SNS 消息：通知时返回 `Message` 的原文；订阅确认时确认订阅、返回 `None`。签名不对时拒绝。
pub(in crate::api::http::routes) async fn sns_checked(notice: &Value) -> Result<Option<&str>> {
    verify_sns(notice).await?;

    match text(notice, "Type") {
        "SubscriptionConfirmation" => {
            let subscribe = sns_url(text(notice, "SubscribeURL"))?;

            reqwest::Client::builder()
                .timeout(SNS_TIMEOUT)
                .build()
                .map_err(|_| Error::internal("SNS client"))?
                .get(subscribe)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| Error::unavailable("the SNS subscription could not be confirmed"))?;

            Ok(None)
        }
        "Notification" => Ok(Some(text(notice, "Message"))),
        _ => Ok(None),
    }
}

async fn verify_sns(notice: &Value) -> Result<()> {
    let certificate = sns_url(text(notice, "SigningCertURL"))?;

    if !certificate.path().ends_with(".pem") {
        return Err(Error::unauthorized("not an SNS signing certificate"));
    }

    let pem = reqwest::Client::builder()
        .timeout(SNS_TIMEOUT)
        .build()
        .map_err(|_| Error::internal("SNS client"))?
        .get(certificate)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| Error::unavailable("the SNS signing certificate could not be fetched"))?
        .text()
        .await
        .map_err(|_| Error::unavailable("the SNS signing certificate could not be read"))?;

    check_sns_signature(notice, &pem)
}

/// 用签名证书验一条 SNS 消息的签名。
fn check_sns_signature(notice: &Value, pem: &str) -> Result<()> {
    let signature = base64::engine::general_purpose::STANDARD
        .decode(text(notice, "Signature"))
        .map_err(|_| Error::unauthorized("the SNS signature is not Base64"))?;

    let public_key = public_key_of(pem)?;

    let algorithm = match text(notice, "SignatureVersion") {
        "1" => &RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY,
        "2" => &RSA_PKCS1_2048_8192_SHA256,
        _ => return Err(Error::unauthorized("unknown SNS signature version")),
    };

    UnparsedPublicKey::new(algorithm, &public_key)
        .verify(string_to_sign(notice).as_bytes(), &signature)
        .map_err(|_| Error::unauthorized("invalid SNS signature"))
}

/// SNS 按消息类型签的内容：一组名字与值，各占一行。
fn string_to_sign(notice: &Value) -> String {
    let names: &[&str] = match text(notice, "Type") {
        "Notification" => &[
            "Message",
            "MessageId",
            "Subject",
            "Timestamp",
            "TopicArn",
            "Type",
        ],
        _ => &[
            "Message",
            "MessageId",
            "SubscribeURL",
            "Timestamp",
            "Token",
            "TopicArn",
            "Type",
        ],
    };

    names
        .iter()
        .filter(|name| notice.get(**name).and_then(Value::as_str).is_some())
        .map(|name| format!("{name}\n{}\n", text(notice, name)))
        .collect()
}

/// 只认 AWS 的 SNS 地址：`https://sns.<region>.amazonaws.com(.cn)/…`。
fn sns_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::unauthorized("not an SNS address"))?;
    let host = url.host_str().unwrap_or_default();

    let region = host
        .strip_prefix("sns.")
        .and_then(|rest| {
            rest.strip_suffix(".amazonaws.com")
                .or_else(|| rest.strip_suffix(".amazonaws.com.cn"))
        })
        .filter(|region| {
            !region.is_empty()
                && region
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });

    if url.scheme() != "https" || region.is_none() {
        return Err(Error::unauthorized("not an SNS address"));
    }

    Ok(url)
}

/// 证书里的 RSA 公钥。
fn public_key_of(pem: &str) -> Result<Vec<u8>> {
    use x509_cert::{Certificate, der::Decode};

    let parsed = pem::parse(pem.trim().as_bytes())
        .map_err(|_| Error::unauthorized("the SNS signing certificate is not PEM"))?;

    let certificate = Certificate::from_der(parsed.contents())
        .map_err(|_| Error::unauthorized("the SNS signing certificate cannot be read"))?;

    certificate
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::unauthorized("the SNS signing key cannot be read"))
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_twilio_signature_covers_the_address_and_the_sorted_form() {
        // twilio-python 验签测试用的那组。
        let form = BTreeMap::from([
            ("CallSid".to_string(), "CA1234567890ABCDE".to_string()),
            ("Caller".to_string(), "+14158675309".to_string()),
            ("Digits".to_string(), "1234".to_string()),
            ("From".to_string(), "+14158675309".to_string()),
            ("To".to_string(), "+18005551212".to_string()),
        ]);
        let url = "https://mycompany.com/myapp.php?foo=1&bar=2";
        let signature = "RSOYDt4T1cUTdK1PDd93/VVr8B8=";

        assert!(twilio_signature_matches("12345", url, &form, signature));
        assert!(!twilio_signature_matches("54321", url, &form, signature));
        assert!(!twilio_signature_matches(
            "12345",
            "https://mycompany.com/myapp.php",
            &form,
            signature
        ));
        assert!(!twilio_signature_matches("", url, &form, signature));
    }

    #[test]
    fn only_aws_sns_addresses_are_fetched() {
        assert!(
            sns_url("https://sns.us-east-1.amazonaws.com/SimpleNotificationService-1.pem").is_ok()
        );
        assert!(sns_url("https://sns.cn-north-1.amazonaws.com.cn/x.pem").is_ok());
        assert!(sns_url("http://sns.us-east-1.amazonaws.com/x.pem").is_err());
        assert!(sns_url("https://sns.us-east-1.amazonaws.com.evil.com/x.pem").is_err());
        assert!(sns_url("https://evil.com/sns.us-east-1.amazonaws.com").is_err());
    }

    /// 测试用的自签证书与它的私钥签的一条通知（`openssl dgst -sha256 -sign`）。
    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIDCTCCAfGgAwIBAgIUKwqr63hwdthYrn7IUKEGjsRB4qUwDQYJKoZIhvcNAQEL\nBQAwEzERMA8GA1UEAwwIc25zLnRlc3QwIBcNMjYxMDI3MTg0NzQ1WhgPMjEyNjEw\nMDMxODQ3NDVaMBMxETAPBgNVBAMMCHNucy50ZXN0MIIBIjANBgkqhkiG9w0BAQEF\nAAOCAQ8AMIIBCgKCAQEAtOplZpsrLOaLUSlLhkxFueN4JxCi1717dVpedjFRz785\n7IpF9HSbLGETttUBM+8cNOcxzlz4kGuCaxFcHPGr6P0ClaX2iqqiOp12M1wJQ8a2\n4qrVeSP+gzF+zqCt/giezOFz70YGeAJuEIzn3MzeH3iMj5tyOoLiUzTTH8iNDBet\nJnHere0q1oRb+eDW5aRAUUqD4+GYfTgQAcSbBMCCgH03IYOuQ+LmOiskayykvWCo\nut319GUCdVlWoid6Aw0TatYwFKhsTq3hNsQ8X0kU/kAIKuL0r3GgNvgCty+zg9GZ\nt8BO+IDaEO5+MRBn81/nnrJqR2kOjD36Gs3aBbWbRQIDAQABo1MwUTAdBgNVHQ4E\nFgQUG14IaUU5BJKWgiMfYXk6/e2Oq9EwHwYDVR0jBBgwFoAUG14IaUU5BJKWgiMf\nYXk6/e2Oq9EwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAdl/y\n3xMD6M2KvL2hV2ZxKLIAWY3e46fVrEezgUYNXKp/q8ubF7Imkf5NgZfrlF/izxcY\nFy2O5E8iO/cJGONbDCg8hMzDqYMvPfKYhPbGbSNdoGhdm6GWfG3SXjwyxkx+E+kr\n3cB55WaRN/G3JzjvJuQnbWDVdjaa+ItMlTsJQU2w2OOcj3b3bfZfAgx2zncgbQ9C\nxvLbvE8bv9s/1euODEdeGhUH6HJk6E102JvkdeauYo2cdKRxJ0I29Zb0i6NJ8k5f\nGNi2M0s7HczdyO8ZkI0j/AOPh3vRV91fTCgxiGUt2ec7K9FgrzUkwvpoRDj89560\nWYvdHdpnmtn9SKj75g==\n-----END CERTIFICATE-----";
    const TEST_SIGNATURE: &str = "phVHiycGZFtIdP8iuJZSf3Y4Z5nlsaqQbiUtnp804da92oL5xC/X68UC9w2s3dan9nj5YsktmUjrSqMSIbQSlUXQNtLwm05fB60BHwdaIfOLTNCvnZxsGdETLyF9BqQ71afcu/bOHp9+NEtzV2ynex625O89mAFew77eIQWQv9Bi54FDuQ6DjXBQpphQQyMf6ywt05IkCQhk6Wkprrjkw13J9V4+GtvyE09O4I89mSJS8GRznMYiTOShgoNW1616Rr10/CVSEC64avyK2oefMO6jup0xVSrR++13jJntzw/lZifEaHMKpAylCaI03NFay8yzQibKQ1bDIVEdnElO8g==";

    fn signed_notice() -> Value {
        json!({
            "Type": "Notification",
            "MessageId": "m-1",
            "TopicArn": "arn:aws:sns:us-east-1:1:replies",
            "Message": r#"{"originationNumber":"+14155550100","messageBody":"31"}"#,
            "Timestamp": "2026-10-28T00:00:00.000Z",
            "SignatureVersion": "2",
            "Signature": TEST_SIGNATURE,
        })
    }

    #[test]
    fn an_sns_notification_is_taken_only_with_its_signature() {
        assert!(check_sns_signature(&signed_notice(), TEST_CERT).is_ok());

        let mut changed = signed_notice();
        changed["Message"] = json!(r#"{"originationNumber":"+14155550100","messageBody":"32"}"#);
        assert!(check_sns_signature(&changed, TEST_CERT).is_err());

        let mut older = signed_notice();
        older["SignatureVersion"] = json!("1");
        assert!(
            check_sns_signature(&older, TEST_CERT).is_err(),
            "SHA1 does not match a SHA256 signature"
        );

        let mut unknown = signed_notice();
        unknown["SignatureVersion"] = json!("3");
        assert!(check_sns_signature(&unknown, TEST_CERT).is_err());

        assert!(check_sns_signature(&signed_notice(), "not a certificate").is_err());
    }

    #[test]
    fn a_notification_signs_its_fields_in_order() {
        let notice = json!({
            "Type": "Notification",
            "MessageId": "m-1",
            "TopicArn": "arn:aws:sns:us-east-1:1:replies",
            "Message": "{}",
            "Timestamp": "2026-10-28T00:00:00.000Z",
            "SignatureVersion": "2",
        });

        assert_eq!(
            string_to_sign(&notice),
            "Message\n{}\nMessageId\nm-1\nTimestamp\n2026-10-28T00:00:00.000Z\nTopicArn\narn:aws:sns:us-east-1:1:replies\nType\nNotification\n"
        );
    }
}
