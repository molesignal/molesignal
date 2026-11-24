// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The OIDC token of an authenticated push subscription.
//!
//! With authentication on, Pub/Sub signs a Google ID token for the subscription's service account
//! and sends it as `Authorization: Bearer`. It is checked against Google's keys: the signature, the
//! issuer, the expiry, `email` (the service account the connector names, verified) and, when the
//! connector names one, `aud`. See
//! <https://cloud.google.com/pubsub/docs/authenticate-push-subscriptions>.

use jsonwebtoken::{Validation, decode, decode_header};
use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::sso::oidc::jwks::JwksCache;

/// Where Google publishes the keys its ID tokens are signed with.
const GOOGLE_JWKS_URI: &str = "https://www.googleapis.com/oauth2/v3/certs";

/// The issuers a Google ID token may name.
const GOOGLE_ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// How far clocks may disagree, in seconds.
const LEEWAY_SECS: u64 = 60;

/// What a token must say to be accepted.
pub struct Expected<'a> {
    pub service_account: &'a str,
    /// Empty: any audience.
    pub audience: &'a str,
}

/// Checks the bearer token of a push request.
pub async fn verify(token: &str, expected: &Expected<'_>, keys: &JwksCache) -> Result<()> {
    let header = decode_header(token)
        .map_err(|e| Error::unauthorized(format!("pub/sub push token: {e}")))?;

    let mut set = keys.get(GOOGLE_JWKS_URI).await?;

    if header
        .kid
        .as_deref()
        .is_some_and(|kid| set.find(kid).is_none())
    {
        // Google rotates its keys; one not seen yet is fetched again.
        set = keys.refresh(GOOGLE_JWKS_URI).await?;
    }

    let jwk = header
        .kid
        .as_deref()
        .and_then(|kid| set.find(kid))
        .ok_or_else(|| Error::unauthorized("pub/sub push token is signed by an unknown key"))?;

    let key = jsonwebtoken::DecodingKey::from_jwk(jwk)
        .map_err(|e| Error::unauthorized(format!("pub/sub push token key: {e}")))?;

    let claims = decode::<Value>(token, &key, &validation(header.alg, expected))
        .map_err(|e| Error::unauthorized(format!("pub/sub push token: {e}")))?
        .claims;

    check_claims(&claims, expected)
}

fn validation(alg: jsonwebtoken::Algorithm, expected: &Expected<'_>) -> Validation {
    let mut validation = Validation::new(alg);
    validation.leeway = LEEWAY_SECS;
    validation.set_issuer(&GOOGLE_ISSUERS);

    if expected.audience.is_empty() {
        validation.validate_aud = false;
    } else {
        validation.set_audience(&[expected.audience]);
    }

    validation
}

/// The token is the named service account's, and Google verified its address.
fn check_claims(claims: &Value, expected: &Expected<'_>) -> Result<()> {
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let verified = claims.get("email_verified").and_then(Value::as_bool) == Some(true);

    if !verified || !email.eq_ignore_ascii_case(expected.service_account.trim()) {
        return Err(Error::unauthorized(format!(
            "pub/sub push token is for `{email}`, not the connector's service account"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ACCOUNT: &str = "push@acme.iam.gserviceaccount.com";

    fn expected() -> Expected<'static> {
        Expected {
            service_account: ACCOUNT,
            audience: "",
        }
    }

    #[test]
    fn the_token_must_be_the_named_service_accounts_and_verified() {
        assert!(
            check_claims(
                &json!({"email": ACCOUNT, "email_verified": true}),
                &expected()
            )
            .is_ok()
        );
        assert!(
            check_claims(
                &json!({"email": "other@acme.iam.gserviceaccount.com", "email_verified": true}),
                &expected()
            )
            .is_err()
        );
        assert!(
            check_claims(
                &json!({"email": ACCOUNT, "email_verified": false}),
                &expected()
            )
            .is_err()
        );
        assert!(check_claims(&json!({}), &expected()).is_err());
    }

    #[test]
    fn the_audience_is_checked_only_when_named() {
        let alg = jsonwebtoken::Algorithm::RS256;

        assert!(!validation(alg, &expected()).validate_aud);

        let named = Expected {
            service_account: ACCOUNT,
            audience: "https://ms.example.com/api/v1/_gcp_pubsub",
        };

        assert!(validation(alg, &named).validate_aud);
    }
}
