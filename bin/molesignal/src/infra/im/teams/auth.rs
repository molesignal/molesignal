// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bot Framework 发来的请求是不是真的：`Authorization: Bearer <JWT>`，按 Bot Framework 的规则验。
//!
//! - 签名是 RS256，密钥来自 OpenID 元数据里的 `jwks_uri`，缓存 24 小时；遇到不认识的 `kid` 时
//!   重新取，5 分钟内最多一次。取不到新的时用缓存里的。
//! - `iss` 是 `https://api.botframework.com`（美国政府云是 `https://api.botframework.us`），`aud` 是
//!   Bot 的应用 ID，`exp` 与 `nbf` 允许 5 分钟误差。验不过是 401。
//! - 签名的密钥要为这条消息的 `channelId` 背书（`endorsements`），令牌里的 `serviceUrl` 要和
//!   消息里的一致，不然是 403。

use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

/// 密钥缓存多久。
const KEYS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// 为了不认识的 `kid` 重新取密钥，最多这么频繁。
const REFETCH_EVERY: Duration = Duration::from_secs(5 * 60);
/// `exp` 与 `nbf` 允许的误差（秒）。
const LEEWAY_SECS: u64 = 300;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// 认谁签的令牌：OpenID 元数据的地址与令牌的签发方。
pub(crate) struct Trust {
    pub(crate) metadata_url: String,
    pub(crate) issuer: String,
}

impl Trust {
    /// Azure 公有云的 Bot Framework。
    pub(crate) fn public() -> Self {
        Self {
            metadata_url: "https://login.botframework.com/v1/.well-known/openidconfiguration"
                .into(),
            issuer: "https://api.botframework.com".into(),
        }
    }

    /// 美国政府云（GCC High、DoD）的 Bot Framework。
    pub(crate) fn government() -> Self {
        Self {
            metadata_url: "https://login.botframework.azure.us/v1/.well-known/openidconfiguration"
                .into(),
            issuer: "https://api.botframework.us".into(),
        }
    }
}

/// 一把签名密钥，和它背书的渠道。
#[derive(Debug, Clone)]
struct SigningKey {
    kid: String,
    n: String,
    e: String,
    endorsements: Vec<String>,
}

struct Keys {
    keys: Vec<SigningKey>,
    fetched_at: Instant,
}

/// 元数据地址 → 取到的密钥。
static KEYS: LazyLock<Mutex<HashMap<String, Keys>>> = LazyLock::new(Default::default);

fn refused(reason: &str) -> Error {
    Error::unauthorized(format!("the Bot Framework token is not valid: {reason}"))
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value> {
    let response = http_trace::send(http, http.get(url), HttpTarget::ThirdParty)
        .await
        .map_err(|error| {
            Error::unavailable(format!("Bot Framework keys request failed: {error}"))
        })?;

    if !response.status().is_success() {
        return Err(Error::unavailable(format!(
            "Bot Framework keys request failed: HTTP {}",
            response.status().as_u16()
        )));
    }

    response
        .json()
        .await
        .map_err(|_| Error::unavailable("Bot Framework returned invalid keys"))
}

async fn fetch(trust: &Trust) -> Result<Vec<SigningKey>> {
    let http = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|error| Error::internal(format!("Bot Framework HTTP client: {error}")))?;

    let metadata = get_json(&http, &trust.metadata_url).await?;
    let jwks_uri = metadata["jwks_uri"]
        .as_str()
        .ok_or_else(|| Error::unavailable("the Bot Framework metadata has no jwks_uri"))?;
    let keys = get_json(&http, jwks_uri).await?;

    Ok(keys["keys"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|key| key["kty"] == "RSA")
        .filter_map(|key| {
            Some(SigningKey {
                kid: key["kid"].as_str()?.to_string(),
                n: key["n"].as_str()?.to_string(),
                e: key["e"].as_str()?.to_string(),
                endorsements: key["endorsements"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect())
}

/// `kid` 对应的密钥：缓存过期或不认识时重新取。
async fn key(trust: &Trust, kid: &str) -> Result<Option<SigningKey>> {
    let mut cache = KEYS.lock().await;
    let find = |keys: &Keys| keys.keys.iter().find(|key| key.kid == kid).cloned();

    let (fresh, known, refetch_allowed) = match cache.get(&trust.metadata_url) {
        Some(keys) => (
            keys.fetched_at.elapsed() < KEYS_TTL,
            find(keys),
            keys.fetched_at.elapsed() >= REFETCH_EVERY,
        ),
        None => (false, None, true),
    };

    if fresh && known.is_some() {
        return Ok(known);
    }

    if !fresh || refetch_allowed {
        match fetch(trust).await {
            Ok(keys) => {
                cache.insert(
                    trust.metadata_url.clone(),
                    Keys {
                        keys,
                        fetched_at: Instant::now(),
                    },
                );
            }
            Err(error) if cache.contains_key(&trust.metadata_url) => {
                tracing::warn!(%error, "Bot Framework keys could not be refreshed; using the cached ones");
            }
            Err(error) => return Err(error),
        }
    }

    Ok(cache.get(&trust.metadata_url).and_then(find))
}

fn without_slash(url: &str) -> &str {
    url.trim().trim_end_matches('/')
}

/// 验一个请求：`authorization` 是它的 `Authorization` 头，`activity` 是它的正文。
pub(crate) async fn authenticate(
    trust: &Trust,
    authorization: Option<&str>,
    app_id: &str,
    activity: &Value,
) -> Result<()> {
    let token = authorization
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| refused("the request has no bearer token"))?;

    let header = decode_header(token).map_err(|_| refused("it is not a JWT"))?;

    if header.alg != Algorithm::RS256 {
        return Err(refused("it is not signed with RS256"));
    }

    let kid = header
        .kid
        .ok_or_else(|| refused("it names no signing key"))?;
    let key = key(trust, &kid)
        .await?
        .ok_or_else(|| refused("its signing key is unknown"))?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[app_id.trim()]);
    validation.set_issuer(&[trust.issuer.as_str()]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.validate_nbf = true;
    validation.leeway = LEEWAY_SECS;

    let decoding = DecodingKey::from_rsa_components(&key.n, &key.e)
        .map_err(|_| refused("its signing key cannot be read"))?;
    let claims = decode::<Value>(token, &decoding, &validation)
        .map_err(|error| refused(&error.to_string()))?
        .claims;

    let channel = activity["channelId"].as_str().unwrap_or_default();

    if channel.is_empty() || !key.endorsements.iter().any(|endorsed| endorsed == channel) {
        return Err(Error::forbidden(
            "the Bot Framework signing key does not endorse this channel",
        ));
    }

    match (
        claims["serviceUrl"].as_str(),
        activity["serviceUrl"].as_str(),
    ) {
        (Some(claimed), Some(actual)) if without_slash(claimed) == without_slash(actual) => Ok(()),
        _ => Err(Error::forbidden(
            "the serviceUrl of the token and of the activity differ",
        )),
    }
}

#[cfg(test)]
mod tests;
