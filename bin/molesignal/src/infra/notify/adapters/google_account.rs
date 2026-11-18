// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google service accounts: the key file signs a JWT assertion that Google trades for an access
//! token (the OAuth 2.0 JWT bearer grant).
//!
//! A Google Chat app signs in as the service account itself. Gmail over SMTP uses domain-wide
//! delegation: the assertion names the mailbox (`subject`) the service account acts as.

use std::{
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kernel::{Error, Result};
use rustls::pki_types::PrivateKeyDer;
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use super::common::{provider_message, validate_http_url};

const JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Google's token endpoints. A key's `token_uri` must be one of them, so a signed assertion is
/// never sent anywhere else. Older keys carry the second one.
pub(crate) const GOOGLE_TOKEN_URIS: [&str; 2] = [
    "https://oauth2.googleapis.com/token",
    "https://accounts.google.com/o/oauth2/token",
];

fn default_token_uri() -> String {
    GOOGLE_TOKEN_URIS[0].into()
}

/// The parts of a service account key file (JSON) that signing in needs.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ServiceAccount {
    pub(crate) client_email: String,
    pub(crate) private_key: String,
    #[serde(default = "default_token_uri")]
    pub(crate) token_uri: String,
}

#[derive(Debug, Default, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

impl ServiceAccount {
    /// The key in a connector's config: a JSON object, or its text. `owner` is the connector
    /// type the errors name.
    pub(crate) fn parse(credentials: &Value, owner: &str) -> Result<Self> {
        let credentials = match credentials {
            Value::String(text) => serde_json::from_str(text).map_err(|_| {
                Error::invalid(format!(
                    "{owner} credentials must be a service account key (JSON)"
                ))
            })?,
            other => other.clone(),
        };

        let account: Self = serde_json::from_value(credentials).map_err(|_| {
            Error::invalid(format!(
                "{owner} credentials need the client_email and private_key of a service account"
            ))
        })?;

        signing_key(&account.private_key, owner)?;
        validate_http_url(&account.token_uri, &format!("{owner} token_uri"))?;

        Ok(account)
    }

    /// The key's `token_uri` is Google's. Checked before saving, testing and direct sends;
    /// parsing alone does not check it, so tests can sign in against a local server.
    pub(crate) fn validate_token_uri(&self, owner: &str) -> Result<()> {
        let token_uri = self.token_uri.trim().trim_end_matches('/');

        if GOOGLE_TOKEN_URIS.contains(&token_uri) {
            return Ok(());
        }

        Err(Error::invalid(format!(
            "{owner} credentials token_uri must be Google's token endpoint"
        )))
    }

    /// Trades a fresh assertion for an access token with `scope`, acting as `subject` when one is
    /// given. Returns the token and how many seconds it lasts.
    pub(crate) async fn fetch_token(
        &self,
        http: &reqwest::Client,
        scope: &str,
        subject: Option<&str>,
        owner: &str,
    ) -> Result<(String, u64)> {
        let assertion = self.assertion(scope, subject, unix_seconds()?, owner)?;

        let response = http_trace::send(
            http,
            http.post(self.token_uri.trim()).form(&[
                ("grant_type", JWT_BEARER),
                ("assertion", assertion.as_str()),
            ]),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable(format!("{owner} token request failed")))?;

        let token: TokenResponse = response.json().await.unwrap_or_default();

        if token.access_token.is_empty() {
            let reason = if token.error_description.is_empty() {
                &token.error
            } else {
                &token.error_description
            };

            return Err(Error::unavailable(format!(
                "{owner} token request was rejected: {}",
                provider_message(reason)
            )));
        }

        Ok((token.access_token, token.expires_in.max(60)))
    }

    /// The assertion: a JWT (RS256) signed by the service account, valid for an hour.
    pub(crate) fn assertion(
        &self,
        scope: &str,
        subject: Option<&str>,
        now: u64,
        owner: &str,
    ) -> Result<String> {
        let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());

        let mut claims = json!({
            "iss": self.client_email.trim(),
            "scope": scope,
            "aud": self.token_uri.trim(),
            "iat": now,
            "exp": now + 3_600,
        });

        if let Some(subject) = subject.map(str::trim).filter(|subject| !subject.is_empty()) {
            claims["sub"] = Value::String(subject.to_string());
        }

        let signed = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));

        let key = signing_key(&self.private_key, owner)?;
        let mut signature = vec![0; key.public_modulus_len()];

        key.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            signed.as_bytes(),
            &mut signature,
        )
        .map_err(|_| Error::internal(format!("{owner} could not sign its token request")))?;

        Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }
}

/// The service account's private key (PEM, PKCS#8 or PKCS#1).
fn signing_key(pem: &str, owner: &str) -> Result<RsaKeyPair> {
    let invalid = || Error::invalid(format!("{owner} private_key is not an RSA private key"));

    let key = rustls_pemfile::private_key(&mut Cursor::new(pem.as_bytes()))
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;

    match key {
        PrivateKeyDer::Pkcs8(der) => RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()),
        PrivateKeyDer::Pkcs1(der) => RsaKeyPair::from_der(der.secret_pkcs1_der()),
        _ => return Err(invalid()),
    }
    .map_err(|_| invalid())
}

fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| Error::internal("system time is before unix epoch"))
}

/// A 2048-bit RSA private key (PKCS#8 PEM) generated for a test.
#[cfg(test)]
pub(crate) fn test_key() -> String {
    use aws_lc_rs::{
        encoding::{AsDer, Pkcs8V1Der},
        rsa::KeySize,
    };
    use base64::engine::general_purpose::STANDARD;

    let key = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
    let der: Pkcs8V1Der<'static> = key.as_der().unwrap();
    let body = STANDARD.encode(der.as_ref());
    let lines = body
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    format!("-----BEGIN PRIVATE KEY-----\n{lines}\n-----END PRIVATE KEY-----\n")
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    use super::*;

    fn account(key: &str) -> ServiceAccount {
        ServiceAccount {
            client_email: "mole@project.iam.gserviceaccount.com".into(),
            private_key: key.into(),
            token_uri: default_token_uri(),
        }
    }

    fn claims(jwt: &str) -> Value {
        let payload = jwt.split('.').nth(1).unwrap();
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap()
    }

    #[test]
    fn the_assertion_is_a_jwt_signed_by_the_service_account() {
        let pem = test_key();
        let jwt = account(&pem)
            .assertion("https://mail.google.com/", None, 1_700_000_000, "test")
            .unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        let claims = claims(&jwt);
        assert_eq!(claims["iss"], "mole@project.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], GOOGLE_TOKEN_URIS[0]);
        assert_eq!(claims["exp"], 1_700_003_600);
        assert!(claims.get("sub").is_none());

        let key = signing_key(&pem, "test").unwrap();
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();

        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key().as_ref())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn delegation_names_the_mailbox_in_the_assertion() {
        let jwt = account(&test_key())
            .assertion(
                "https://mail.google.com/",
                Some(" alerts@example.com "),
                1_700_000_000,
                "test",
            )
            .unwrap();

        assert_eq!(claims(&jwt)["sub"], "alerts@example.com");
    }

    #[test]
    fn a_key_is_read_from_an_object_or_its_text_and_checked() {
        let key = json!({
            "client_email": "mole@project.iam.gserviceaccount.com",
            "private_key": test_key(),
        });

        assert!(ServiceAccount::parse(&key, "test").is_ok());
        assert!(ServiceAccount::parse(&Value::String(key.to_string()), "test").is_ok());
        assert!(ServiceAccount::parse(&json!({ "client_email": "x" }), "test").is_err());
        assert!(
            ServiceAccount::parse(
                &json!({ "client_email": "x", "private_key": "not a key" }),
                "test"
            )
            .is_err()
        );

        let mut elsewhere = account(&test_key());
        elsewhere.token_uri = "https://attacker.example/token".into();
        assert!(elsewhere.validate_token_uri("test").is_err());
        assert!(account(&test_key()).validate_token_uri("test").is_ok());
    }
}
