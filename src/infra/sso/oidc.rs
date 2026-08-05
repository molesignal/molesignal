// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC Authorization Code flow。
//!
//! 当前设计权衡：
//! - 不引 `openidconnect` 全家桶（重 + tls feature 选择繁琐）。
//! - 手写 `/authorize` URL 构造 + `/token` exchange + ID Token 解（仅 decode header，
//!   不强制 JWKS 校验——生产可加 `decode_header` + 远端 JWKS 缓存）。
//! - state / nonce 由调用方（HTTP handler）保管在签名 cookie 或 redis；本模块不持有状态。

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    domain::iam::SsoFieldMapping,
    shared::{Error, Result},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OidcConfig {
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    /// `jwks_uri`：JWK Set 端点；用于 ID Token RS256 验签。常规放 IdP 的
    /// `/.well-known/openid-configuration` 里的 `jwks_uri` 字段。空字符串表示
    /// 跳过验签（不推荐，仅 dev/test 自托管场景）。
    #[serde(default)]
    pub jwks_uri: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub field_mapping: SsoFieldMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcTokens {
    pub access_token: String,
    pub id_token: Option<String>,
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUser {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
    pub groups: Vec<String>,
}

pub struct OidcLoginFlow {
    cfg: OidcConfig,
    http: Client,
}

impl OidcLoginFlow {
    pub fn new(cfg: OidcConfig) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| Error::internal(format!("oidc http client: {e}")))?;
        Ok(Self { cfg, http })
    }

    /// 构造 `GET /authorize?...` 302 目标。`state` 由 handler 生成（用于 CSRF + 回调匹配）。
    pub fn build_authz_url(&self, state: &str, nonce: &str) -> Result<Url> {
        let mut url = Url::parse(&self.cfg.authorize_url)
            .map_err(|e| Error::invalid(format!("authorize_url: {e}")))?;
        let scopes = if self.cfg.scopes.is_empty() {
            "openid profile email".to_string()
        } else {
            self.cfg.scopes.join(" ")
        };
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", &self.cfg.redirect_uri)
            .append_pair("scope", &scopes)
            .append_pair("state", state)
            .append_pair("nonce", nonce);
        Ok(url)
    }

    /// POST 到 `/token` 交换 access_token + id_token。
    pub async fn exchange_code(&self, code: &str) -> Result<OidcTokens> {
        let request = self.http.post(&self.cfg.token_url).form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.cfg.redirect_uri),
            ("client_id", &self.cfg.client_id),
            ("client_secret", &self.cfg.client_secret),
        ]);
        let resp = crate::shared::http_trace::send(
            &self.http,
            request,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::internal(format!("oidc token http: {e}")))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Unauthorized(format!("idp /token {s}: {body}")));
        }
        let tokens: OidcTokens = resp
            .json()
            .await
            .map_err(|e| Error::internal(format!("oidc token decode: {e}")))?;
        Ok(tokens)
    }

    /// 直接调 `userinfo` 端点（如果配置了）。也可以直接 decode id_token。
    pub async fn fetch_userinfo(&self, access_token: &str) -> Result<OidcUser> {
        let url = self.cfg.userinfo_url.as_ref().ok_or_else(|| {
            Error::invalid("userinfo_url not configured; use id_token decode instead")
        })?;
        let request = self.http.get(url).bearer_auth(access_token);
        let resp = crate::shared::http_trace::send(
            &self.http,
            request,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::internal(format!("oidc userinfo http: {e}")))?;
        if !resp.status().is_success() {
            let s = resp.status();
            return Err(Error::Unauthorized(format!("idp /userinfo {s}")));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::internal(format!("oidc userinfo decode: {e}")))?;
        parse_userinfo(&v, &self.cfg.field_mapping)
    }

    /// 走 JWKS 验签 + nonce 校验后解 `id_token`，返身份信息。
    ///
    /// `expected_nonce` 来自 login 阶段存进 [`crate::infra::sso::SsoStateStore`] 的 nonce；
    /// callback handler 在校验 state 后取出对应 entry 传入。jwks_uri 缺失时回落到
    /// [`Self::decode_id_token`]（无验签，仅 dev 路径）。
    pub async fn verify_id_token(
        &self,
        id_token: &str,
        jwks_cache: &crate::infra::sso::JwksCache,
        expected_nonce: &str,
    ) -> Result<OidcUser> {
        if self.cfg.jwks_uri.trim().is_empty() {
            // No JWKS configured → decode-only fallback. We still enforce nonce
            // equality so CSRF protection survives, but signature is not checked.
            let user = self.decode_id_token(id_token)?;
            let parts: Vec<&str> = id_token.split('.').collect();
            if parts.len() == 3 {
                let payload = base64_url::decode(parts[1])
                    .map_err(|e| Error::Unauthorized(format!("id_token b64: {e}")))?;
                let v: serde_json::Value = serde_json::from_slice(&payload)
                    .map_err(|e| Error::Unauthorized(format!("id_token json: {e}")))?;
                let actual = v.get("nonce").and_then(|x| x.as_str()).unwrap_or_default();
                if actual != expected_nonce {
                    return Err(Error::Unauthorized("id_token nonce mismatch".into()));
                }
            }
            return Ok(user);
        }
        let jwks = jwks_cache.get(&self.cfg.jwks_uri).await?;
        let claims = crate::infra::sso::verify_rs256_id_token(
            id_token,
            &jwks,
            &self.cfg.client_id,
            &self.cfg.issuer,
            expected_nonce,
        )?;
        parse_userinfo(&claims, &self.cfg.field_mapping)
    }

    /// 直接解 `id_token`（不校验签名）。保留给 SAML callback / 非 RS256 IdP；
    /// 新代码请用 [`Self::verify_id_token`]。
    pub fn decode_id_token(&self, id_token: &str) -> Result<OidcUser> {
        let parts: Vec<&str> = id_token.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::Unauthorized("id_token must be a JWT".into()));
        }
        let payload = base64_url::decode(parts[1])
            .map_err(|e| Error::Unauthorized(format!("id_token b64: {e}")))?;
        let v: serde_json::Value = serde_json::from_slice(&payload)
            .map_err(|e| Error::Unauthorized(format!("id_token json: {e}")))?;
        parse_userinfo(&v, &self.cfg.field_mapping)
    }
}

fn parse_userinfo(v: &serde_json::Value, mapping: &SsoFieldMapping) -> Result<OidcUser> {
    let subject = claim_string(v, &mapping.subject).ok_or_else(|| {
        Error::unauthorized(format!(
            "OIDC identity is missing mapped subject claim `{}`",
            mapping.subject
        ))
    })?;
    let email = claim_string(v, &mapping.email).ok_or_else(|| {
        Error::unauthorized(format!(
            "OIDC identity is missing mapped email claim `{}`",
            mapping.email
        ))
    })?;
    let name = claim_string(v, &mapping.display_name);
    let mut groups = claim_strings(v, &mapping.groups);
    groups.sort();
    groups.dedup();
    Ok(OidcUser {
        subject,
        email,
        name,
        groups,
    })
}

fn claim_at_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    value.get(path).or_else(|| {
        path.split('.')
            .try_fold(value, |current, segment| current.get(segment))
    })
}

fn claim_string(value: &serde_json::Value, path: &str) -> Option<String> {
    claim_at_path(value, path)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn claim_strings(value: &serde_json::Value, path: &str) -> Vec<String> {
    let Some(value) = claim_at_path(value, path) else {
        return Vec::new();
    };
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        serde_json::Value::String(value) if !value.trim().is_empty() => {
            vec![value.trim().to_owned()]
        }
        _ => Vec::new(),
    }
}

mod base64_url {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    pub fn decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
        // 补齐 padding（OIDC JWT 标准是 NO_PAD，但有些 IdP 会带）
        let mut s = s.to_string();
        let pad = (4 - s.len() % 4) % 4;
        s.extend(std::iter::repeat_n('=', pad));
        // 先试 NO_PAD（标准）
        URL_SAFE_NO_PAD
            .decode(s.trim_end_matches('='))
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> OidcConfig {
        OidcConfig {
            issuer: "https://idp.example".into(),
            authorize_url: "https://idp.example/auth".into(),
            token_url: "https://idp.example/token".into(),
            userinfo_url: Some("https://idp.example/userinfo".into()),
            jwks_uri: String::new(),
            client_id: "obsv".into(),
            client_secret: "shh".into(),
            redirect_uri: "https://obs.local/api/v1/auth/sso/callback".into(),
            scopes: vec!["openid".into(), "email".into()],
            field_mapping: SsoFieldMapping::oidc(),
        }
    }

    #[test]
    fn authorize_url_includes_required_params() {
        let f = OidcLoginFlow::new(cfg()).unwrap();
        let u = f.build_authz_url("st1", "n1").unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(q.get("client_id").map(String::as_str), Some("obsv"));
        assert_eq!(q.get("state").map(String::as_str), Some("st1"));
        assert_eq!(q.get("nonce").map(String::as_str), Some("n1"));
        assert_eq!(q.get("scope").map(String::as_str), Some("openid email"));
    }

    #[test]
    fn decode_id_token_extracts_email_and_groups() {
        use base64::Engine as _;
        let payload = serde_json::json!({
            "sub": "u-1",
            "email": "alice@example.com",
            "name": "Alice",
            "groups": ["g-admin", "g-eng"]
        });
        let payload_b64 =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let jwt = format!("eyJ.{payload_b64}.sig");
        let f = OidcLoginFlow::new(cfg()).unwrap();
        let u = f.decode_id_token(&jwt).unwrap();
        assert_eq!(u.email, "alice@example.com");
        assert_eq!(u.groups.len(), 2);
    }

    #[test]
    fn custom_mapping_supports_nested_claim_paths() {
        let payload = serde_json::json!({
            "uid": "u-1",
            "mail": "alice@example.com",
            "profile": { "display": "Alice" },
            "realm_access": { "roles": ["operator"] }
        });
        let mapping = SsoFieldMapping {
            subject: "uid".into(),
            email: "mail".into(),
            display_name: "profile.display".into(),
            groups: "realm_access.roles".into(),
        };
        let user = parse_userinfo(&payload, &mapping).expect("mapped OIDC identity");
        assert_eq!(user.name.as_deref(), Some("Alice"));
        assert_eq!(user.groups, ["operator"]);
    }
}
