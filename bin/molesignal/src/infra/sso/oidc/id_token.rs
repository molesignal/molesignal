// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ID Token 验签与声明校验。
//!
//! 签名算法按令牌头的 `alg` 选：RS* / PS* / ES256 / ES384 / EdDSA 用 JWKS 里对得上的
//! 公钥；HS* 用 Client Secret（OIDC Core 10.1）。没配 JWKS 时，ID Token 是从 Token
//! 端点经 TLS 直接取回的，按 OIDC Core 3.1.3.7 只校验声明。`nonce` 一律必须对上。

use jsonwebtoken::{
    Algorithm, AlgorithmFamily, DecodingKey, Header, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet, KeyAlgorithm, PublicKeyUse},
};
use serde_json::Value;

use super::jwks::JwksCache;
use crate::shared::{Error, Result, time::TimestampMicros};

/// 时钟误差容忍。
const LEEWAY_SECS: i64 = 60;

/// ID Token 必须满足的声明。`issuer` / `audience` 为空时不校验对应声明。
pub struct Expected<'a> {
    pub issuer: &'a str,
    pub audience: &'a str,
    pub nonce: &'a str,
    pub client_secret: &'a str,
}

/// 验签并校验声明，返回 claims。`jwks` 是密钥缓存和身份源的 `jwks_uri`。
pub async fn verify(
    token: &str,
    expected: &Expected<'_>,
    jwks: Option<(&JwksCache, &str)>,
) -> Result<Value> {
    let header =
        decode_header(token).map_err(|e| Error::unauthorized(format!("id_token header: {e}")))?;

    let Some((cache, jwks_uri)) = jwks.filter(|_| header.alg.family() != AlgorithmFamily::Hmac)
    else {
        return verify_offline(token, &header, expected);
    };

    let mut keys = cache.get(jwks_uri).await?;

    // `kid` 对不上多半是身份源刚轮换了密钥。
    if find_key(&keys, &header).is_none() {
        keys = cache.refresh(jwks_uri).await?;
    }

    verify_with_keys(token, &header, expected, &keys)
}

/// 不用 JWKS：HS* 用 Client Secret 验签，其余只校验声明。
fn verify_offline(token: &str, header: &Header, expected: &Expected<'_>) -> Result<Value> {
    if header.alg.family() != AlgorithmFamily::Hmac {
        return decode_over_tls(token, expected);
    }

    if expected.client_secret.is_empty() {
        return Err(Error::unauthorized(
            "id_token is signed with the client secret, but no client secret is configured",
        ));
    }

    let key = DecodingKey::from_secret(expected.client_secret.as_bytes());
    decode_claims(token, header.alg, &key, expected)
}

fn verify_with_keys(
    token: &str,
    header: &Header,
    expected: &Expected<'_>,
    keys: &JwkSet,
) -> Result<Value> {
    let jwk = find_key(keys, header).ok_or_else(|| {
        Error::unauthorized(format!(
            "no {:?} key{} in the identity provider's JWKS",
            header.alg,
            header
                .kid
                .as_deref()
                .map(|kid| format!(" with kid `{kid}`"))
                .unwrap_or_default()
        ))
    })?;

    let key = DecodingKey::from_jwk(jwk)
        .map_err(|e| Error::unauthorized(format!("id_token key: {e}")))?;

    decode_claims(token, header.alg, &key, expected)
}

fn decode_claims(
    token: &str,
    alg: Algorithm,
    key: &DecodingKey,
    expected: &Expected<'_>,
) -> Result<Value> {
    let mut validation = Validation::new(alg);
    validation.leeway = LEEWAY_SECS as u64;

    if expected.audience.is_empty() {
        validation.validate_aud = false;
    } else {
        validation.set_audience(&[expected.audience]);
    }

    if !expected.issuer.is_empty() {
        let issuer = expected.issuer.trim_end_matches('/');
        validation.set_issuer(&[issuer.to_owned(), format!("{issuer}/")]);
    }

    let claims = decode::<Value>(token, key, &validation)
        .map_err(|e| Error::unauthorized(format!("id_token verify: {e}")))?
        .claims;

    check_nonce(&claims, expected.nonce)?;
    Ok(claims)
}

/// 没有密钥可用时的校验：过期时间、签发者、受众和 `nonce`。
fn decode_over_tls(token: &str, expected: &Expected<'_>) -> Result<Value> {
    let claims = jsonwebtoken::dangerous::insecure_decode::<Value>(token)
        .map_err(|e| Error::unauthorized(format!("id_token decode: {e}")))?
        .claims;

    let expires_at = claims
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::unauthorized("id_token has no exp claim"))?;

    if expires_at + LEEWAY_SECS < TimestampMicros::now().0 / 1_000_000 {
        return Err(Error::unauthorized("id_token has expired"));
    }

    let issuer = claims
        .get("iss")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if !expected.issuer.is_empty()
        && issuer.trim_end_matches('/') != expected.issuer.trim_end_matches('/')
    {
        return Err(Error::unauthorized(format!(
            "id_token issuer `{issuer}` does not match the configured issuer"
        )));
    }

    let audience_matches = match claims.get("aud") {
        Some(Value::String(audience)) => audience == expected.audience,
        Some(Value::Array(audiences)) => audiences
            .iter()
            .any(|audience| audience.as_str() == Some(expected.audience)),
        _ => false,
    };

    if !expected.audience.is_empty() && !audience_matches {
        return Err(Error::unauthorized(
            "id_token audience does not include the client id",
        ));
    }

    check_nonce(&claims, expected.nonce)?;
    Ok(claims)
}

fn check_nonce(claims: &Value, expected_nonce: &str) -> Result<()> {
    match claims.get("nonce").and_then(Value::as_str) {
        Some(nonce) if nonce == expected_nonce => Ok(()),
        _ => Err(Error::unauthorized("id_token nonce mismatch")),
    }
}

/// 令牌头指定了 `kid` 就只认这把；没有 `kid` 时取第一把能验这种算法的。
fn find_key<'a>(keys: &'a JwkSet, header: &Header) -> Option<&'a Jwk> {
    let mut usable = keys.keys.iter().filter(|jwk| key_fits(jwk, header.alg));

    match header.kid.as_deref() {
        Some(kid) => usable.find(|jwk| jwk.common.key_id.as_deref() == Some(kid)),
        None => usable.next(),
    }
}

fn key_fits(jwk: &Jwk, alg: Algorithm) -> bool {
    if matches!(jwk.common.public_key_use, Some(PublicKeyUse::Encryption)) {
        return false;
    }

    if jwk
        .common
        .key_algorithm
        .is_some_and(|declared| declared_algorithm(declared) != Some(alg))
    {
        return false;
    }

    match (&jwk.algorithm, alg) {
        (
            AlgorithmParameters::RSA(_),
            Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512,
        ) => true,
        (AlgorithmParameters::EllipticCurve(params), Algorithm::ES256) => {
            params.curve == EllipticCurve::P256
        }
        (AlgorithmParameters::EllipticCurve(params), Algorithm::ES384) => {
            params.curve == EllipticCurve::P384
        }
        (AlgorithmParameters::OctetKeyPair(params), Algorithm::EdDSA) => {
            params.curve == EllipticCurve::Ed25519
        }
        _ => false,
    }
}

/// JWK 里声明的 `alg` 与 JWS 算法同名；加密算法（如 `RSA-OAEP`）没有对应的签名算法。
fn declared_algorithm(declared: KeyAlgorithm) -> Option<Algorithm> {
    serde_json::to_value(declared).ok()?.as_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use jsonwebtoken::{EncodingKey, encode};
    use serde_json::json;

    use super::*;

    /// 只用于测试的一次性 P-384 私钥（PKCS#8 DER）及其公钥坐标。
    const EC384_PKCS8: &str = "MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDCABpOPIy/YrA9rXkrcFj4NiDt4KQ5JNnI1rSBbIplx592z3dKwkUurj5idVsmK7s+hZANiAARjdIs2RzNpChXm6FPivFSxUjUP64EMYDG0L/Es/sVLBhdj2YHH6cW8gh//DEPvWzqQfkdcfc6qNEeH6Im3upcLCWEHrVJ6dZbYUdBKMSraRmmhxARLxS6vMIQDak8TmWw=";
    const EC384_X: &str = "Y3SLNkczaQoV5uhT4rxUsVI1D-uBDGAxtC_xLP7FSwYXY9mBx-nFvIIf_wxD71s6";
    const EC384_Y: &str = "kH5HXH3OqjRHh-iJt7qXCwlhB61SenWW2FHQSjEq2kZpocQES8UurzCEA2pPE5ls";

    fn expected<'a>(nonce: &'a str) -> Expected<'a> {
        Expected {
            issuer: "https://idp.example/oidc",
            audience: "molesignal",
            nonce,
            client_secret: "client-secret",
        }
    }

    fn claims(nonce: &str) -> Value {
        json!({
            "iss": "https://idp.example/oidc",
            "aud": "molesignal",
            "sub": "u-1",
            "email": "alice@example.com",
            "nonce": nonce,
            "exp": TimestampMicros::now().0 / 1_000_000 + 300,
        })
    }

    fn ec_keys(kid: &str) -> JwkSet {
        super::super::jwks::parse_key_set(&json!({
            "keys": [
                { "kty": "RSA", "kid": "rsa-1", "alg": "RS256", "n": "sXch", "e": "AQAB" },
                { "kty": "EC", "kid": kid, "crv": "P-384", "alg": "ES384", "x": EC384_X, "y": EC384_Y }
            ]
        }))
    }

    fn es384_token(kid: &str, claims: &Value) -> String {
        let der = STANDARD.decode(EC384_PKCS8).expect("test key");
        let mut header = Header::new(Algorithm::ES384);
        header.kid = Some(kid.into());
        encode(&header, claims, &EncodingKey::from_ec_der(&der)).expect("sign")
    }

    #[test]
    fn es384_tokens_verify_against_the_matching_ec_key() {
        let token = es384_token("ec-1", &claims("n-1"));
        let header = decode_header(&token).expect("header");
        let verified = verify_with_keys(&token, &header, &expected("n-1"), &ec_keys("ec-1"))
            .expect("verified");
        assert_eq!(verified["email"], "alice@example.com");
    }

    #[test]
    fn an_unknown_kid_or_a_wrong_nonce_is_rejected() {
        let token = es384_token("ec-2", &claims("n-1"));
        let header = decode_header(&token).expect("header");
        assert!(verify_with_keys(&token, &header, &expected("n-1"), &ec_keys("ec-1")).is_err());

        let token = es384_token("ec-1", &claims("n-1"));
        let header = decode_header(&token).expect("header");
        let error = verify_with_keys(&token, &header, &expected("n-2"), &ec_keys("ec-1"))
            .expect_err("nonce");
        assert!(error.to_string().contains("nonce"));
    }

    #[test]
    fn hmac_tokens_verify_with_the_client_secret_only() {
        let header = Header::new(Algorithm::HS256);
        let signed = |secret: &str| {
            encode(
                &header,
                &claims("n-1"),
                &EncodingKey::from_secret(secret.as_bytes()),
            )
            .expect("sign")
        };

        let token = signed("client-secret");
        assert!(verify_offline(&token, &header, &expected("n-1")).is_ok());

        let forged = signed("guess");
        assert!(verify_offline(&forged, &header, &expected("n-1")).is_err());
    }

    #[test]
    fn tokens_without_keys_still_check_issuer_audience_and_expiry() {
        let unsigned = |claims: &Value| {
            let encode_part = |value: &Value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string())
            };
            format!(
                "{}.{}.sig",
                encode_part(&json!({ "alg": "RS256", "typ": "JWT" })),
                encode_part(claims)
            )
        };

        let header = Header::new(Algorithm::RS256);
        let good = unsigned(&claims("n-1"));
        assert!(verify_offline(&good, &header, &expected("n-1")).is_ok());

        let mut other_audience = claims("n-1");
        other_audience["aud"] = json!(["someone-else"]);
        assert!(verify_offline(&unsigned(&other_audience), &header, &expected("n-1")).is_err());

        let mut expired = claims("n-1");
        expired["exp"] = json!(1_000);
        assert!(verify_offline(&unsigned(&expired), &header, &expected("n-1")).is_err());
    }

    #[test]
    fn keys_must_fit_the_algorithm() {
        let keys = ec_keys("ec-1");
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some("ec-1".into());
        assert!(
            find_key(&keys, &header).is_none(),
            "a P-384 key cannot verify ES256"
        );

        header.alg = Algorithm::RS256;
        header.kid = None;
        assert_eq!(
            find_key(&keys, &header).and_then(|jwk| jwk.common.key_id.as_deref()),
            Some("rsa-1")
        );
    }
}
