// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC JWKS 拉取 + 缓存 + ID Token 验签。
//!
//! - 启动后第一次需要某个 `jwks_uri` 时拉取并缓存；TTL 内重用，超时刷新。
//! - 验签：当前支持 RS256（OIDC 主流），用 [`jsonwebtoken::decode`] 校验签名 +
//!   audience + issuer + 过期。nonce claim 单独 string-equal 校验。
//!
//! 多 jwks_uri 共存：用一个内部 `HashMap<String, CachedSet>` 按 uri 分桶。

use std::{collections::HashMap, sync::RwLock};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
    /// RSA modulus（base64url）
    pub n: String,
    /// RSA exponent（base64url）
    pub e: String,
    #[serde(default, rename = "use")]
    pub usage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

struct CachedSet {
    set: JwkSet,
    expires_at_us: i64,
}

pub struct JwksCache {
    cache: RwLock<HashMap<String, CachedSet>>,
    ttl_us: i64,
    http: Client,
}

impl JwksCache {
    pub fn new(ttl_secs: u64) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| Error::internal(format!("jwks http client: {e}")))?;
        Ok(Self {
            cache: RwLock::new(HashMap::new()),
            ttl_us: (ttl_secs as i64).saturating_mul(1_000_000),
            http,
        })
    }

    /// 取一个 `jwks_uri` 对应的 JWK set；缓存命中直接返。miss/expired 时去拉。
    pub async fn get(&self, jwks_uri: &str) -> Result<JwkSet> {
        let now = TimestampMicros::now().0;
        {
            let g = self.cache.read().expect("jwks cache poisoned");
            if let Some(c) = g.get(jwks_uri)
                && c.expires_at_us > now
            {
                return Ok(c.set.clone());
            }
        }
        // refresh
        let resp = crate::shared::http_trace::send(
            &self.http,
            self.http.get(jwks_uri),
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::internal(format!("jwks fetch: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::internal(format!(
                "jwks fetch status {}",
                resp.status()
            )));
        }
        let set: JwkSet = resp
            .json()
            .await
            .map_err(|e| Error::internal(format!("jwks decode: {e}")))?;
        let mut w = self.cache.write().expect("jwks cache poisoned");
        w.insert(
            jwks_uri.to_string(),
            CachedSet {
                set: set.clone(),
                expires_at_us: now + self.ttl_us,
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

/// 校验 RS256 OIDC ID Token：签名 + iss + aud + exp + nonce。
///
/// 不强制 audience/issuer 配置：caller 传空字符串时跳过对应校验（少数自托管 IdP
/// 实现不规范）。但 nonce 必须匹配 — 这是 CSRF 关键。
pub fn verify_rs256_id_token(
    id_token: &str,
    jwks: &JwkSet,
    expected_audience: &str,
    expected_issuer: &str,
    expected_nonce: &str,
) -> Result<serde_json::Value> {
    let header = decode_header(id_token)
        .map_err(|e| Error::unauthorized(format!("id_token header: {e}")))?;
    let kid = header.kid.as_deref();
    let jwk = jwks
        .keys
        .iter()
        .find(|k| kid.is_some() && k.kid.as_deref() == kid)
        .or_else(|| jwks.keys.first())
        .ok_or_else(|| Error::unauthorized("no matching JWK in jwks_uri set".to_string()))?;
    if jwk.kty != "RSA" {
        return Err(Error::unauthorized(format!(
            "unsupported jwk kty: {} (only RSA today)",
            jwk.kty
        )));
    }
    let key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
        .map_err(|e| Error::unauthorized(format!("rsa components: {e}")))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = true;
    validation.validate_nbf = false;
    if !expected_audience.is_empty() {
        validation.set_audience(&[expected_audience]);
    } else {
        validation.validate_aud = false;
    }
    if !expected_issuer.is_empty() {
        validation.set_issuer(&[expected_issuer]);
    }
    let data = decode::<serde_json::Value>(id_token, &key, &validation)
        .map_err(|e| Error::unauthorized(format!("id_token verify: {e}")))?;
    let actual_nonce = data
        .claims
        .get("nonce")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if actual_nonce != expected_nonce {
        return Err(Error::unauthorized("id_token nonce mismatch".to_string()));
    }
    Ok(data.claims)
}
