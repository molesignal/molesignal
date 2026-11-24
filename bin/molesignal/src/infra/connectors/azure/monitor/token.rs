// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Access tokens from Microsoft Entra ID, by the client credentials grant.
//!
//! A pull takes two: one for Azure Resource Manager (Resource Graph) and one for the metrics data
//! plane. The runner builds a new connector on every tick, so tokens are kept process-wide, by
//! tenant, client, scope and a digest of the secret (a new secret takes a new token), until a few
//! minutes before they expire.

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use kernel::{Error, Result};
use moka::{Expiry, future::Cache};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use signals::http_trace::{self, HttpTarget};

use super::settings::Settings;

/// The scope of Azure Resource Manager, which Resource Graph is part of.
pub(super) const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// The scope of the Azure Monitor metrics data plane (`*.metrics.monitor.azure.com`).
pub(super) const METRICS_SCOPE: &str = "https://metrics.monitor.azure.com/.default";

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
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

/// Where the tokens of one app registration for one scope come from.
pub(super) struct TokenSource<'a> {
    settings: &'a Settings,
    scope: &'static str,
}

impl<'a> TokenSource<'a> {
    pub(super) fn new(settings: &'a Settings, scope: &'static str) -> Self {
        Self { settings, scope }
    }

    fn key(&self) -> String {
        let digest = Sha256::digest(self.settings.client_secret.as_bytes());

        format!(
            "{}:{}:{}:{}",
            self.settings.tenant_id,
            self.settings.client_id,
            self.scope,
            hex::encode(&digest[..8])
        )
    }

    /// A token for the scope: the kept one, or a new one.
    pub(super) async fn token(&self, http: &reqwest::Client) -> Result<String> {
        let key = self.key();

        if let Some(token) = TOKENS.get(&key).await {
            return Ok(token.value);
        }

        let token = self.fetch(http).await?;

        TOKENS.insert(key, token.clone()).await;
        Ok(token.value)
    }

    /// Drops the kept token, after Azure refused it.
    pub(super) async fn forget(&self) {
        TOKENS.invalidate(&self.key()).await;
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Token> {
        let url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            self.settings.tenant_id
        );

        let request = http.post(url).form(&[
            ("grant_type", "client_credentials"),
            ("client_id", self.settings.client_id.as_str()),
            ("client_secret", self.settings.client_secret.as_str()),
            ("scope", self.scope),
        ]);

        let response = http_trace::send(http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|e| Error::unavailable(format!("microsoft entra token request: {e}")))?;

        let status = response.status().as_u16();

        let body = response
            .text()
            .await
            .map_err(|e| Error::unavailable(format!("microsoft entra token response: {e}")))?;

        parse(status, &body)
    }
}

/// A token response: the token and how long it may be kept, or why there is none.
fn parse(status: u16, body: &str) -> Result<Token> {
    let response: TokenResponse = serde_json::from_str(body).unwrap_or_default();

    if (200..300).contains(&status) && !response.access_token.is_empty() {
        // Entra ID sends `expires_in` as a number; older endpoints sent a string.
        let expires_in = match &response.expires_in {
            Some(serde_json::Value::Number(seconds)) => seconds.as_u64(),
            Some(serde_json::Value::String(seconds)) => seconds.parse().ok(),
            _ => None,
        }
        .unwrap_or(3_600);

        return Ok(Token {
            value: response.access_token,
            lifetime: Duration::from_secs(expires_in.saturating_sub(EXPIRY_MARGIN_SECS).max(60)),
        });
    }

    let reason = match (response.error.as_str(), response.error_description.as_str()) {
        ("", "") => body.chars().take(300).collect::<String>(),
        (error, "") => error.to_string(),
        (_, description) => description.lines().next().unwrap_or_default().to_string(),
    };

    let message = format!("microsoft entra refused the client credentials ({status}): {reason}");

    // A wrong tenant, client or secret comes back as 400 or 401.
    match status {
        400 | 401 => Err(Error::unauthorized(message)),
        403 => Err(Error::forbidden(message)),
        429 => Err(Error::resource_exhausted(message)),
        _ => Err(Error::unavailable(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_kept_until_shortly_before_it_expires() {
        let token = parse(
            200,
            r#"{"token_type":"Bearer","expires_in":3599,"ext_expires_in":3599,"access_token":"eyJ0"}"#,
        )
        .unwrap();

        assert_eq!(token.value, "eyJ0");
        assert_eq!(token.lifetime, Duration::from_secs(3_299));
    }

    #[test]
    fn a_refusal_says_why() {
        let error = parse(
            401,
            r#"{"error":"invalid_client","error_description":"AADSTS7000215: Invalid client secret provided.\r\nTrace ID: 1"}"#,
        )
        .err()
        .unwrap();

        assert!(matches!(error, Error::Unauthorized(_)));
        assert!(error.to_string().contains("AADSTS7000215"));
        assert!(!error.to_string().contains("Trace ID"));
    }
}
