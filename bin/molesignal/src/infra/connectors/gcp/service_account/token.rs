// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Access tokens for a Google service account: the key signs a JWT assertion (RS256) that
//! Google trades for a token (the OAuth 2.0 JWT bearer grant), for one scope.
//!
//! The runner builds a new connector on every tick, so tokens are kept process-wide, by service
//! account and a digest of its key (a new key takes a new token), until a few minutes before they
//! expire.

use std::{
    io::Cursor,
    sync::LazyLock,
    time::{Duration, Instant},
};

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kernel::{Error, Result, time::TimestampMicros};
use moka::{Expiry, future::Cache};
use rustls::pki_types::PrivateKeyDer;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use signals::http_trace::{self, HttpTarget};

use super::ServiceAccount;

const JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// How long before it expires a token is no longer handed out.
const EXPIRY_MARGIN_SECS: u64 = 300;

#[derive(Clone)]
struct Token {
    value: String,
    lifetime: Duration,
}

/// Each token is kept for its own lifetime.
struct UntilExpiry;

impl Expiry<String, Token> for UntilExpiry {
    fn expire_after_create(&self, _key: &String, token: &Token, _at: Instant) -> Option<Duration> {
        Some(token.lifetime)
    }
}

static TOKENS: LazyLock<Cache<String, Token>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(1024)
        .expire_after(UntilExpiry)
        .build()
});

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

/// Where the tokens of one service account for one scope come from.
pub struct TokenSource<'a> {
    account: &'a ServiceAccount,
    scope: &'static str,
}

impl<'a> TokenSource<'a> {
    pub fn new(account: &'a ServiceAccount, scope: &'static str) -> Self {
        Self { account, scope }
    }

    fn key(&self) -> String {
        let digest = Sha256::digest(self.account.private_key.as_bytes());

        format!(
            "{}:{}:{}",
            self.account.client_email,
            self.scope,
            hex::encode(&digest[..8])
        )
    }

    /// A token: the kept one, or a new one.
    pub async fn token(&self, http: &reqwest::Client) -> Result<String> {
        let key = self.key();

        if let Some(token) = TOKENS.get(&key).await {
            return Ok(token.value);
        }

        let token = self.fetch(http).await?;

        TOKENS.insert(key, token.clone()).await;
        Ok(token.value)
    }

    /// Drops the kept token, after Google refused it.
    pub async fn forget(&self) {
        TOKENS.invalidate(&self.key()).await;
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Token> {
        let now = TimestampMicros::now().0 / 1_000_000;
        let assertion = assertion(self.account, self.scope, now)?;

        let request = http.post(&self.account.token_uri).form(&[
            ("grant_type", JWT_BEARER),
            ("assertion", assertion.as_str()),
        ]);

        let response = http_trace::send(http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|e| Error::unavailable(format!("google token request: {e}")))?;

        let status = response.status().as_u16();

        let body = response
            .text()
            .await
            .map_err(|e| Error::unavailable(format!("google token response: {e}")))?;

        parse(status, &body)
    }
}

/// The assertion: a JWT (RS256) signed by the service account, asking for `scope` for an hour.
pub(super) fn assertion(account: &ServiceAccount, scope: &str, now: i64) -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());

    let claims = json!({
        "iss": account.client_email,
        "scope": scope,
        "aud": account.token_uri,
        "iat": now,
        "exp": now + 3_600,
    });

    let signed = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
    let key = signing_key(&account.private_key)?;
    let mut signature = vec![0; key.public_modulus_len()];

    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| Error::internal("could not sign the google token request"))?;

    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

/// The service account's private key (PEM, PKCS#8 or PKCS#1).
pub(super) fn signing_key(pem: &str) -> Result<RsaKeyPair> {
    let invalid =
        || Error::invalid("the private_key of `service_account_key` is not an RSA private key");

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

/// A token response: the token and how long it may be kept, or why there is none.
fn parse(status: u16, body: &str) -> Result<Token> {
    let response: TokenResponse = serde_json::from_str(body).unwrap_or_default();

    if (200..300).contains(&status) && !response.access_token.is_empty() {
        let expires_in = if response.expires_in == 0 {
            3_600
        } else {
            response.expires_in
        };

        return Ok(Token {
            value: response.access_token,
            lifetime: Duration::from_secs(expires_in.saturating_sub(EXPIRY_MARGIN_SECS).max(60)),
        });
    }

    let reason = match (response.error.as_str(), response.error_description.as_str()) {
        ("", "") => body.chars().take(300).collect::<String>(),
        (error, "") => error.to_string(),
        (error, description) => format!("{error}: {description}"),
    };

    let message = format!("google refused the service account key ({status}): {reason}");

    // A deleted key or service account comes back as 400 `invalid_grant` or 401.
    match status {
        400 | 401 => Err(Error::unauthorized(message)),
        403 => Err(Error::forbidden(message)),
        429 => Err(Error::resource_exhausted(message)),
        _ => Err(Error::unavailable(message)),
    }
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
    use serde_json::Value;

    use super::*;

    #[test]
    fn the_assertion_is_signed_by_the_key_and_asks_for_read_access() {
        let account = ServiceAccount {
            client_email: "reader@p.iam.gserviceaccount.com".into(),
            private_key: test_key(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
            project_id: "p".into(),
        };

        let jwt = assertion(
            &account,
            "https://www.googleapis.com/auth/pubsub",
            1_700_000_000,
        )
        .unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();

        assert_eq!(parts.len(), 3);

        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();

        assert_eq!(claims["iss"], "reader@p.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], "https://www.googleapis.com/auth/pubsub");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["exp"], 1_700_003_600);

        let key = signing_key(&account.private_key).unwrap();
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key().as_ref());
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();

        public
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn a_token_is_kept_until_shortly_before_it_expires() {
        let token = parse(
            200,
            r#"{"access_token":"ya29.c","expires_in":3599,"token_type":"Bearer"}"#,
        )
        .unwrap();

        assert_eq!(token.value, "ya29.c");
        assert_eq!(token.lifetime, Duration::from_secs(3_299));
    }

    #[test]
    fn a_refusal_says_why() {
        let error = parse(
            400,
            r#"{"error":"invalid_grant","error_description":"Invalid JWT Signature."}"#,
        )
        .err()
        .unwrap();

        assert!(matches!(error, Error::Unauthorized(_)));
        assert!(error.to_string().contains("Invalid JWT Signature."));
    }
}
