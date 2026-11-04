// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! STS AssumeRole as an `object_store` credential provider: the role's temporary key, asked for
//! with the base credentials and kept until shortly before it expires.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use object_store::{
    CredentialProvider,
    aws::{AwsCredential, AwsCredentialProvider},
};
use tokio::sync::Mutex;

use super::sigv4::{self, SigningKey};
use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

/// How long an assumed role's key is asked for.
const SESSION_SECS: u64 = 3600;

/// A key is replaced this long before it expires, so that no request is signed with one about
/// to lapse.
const REFRESH_MARGIN: Duration = Duration::from_secs(300);

const STS_TIMEOUT: Duration = Duration::from_secs(10);

const FORM: &str = "application/x-www-form-urlencoded; charset=utf-8";

pub struct AssumeRoleProvider {
    base: AwsCredentialProvider,
    role_arn: String,
    external_id: String,
    session_name: String,
    region: String,
    cached: Mutex<Option<(Arc<AwsCredential>, Instant)>>,
}

impl std::fmt::Debug for AssumeRoleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssumeRoleProvider")
            .field("role_arn", &self.role_arn)
            .field("session_name", &self.session_name)
            .finish_non_exhaustive()
    }
}

impl AssumeRoleProvider {
    pub fn new(
        base: AwsCredentialProvider,
        role_arn: String,
        external_id: String,
        session_name: String,
        region: String,
    ) -> Self {
        Self {
            base,
            role_arn,
            external_id,
            session_name,
            region,
            cached: Mutex::new(None),
        }
    }

    async fn assume(&self) -> Result<(AwsCredential, Option<DateTime<Utc>>)> {
        let base = self.base.get_credential().await.map_err(|error| {
            Error::unauthorized(format!(
                "no credentials to assume `{}` with: {error}",
                self.role_arn
            ))
        })?;

        let endpoint = super::sts_endpoint(&self.region);
        let host = endpoint.trim_start_matches("https://");

        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("Action", "AssumeRole")
            .append_pair("Version", "2011-06-15")
            .append_pair("RoleArn", &self.role_arn)
            .append_pair("RoleSessionName", &self.session_name)
            .append_pair("ExternalId", &self.external_id)
            .append_pair("DurationSeconds", &SESSION_SECS.to_string())
            .finish();

        let signed = sigv4::sign_post(
            &SigningKey {
                access_key: &base.key_id,
                secret_key: &base.secret_key,
                session_token: base.token.as_deref(),
            },
            super::region_or_default(&self.region),
            "sts",
            host,
            &[("content-type", FORM)],
            body.as_bytes(),
            &sigv4::amz_date_now(),
        );

        let client = reqwest::Client::builder()
            .timeout(STS_TIMEOUT)
            .build()
            .map_err(|error| Error::internal(format!("sts client: {error}")))?;

        let mut request = client
            .post(format!("{endpoint}/"))
            .header("content-type", FORM)
            .header("x-amz-date", &signed.amz_date)
            .header("authorization", &signed.authorization);

        if let Some(token) = base.token.as_deref().filter(|token| !token.is_empty()) {
            request = request.header("x-amz-security-token", token);
        }

        let response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
            .await
            .map_err(|error| Error::unavailable(format!("sts AssumeRole: {error}")))?;

        let status = response.status();

        let text = response
            .text()
            .await
            .map_err(|error| Error::unavailable(format!("sts AssumeRole: {error}")))?;

        if !status.is_success() {
            return Err(refusal(status.as_u16(), &text));
        }

        credentials(&text)
    }
}

#[async_trait]
impl CredentialProvider for AssumeRoleProvider {
    type Credential = AwsCredential;

    async fn get_credential(&self) -> object_store::Result<Arc<AwsCredential>> {
        let mut cached = self.cached.lock().await;

        if let Some((credential, fresh_until)) = cached.as_ref()
            && Instant::now() < *fresh_until
        {
            return Ok(credential.clone());
        }

        let (credential, expires_at) =
            self.assume()
                .await
                .map_err(|error| object_store::Error::Generic {
                    store: "S3",
                    source: Box::new(error),
                })?;

        let credential = Arc::new(credential);
        *cached = Some((credential.clone(), Instant::now() + lifetime(expires_at)));

        Ok(credential)
    }
}

/// How long a key that expires at `expires_at` is used before a new one is asked for. A key that
/// has already lapsed by this clock (a clock running ahead of AWS's) is asked for again soon,
/// not kept for a session's length.
fn lifetime(expires_at: Option<DateTime<Utc>>) -> Duration {
    let left = match expires_at {
        Some(at) => (at - Utc::now()).to_std().unwrap_or(Duration::ZERO),
        None => Duration::from_secs(SESSION_SECS),
    };

    left.saturating_sub(REFRESH_MARGIN)
        .max(Duration::from_secs(60))
}

/// The key in an `AssumeRoleResponse`, and when it expires.
fn credentials(xml: &str) -> Result<(AwsCredential, Option<DateTime<Utc>>)> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|error| Error::unavailable(format!("sts AssumeRole response: {error}")))?;

    let field = |name: &str| {
        doc.descendants()
            .find(|node| node.tag_name().name() == name)
            .and_then(|node| node.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };

    let missing = || Error::unavailable("sts AssumeRole response holds no credentials");

    let credential = AwsCredential {
        key_id: field("AccessKeyId").ok_or_else(missing)?.to_string(),
        secret_key: field("SecretAccessKey").ok_or_else(missing)?.to_string(),
        token: Some(field("SessionToken").ok_or_else(missing)?.to_string()),
    };

    let expires_at = field("Expiration")
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc));

    Ok((credential, expires_at))
}

/// An `ErrorResponse` as an error: the code and message STS gave, which say why the role was
/// not assumed (the trust policy, the external ID, the base key).
fn refusal(status: u16, xml: &str) -> Error {
    let doc = roxmltree::Document::parse(xml).ok();

    let field = |name: &str| {
        doc.as_ref().and_then(|doc| {
            doc.descendants()
                .find(|node| node.tag_name().name() == name)
                .and_then(|node| node.text())
                .map(|text| text.trim().to_string())
        })
    };

    let reason = match (field("Code"), field("Message")) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(code), None) => code,
        _ => format!("HTTP {status}"),
    };

    if (400..500).contains(&status) {
        Error::forbidden(format!("sts AssumeRole refused: {reason}"))
    } else {
        Error::unavailable(format!("sts AssumeRole failed: {reason}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <Credentials>
      <AccessKeyId>ASIAEXAMPLE</AccessKeyId>
      <SecretAccessKey>wJalrXUtnFEMI</SecretAccessKey>
      <SessionToken>FwoGZXIvYXdzEXAMPLE</SessionToken>
      <Expiration>2030-01-01T00:00:00Z</Expiration>
    </Credentials>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::123456789012:assumed-role/delivery/molesignal-c1</Arn>
    </AssumedRoleUser>
  </AssumeRoleResult>
</AssumeRoleResponse>"#;

    #[test]
    fn a_response_gives_the_key_and_when_it_expires() {
        let (credential, expires_at) = credentials(RESPONSE).unwrap();

        assert_eq!(credential.key_id, "ASIAEXAMPLE");
        assert_eq!(credential.secret_key, "wJalrXUtnFEMI");
        assert_eq!(credential.token.as_deref(), Some("FwoGZXIvYXdzEXAMPLE"));
        assert_eq!(
            expires_at.unwrap().to_rfc3339(),
            "2030-01-01T00:00:00+00:00"
        );
        assert!(credentials("<AssumeRoleResponse/>").is_err());
    }

    #[test]
    fn a_refusal_says_what_sts_said() {
        let xml = r#"<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <Error>
    <Type>Sender</Type>
    <Code>AccessDenied</Code>
    <Message>User: arn:aws:iam::1:user/x is not authorized to perform: sts:AssumeRole</Message>
  </Error>
</ErrorResponse>"#;

        let error = refusal(403, xml);
        assert!(matches!(error, Error::Forbidden(_)));
        assert!(
            error
                .to_string()
                .contains("AccessDenied: User: arn:aws:iam::1:user/x")
        );
        assert!(matches!(refusal(503, "not xml"), Error::Unavailable(_)));
        assert!(refusal(503, "not xml").to_string().contains("HTTP 503"));
    }

    #[test]
    fn a_key_is_renewed_before_it_lapses() {
        let in_an_hour = Utc::now() + chrono::Duration::seconds(3600);
        let renewed_after = lifetime(Some(in_an_hour));

        assert!(renewed_after <= Duration::from_secs(3300));
        assert!(renewed_after > Duration::from_secs(3200));
        assert_eq!(lifetime(Some(Utc::now())), Duration::from_secs(60));
        assert_eq!(
            lifetime(Some(Utc::now() - chrono::Duration::seconds(600))),
            Duration::from_secs(60)
        );
        assert_eq!(lifetime(None), Duration::from_secs(3300));
    }
}
