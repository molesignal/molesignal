// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC JWKS 拉取与缓存。
//!
//! - 某个 `jwks_uri` 第一次用到时拉取并缓存，TTL 内重用，过期再拉。
//! - 逐把解析密钥：RSA、EC（P-256 / P-384）、OKP（Ed25519）留下；对称密钥和解析不了的
//!   密钥跳过，不让一把陌生的密钥拖垮整份 JWKS。
//! - ID Token 的 `kid` 在缓存里找不到时可强制刷新（密钥轮换），同一地址
//!   [`REFRESH_INTERVAL_SECS`] 内只刷新一次，防止伪造 `kid` 反复打到身份源。

use std::{collections::HashMap, sync::Arc, time::Duration};

use jsonwebtoken::{
    AlgorithmFamily, DecodingKey,
    jwk::{Jwk, JwkSet},
};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
    time::TimestampMicros,
};

const REFRESH_INTERVAL_SECS: i64 = 30;

struct CachedSet {
    set: Arc<JwkSet>,
    fetched_at_us: i64,
}

pub struct JwksCache {
    cache: RwLock<HashMap<String, CachedSet>>,
    ttl_us: i64,
    http: Client,
}

impl JwksCache {
    pub fn new(ttl_secs: u64) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| Error::internal(format!("jwks http client: {e}")))?;

        Ok(Self {
            cache: RwLock::new(HashMap::new()),
            ttl_us: (ttl_secs as i64).saturating_mul(1_000_000),
            http,
        })
    }

    /// 取一个 `jwks_uri` 的密钥；缓存命中直接返回，缺失或过期时去拉。
    pub async fn get(&self, jwks_uri: &str) -> Result<Arc<JwkSet>> {
        let now = TimestampMicros::now().0;

        if let Some(cached) = self.cache.read().await.get(jwks_uri)
            && cached.fetched_at_us + self.ttl_us > now
        {
            return Ok(cached.set.clone());
        }

        self.fetch(jwks_uri).await
    }

    /// 密钥轮换后重新拉取；刚拉过就直接返回缓存。
    pub async fn refresh(&self, jwks_uri: &str) -> Result<Arc<JwkSet>> {
        let now = TimestampMicros::now().0;

        if let Some(cached) = self.cache.read().await.get(jwks_uri)
            && cached.fetched_at_us + REFRESH_INTERVAL_SECS * 1_000_000 > now
        {
            return Ok(cached.set.clone());
        }

        self.fetch(jwks_uri).await
    }

    async fn fetch(&self, jwks_uri: &str) -> Result<Arc<JwkSet>> {
        let response =
            http_trace::send(&self.http, self.http.get(jwks_uri), HttpTarget::ThirdParty)
                .await
                .map_err(|e| Error::internal(format!("jwks fetch: {e}")))?;

        if !response.status().is_success() {
            return Err(Error::internal(format!(
                "jwks fetch status {}",
                response.status()
            )));
        }

        let document: Value = response
            .json()
            .await
            .map_err(|e| Error::internal(format!("jwks decode: {e}")))?;

        let set = Arc::new(parse_key_set(&document));

        self.cache.write().await.insert(
            jwks_uri.to_owned(),
            CachedSet {
                set: set.clone(),
                fetched_at_us: TimestampMicros::now().0,
            },
        );

        Ok(set)
    }
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new(3600).expect("jwks http client builds with default timeout")
    }
}

/// 留下能验签的公钥。
pub fn parse_key_set(document: &Value) -> JwkSet {
    let keys = document
        .get("keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|key| serde_json::from_value::<Jwk>(key.clone()).ok())
        .filter(|jwk| {
            DecodingKey::from_jwk(jwk).is_ok_and(|key| key.family() != AlgorithmFamily::Hmac)
        })
        .collect();

    JwkSet { keys }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn unsupported_keys_are_skipped_instead_of_failing_the_set() {
        let document = json!({
            "keys": [
                { "kty": "RSA", "kid": "rsa-1", "use": "sig", "alg": "RS256",
                  "n": "sXchDaQebHnPiGvyDOAT4saGEUetSyo9MKLOoWFsueri23bOdgWp4Dy1WlUzewbgBHod5pcM9H95GQRV3JDXboIRROSBigeC5yjU1hGzHHyXss8UDprecbAYxknTcQkhslANGRUZmdTOQ5qTRsLAt6BTYuyvVRdhS8exSZEy_c4gs_7svlJJQ4H9_NxsiIoLwAEk7-Q3UXERGYw_75IDrGA84-lA_-Ct4eTlXHBIY2EaV7t7LjJaynVJCpkv4LKjTTAumiGUIuQhrNhZLuF_RJLqHpM2kgWFLU7-VTdL1VbC2tejvcI2BlMkEpk1BzBZI0KQB0GaDWFLN-aEAw3vRw",
                  "e": "AQAB" },
                { "kty": "EC", "kid": "ec-1", "crv": "P-384", "alg": "ES384",
                  "x": "FcmD3ZNKmhTRcJ9_2iECt2V8EJb0XzUIRZ8-YrGkhfnfbhCzBoNE_k6Y5O5zeDAt",
                  "y": "pqXMhMugE-BZDSXxuxtvIOb2qvQ5vC5ezq6xhUFYOcN73SAVzCNfbG48N_kzNSv8" },
                { "kty": "oct", "kid": "hmac", "k": "c2VjcmV0" },
                { "kty": "RSA", "kid": "enc", "use": "enc", "alg": "RSA-OAEP" },
                { "kty": "XYZ", "kid": "future" }
            ]
        });

        let kids = parse_key_set(&document)
            .keys
            .into_iter()
            .filter_map(|jwk| jwk.common.key_id)
            .collect::<Vec<_>>();

        assert_eq!(kids, ["rsa-1", "ec-1"]);
    }
}
