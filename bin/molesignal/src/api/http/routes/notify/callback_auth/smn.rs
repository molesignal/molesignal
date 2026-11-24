// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The signature of Huawei Cloud SMN messages pushed to an HTTP(S) subscription.
//!
//! The signing certificate is fetched from `signing_cert_url`, accepted only over https from
//! `smn.<region>.myhuaweicloud.com` (or `.eu`) and ending in `.pem`. The signed text is a list
//! of names and values, one per line, in a fixed order by message type; the signature is
//! RSA PKCS#1 v1.5 with SHA-256, Base64 in `signature`
//! (<https://support.huaweicloud.com/usermanual-smn/smn_ug_a9003.html>,
//! <https://support.huaweicloud.com/usermanual-smn/smn_ug_a9004.html>). A subscription
//! confirmation, once verified, is completed by visiting its `subscribe_url`.

use std::time::Duration;

use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use base64::Engine as _;
use kernel::{Error, Result};
use serde_json::Value;
use url::Url;

use super::{public_key_of, text};

/// How long fetching the certificate and confirming a subscription may take.
const SMN_TIMEOUT: Duration = Duration::from_secs(5);

/// Domains of Huawei Cloud: SMN serves certificates and the console confirms subscriptions there.
const HUAWEI_CLOUD_DOMAINS: [&str; 4] = [
    "myhuaweicloud.com",
    "huaweicloud.com",
    "myhuaweicloud.eu",
    "huaweicloud.eu",
];

/// A verified SMN message: a notification gives its `message`; a subscription confirmation is
/// completed and gives `None`. A wrong signature is refused.
pub(in crate::api::http::routes) async fn smn_checked(notice: &Value) -> Result<Option<&str>> {
    verify_smn(notice).await?;

    match text(notice, "type") {
        "SubscriptionConfirmation" => {
            let subscribe = console_url(text(notice, "subscribe_url"))?;

            client()?
                .get(subscribe)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| Error::unavailable("the SMN subscription could not be confirmed"))?;

            Ok(None)
        }
        "Notification" => Ok(Some(text(notice, "message"))),
        _ => Ok(None),
    }
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(SMN_TIMEOUT)
        .build()
        .map_err(|_| Error::internal("SMN client"))
}

async fn verify_smn(notice: &Value) -> Result<()> {
    let certificate = certificate_url(text(notice, "signing_cert_url"))?;

    let pem = client()?
        .get(certificate)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| Error::unavailable("the SMN signing certificate could not be fetched"))?
        .text()
        .await
        .map_err(|_| Error::unavailable("the SMN signing certificate could not be read"))?;

    check_smn_signature(notice, &pem)
}

/// Checks the signature of an SMN message with the signing certificate.
fn check_smn_signature(notice: &Value, pem: &str) -> Result<()> {
    if !text(notice, "signature_version").eq_ignore_ascii_case("v1") {
        return Err(Error::unauthorized("unknown SMN signature version"));
    }

    let signature = base64::engine::general_purpose::STANDARD
        .decode(text(notice, "signature"))
        .map_err(|_| Error::unauthorized("the SMN signature is not Base64"))?;

    let public_key = public_key_of(pem)?;

    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &public_key)
        .verify(string_to_sign(notice).as_bytes(), &signature)
        .map_err(|_| Error::unauthorized("invalid SMN signature"))
}

/// What SMN signs for the message type: names and values, each on its own line.
fn string_to_sign(notice: &Value) -> String {
    let names: &[&str] = match text(notice, "type") {
        "Notification" => &[
            "message",
            "message_id",
            "subject",
            "timestamp",
            "topic_urn",
            "type",
        ],
        _ => &[
            "message",
            "message_id",
            "subscribe_url",
            "timestamp",
            "topic_urn",
            "type",
        ],
    };

    names
        .iter()
        .filter(|name| notice.get(**name).and_then(Value::as_str).is_some())
        .map(|name| format!("{name}\n{}\n", text(notice, name)))
        .collect()
}

/// Only SMN's certificates: `https://smn.<region>.myhuaweicloud.com/…/*.pem` (or `.eu`).
fn certificate_url(value: &str) -> Result<Url> {
    let refused = || Error::unauthorized("not an SMN signing certificate");
    let url = Url::parse(value).map_err(|_| refused())?;
    let host = url.host_str().unwrap_or_default();

    let region = host.strip_prefix("smn.").and_then(|rest| {
        rest.strip_suffix(".myhuaweicloud.com")
            .or_else(|| rest.strip_suffix(".myhuaweicloud.eu"))
    });

    let region_ok = region.is_some_and(|region| {
        !region.is_empty()
            && region
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });

    if url.scheme() != "https" || !region_ok || !url.path().ends_with(".pem") {
        return Err(refused());
    }

    Ok(url)
}

/// Only Huawei Cloud addresses over https, such as `https://console.huaweicloud.com/smn/…`.
fn console_url(value: &str) -> Result<Url> {
    let refused = || Error::unauthorized("not a Huawei Cloud address");
    let url = Url::parse(value).map_err(|_| refused())?;
    let host = url.host_str().unwrap_or_default();

    let huawei = HUAWEI_CLOUD_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    });

    if url.scheme() != "https" || !huawei {
        return Err(refused());
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_smn_certificates_are_fetched() {
        assert!(
            certificate_url(
                "https://smn.cn-north-4.myhuaweicloud.com/smn/SMN_cn-north-4_3190c26a.pem"
            )
            .is_ok()
        );
        assert!(certificate_url("https://smn.eu-west-101.myhuaweicloud.eu/smn/x.pem").is_ok());
        assert!(certificate_url("http://smn.cn-north-4.myhuaweicloud.com/smn/x.pem").is_err());
        assert!(certificate_url("https://smn.cn-north-4.myhuaweicloud.com/smn/x.txt").is_err());
        assert!(
            certificate_url("https://smn.cn-north-4.myhuaweicloud.com.evil.com/x.pem").is_err()
        );
        assert!(certificate_url("https://evil.com/smn.cn-north-4.myhuaweicloud.com.pem").is_err());
        assert!(certificate_url("https://obs.cn-north-4.myhuaweicloud.com/x.pem").is_err());
    }

    #[test]
    fn subscriptions_are_confirmed_only_at_huawei_cloud() {
        assert!(console_url("https://console.huaweicloud.com/smn/subscription/confirm").is_ok());
        assert!(console_url("https://console-intl.huaweicloud.com/smn/confirm").is_ok());
        assert!(console_url("http://console.huaweicloud.com/smn/confirm").is_err());
        assert!(console_url("https://evilhuaweicloud.com/smn/confirm").is_err());
        assert!(console_url("https://console.huaweicloud.com.evil.com/").is_err());
    }

    #[test]
    fn a_notification_signs_its_fields_in_order_and_a_confirmation_its_own() {
        let notification = json!({
            "type": "Notification",
            "message": "{}",
            "message_id": "m-1",
            "timestamp": "2024-07-10T09:45:00Z",
            "topic_urn": "urn:smn:region01:p:t",
            "signature_version": "v1",
        });

        assert_eq!(
            string_to_sign(&notification),
            "message\n{}\nmessage_id\nm-1\ntimestamp\n2024-07-10T09:45:00Z\ntopic_urn\n\
             urn:smn:region01:p:t\ntype\nNotification\n"
        );

        let confirmation = json!({
            "type": "SubscriptionConfirmation",
            "message": "invited",
            "message_id": "m-2",
            "subscribe_url": "https://console.huaweicloud.com/smn/subscription/confirm",
            "timestamp": "2024-07-10T09:43:44Z",
            "topic_urn": "urn:smn:region01:p:t",
        });

        assert_eq!(
            string_to_sign(&confirmation),
            "message\ninvited\nmessage_id\nm-2\nsubscribe_url\n\
             https://console.huaweicloud.com/smn/subscription/confirm\ntimestamp\n\
             2024-07-10T09:43:44Z\ntopic_urn\nurn:smn:region01:p:t\ntype\nSubscriptionConfirmation\n"
        );
    }

    /// A self-signed test certificate, and a notification signed with its key
    /// (`openssl dgst -sha256 -sign`).
    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIC6DCCAdCgAwIBAgIUIz0YFqn0hHMb/0FFefT5II28gr4wDQYJKoZIhvcNAQEL\nBQAwEzERMA8GA1UEAwwIc21uLnRlc3QwIBcNMjYxMTI0MDcyODE2WhgPMjEyNjEw\nMzEwNzI4MTZaMBMxETAPBgNVBAMMCHNtbi50ZXN0MIIBIjANBgkqhkiG9w0BAQEF\nAAOCAQ8AMIIBCgKCAQEAoNvrI2U81EPDxv3z+4Ng8B2Xv+oQfLuq09PTlONgS/ua\nD9NmEacHtVKv6LIeC8ABBjtck3EB46plOocv0AfsJ4mWOZtxk61J42TgLb72UjPO\nF+BPBWvT8Ow65gJ8bVQ7yz5VpL862VYsf1N924lfTsbxemIvzwIdY0xQs6euOZag\nGsNLW8R6/dYvXY8pcoTjpCV2TFesi3gQXS8tnrEuEpbFRvFfcD5yWxxskfxzQ/7/\nu7lc8tDByBvZXOVyf6uNGTnwNXDATXUSNROgzxOFcTc7zdotMydBa02Wfhb6tY8x\nliqDBW9OPENfAPQ+voNvrVUlgCGVhHiX+7TPIRY1kwIDAQABozIwMDAdBgNVHQ4E\nFgQUpgSkjVqbHubkmcoU2dNUHSw7c58wDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG\n9w0BAQsFAAOCAQEAa4GARfjEUD2H5a0ZTjVnm5iaPbFoPhMrNct5soFGDH9uAoMx\nfpLkf1AcLmKRmIzm+ZWrdNQgY+K75mphIvr4GIkaTHynLLVG0Jo44d1OZY3Zjzhf\ndvacBR0cLOrm1JNzt/UEw60LKXE4RzbKkJ1M9SBGz6g/NxuQJlka6ftT4dzQeKp7\nrHwQgwmLDt+o8XGdmfjR8sbcLOwJbsjkgCxDn3stwyXWKPwL+SuDBs5mVUO46g+j\n+H+Y2uBICFu7wZO/DLrsmyQPjfjTB+4anmRaLbfQPOjrFaCtInmHoXTTDt7AGtLO\nsrBBZ6E2FfJY3+/MNjGLxm9/UTsNTA1/S3dxQQ==\n-----END CERTIFICATE-----";
    const TEST_SIGNATURE: &str = "VtLJeFT0umwbUkHs7KFcaUerCi1enZQRWGgRjt6fU5TGbU5aYWBdlFi0sfkSYBH5xrWbHZzUNxA19Q7q2JsogXrGhrDfmafnGhPMV0oLsIDW1PNMP3uLrGfliO32gRgpLNxD8+BYo0EkvZYD0gbzFXntiAb8V3Y6tcQ0RzW/hpxfXAFRKwum2VJ5XOP+crSoyg/aZfd5j9YlO/luznscrMJtanjKo6b/Zin11EYSJIl0ZUvnxuAC03bEPFfoVsuIKw6semJfjgJQAgHR/oFHh30K5TmpdF1TptOUElIz2H6uk4xWirZBw8TV7567eBo/jDc7JGHb/EZvmEgYVl+mLQ==";

    fn signed_notice() -> Value {
        json!({
            "type": "Notification",
            "signature_version": "v1",
            "signature": TEST_SIGNATURE,
            "subject": "SMN",
            "topic_urn": "urn:smn:cn-north-4:0a419ac94f80f2c62f14c01e34000000:ces_alarms",
            "message_id": "d84bd6629ef04513ad2e37bffd6e17cb",
            "message": r#"{"alarm_id":"al-1"}"#,
            "unsubscribe_url": "https://console.huaweicloud.com/smn/subscription/unsubscribe",
            "signing_cert_url": "https://smn.cn-north-4.myhuaweicloud.com/smn/SMN_cn-north-4_0000.pem",
            "timestamp": "2026-05-12T01:47:37Z"
        })
    }

    #[test]
    fn an_smn_notification_is_taken_only_with_its_signature() {
        assert!(check_smn_signature(&signed_notice(), TEST_CERT).is_ok());

        let mut changed = signed_notice();
        changed["message"] = json!(r#"{"alarm_id":"al-2"}"#);
        assert!(check_smn_signature(&changed, TEST_CERT).is_err());

        let mut without_subject = signed_notice();
        without_subject.as_object_mut().unwrap().remove("subject");
        assert!(check_smn_signature(&without_subject, TEST_CERT).is_err());

        let mut unknown = signed_notice();
        unknown["signature_version"] = json!("v2");
        assert!(check_smn_signature(&unknown, TEST_CERT).is_err());

        assert!(check_smn_signature(&signed_notice(), "not a certificate").is_err());
    }
}
