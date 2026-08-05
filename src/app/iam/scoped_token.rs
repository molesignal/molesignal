// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Short-lived, purpose-bound tokens for server-owned opaque state.
//!
//! Cursor payloads are visible to clients but cannot be modified without
//! invalidating the signature. Reusing the active JWT secret set also keeps
//! cursor verification working across the normal signing-key rotation grace
//! window without introducing another configured secret.

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{IamService, JWT_ISSUER};
use crate::shared::{Error, Result, ids::Id};

const MAX_SCOPED_TOKEN_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopedTokenClaims<T> {
    iss: String,
    aud: String,
    org_id: String,
    iat: usize,
    exp: usize,
    payload: T,
}

impl IamService {
    /// Sign server-owned state for one organization and one explicit purpose.
    pub(crate) fn issue_scoped_token<T>(
        &self,
        purpose: &str,
        org_id: &Id,
        payload: T,
        ttl_secs: u64,
    ) -> Result<String>
    where
        T: Clone + Serialize,
    {
        let secrets = self.jwt_secrets.read();
        let primary = secrets.first().expect("primary jwt secret");
        encode_scoped_token(primary, purpose, org_id, payload, ttl_secs)
    }

    /// Verify purpose, organization, expiry, and signature before returning
    /// opaque state supplied by a client.
    pub(crate) fn verify_scoped_token<T>(
        &self,
        purpose: &str,
        org_id: &Id,
        token: &str,
    ) -> Result<T>
    where
        T: Clone + DeserializeOwned,
    {
        let secrets = self.jwt_secrets.read();
        decode_scoped_token(secrets.as_slice(), purpose, org_id, token)
    }
}

fn encode_scoped_token<T>(
    secret: &[u8],
    purpose: &str,
    org_id: &Id,
    payload: T,
    ttl_secs: u64,
) -> Result<String>
where
    T: Clone + Serialize,
{
    let now = chrono::Utc::now().timestamp().max(0) as usize;
    let ttl = ttl_secs.clamp(1, MAX_SCOPED_TOKEN_TTL_SECS) as usize;
    let claims = ScopedTokenClaims {
        iss: JWT_ISSUER.to_owned(),
        aud: purpose.to_owned(),
        org_id: org_id.0.clone(),
        iat: now,
        exp: now.saturating_add(ttl),
        payload,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|error| Error::internal(format!("scoped token encode: {error}")))
}

fn decode_scoped_token<T>(secrets: &[Vec<u8>], purpose: &str, org_id: &Id, token: &str) -> Result<T>
where
    T: Clone + DeserializeOwned,
{
    let mut validation = Validation::default();
    validation.set_issuer(&[JWT_ISSUER]);
    validation.set_audience(&[purpose]);

    for secret in secrets {
        let Ok(data) =
            decode::<ScopedTokenClaims<T>>(token, &DecodingKey::from_secret(secret), &validation)
        else {
            continue;
        };
        if data.claims.org_id == org_id.0 {
            return Ok(data.claims.payload);
        }
    }

    Err(Error::invalid("invalid or expired cursor"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct Payload {
        value: i64,
    }

    #[test]
    fn scoped_token_round_trips_and_binds_purpose_and_org() {
        let secret = b"cursor-test-secret";
        let org = Id::from_string("org-a");
        let token = encode_scoped_token(secret, "trace-list", &org, Payload { value: 42 }, 60)
            .expect("encode cursor");

        let decoded =
            decode_scoped_token::<Payload>(&[secret.to_vec()], "trace-list", &org, &token)
                .expect("decode cursor");
        assert_eq!(decoded, Payload { value: 42 });
        assert!(
            decode_scoped_token::<Payload>(&[secret.to_vec()], "different-purpose", &org, &token,)
                .is_err()
        );
        assert!(
            decode_scoped_token::<Payload>(
                &[secret.to_vec()],
                "trace-list",
                &Id::from_string("org-b"),
                &token,
            )
            .is_err()
        );
    }

    #[test]
    fn scoped_token_accepts_a_rotated_grace_secret_and_rejects_tampering() {
        let old_secret = b"old-cursor-secret";
        let org = Id::from_string("org-a");
        let token = encode_scoped_token(old_secret, "trace-list", &org, Payload { value: 7 }, 60)
            .expect("encode cursor");

        assert!(
            decode_scoped_token::<Payload>(
                &[b"new-cursor-secret".to_vec(), old_secret.to_vec()],
                "trace-list",
                &org,
                &token,
            )
            .is_ok()
        );

        let mut tampered = token.into_bytes();
        let last = tampered.last_mut().expect("non-empty token");
        *last = if *last == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(tampered).expect("token remains utf-8");
        assert!(
            decode_scoped_token::<Payload>(&[old_secret.to_vec()], "trace-list", &org, &tampered,)
                .is_err()
        );
    }
}
