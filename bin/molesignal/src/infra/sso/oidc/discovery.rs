// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC Discovery：读取 `{issuer}/.well-known/openid-configuration`，给管理端填端点。

use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OidcDiscovery {
    /// 身份源自报的 issuer，原样保留（Auth0 带末尾斜杠）。
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_uri: Option<String>,
    pub scopes_supported: Vec<String>,
}

pub async fn discover(issuer: &str) -> Result<OidcDiscovery> {
    let base = issuer.trim().trim_end_matches('/');

    let url = Url::parse(&format!("{base}/.well-known/openid-configuration"))
        .map_err(|e| Error::invalid(format!("issuer is not a URL: {e}")))?;

    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::invalid("issuer must be an http(s) URL"));
    }

    let http = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::internal(format!("discovery http client: {e}")))?;

    let response = http_trace::send(&http, http.get(url.clone()), HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::invalid(format!("could not reach {url}: {e}")))?;

    if !response.status().is_success() {
        return Err(Error::invalid(format!(
            "{url} returned {}",
            response.status()
        )));
    }

    let document: Value = response
        .json()
        .await
        .map_err(|e| Error::invalid(format!("{url} is not a discovery document: {e}")))?;

    parse_document(base, &document)
}

fn parse_document(requested_issuer: &str, document: &Value) -> Result<OidcDiscovery> {
    let text = |field: &str| {
        document
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    let required = |field: &str| {
        text(field)
            .ok_or_else(|| Error::invalid(format!("the discovery document has no `{field}`")))
    };

    let issuer = required("issuer")?;

    // OIDC Discovery 1.0 §4.3：文档里的 issuer 必须就是请求的那个。
    if issuer.trim_end_matches('/') != requested_issuer {
        return Err(Error::invalid(format!(
            "the discovery document names issuer `{issuer}`, not `{requested_issuer}`"
        )));
    }

    let scopes_supported = document
        .get("scopes_supported")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    Ok(OidcDiscovery {
        authorize_url: required("authorization_endpoint")?,
        token_url: required("token_endpoint")?,
        userinfo_url: text("userinfo_endpoint"),
        jwks_uri: text("jwks_uri"),
        scopes_supported,
        issuer,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_endpoints_and_keeps_the_issuer_as_published() {
        let document = json!({
            "issuer": "https://tenant.auth0.com/",
            "authorization_endpoint": "https://tenant.auth0.com/authorize",
            "token_endpoint": "https://tenant.auth0.com/oauth/token",
            "userinfo_endpoint": "https://tenant.auth0.com/userinfo",
            "jwks_uri": "https://tenant.auth0.com/.well-known/jwks.json",
            "scopes_supported": ["openid", "profile", "email"]
        });
        let discovered = parse_document("https://tenant.auth0.com", &document).expect("document");
        assert_eq!(discovered.issuer, "https://tenant.auth0.com/");
        assert_eq!(discovered.token_url, "https://tenant.auth0.com/oauth/token");
        assert_eq!(discovered.scopes_supported.len(), 3);
    }

    #[test]
    fn a_document_for_another_issuer_is_rejected() {
        let document = json!({
            "issuer": "https://evil.example",
            "authorization_endpoint": "https://evil.example/authorize",
            "token_endpoint": "https://evil.example/token"
        });
        assert!(parse_document("https://idp.example", &document).is_err());
    }
}
