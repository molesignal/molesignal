// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 管理端填表用的身份源读取：OIDC Discovery 和 SAML IdP 元数据。
//!
//! - `POST /sso/providers/discover`：`{ issuer }` → 端点与 issuer 原文。
//! - `POST /sso/providers/saml-metadata`：`{ url }` 或 `{ xml }` → 实体 ID、单点登录地址、
//!   签名证书。

use axum::{Extension, Json, Router, routing::post};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::sso::{
        oidc::discovery::{OidcDiscovery, discover},
        saml::metadata::{self, SamlIdpMetadata},
    },
    shared::{Error, Result},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/sso/providers/discover", post(discover_oidc))
        .route("/sso/providers/saml-metadata", post(read_saml_metadata))
}

#[derive(Debug, Deserialize)]
struct DiscoverRequest {
    issuer: String,
}

#[permission("org.sso.manage")]
async fn discover_oidc(
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<DiscoverRequest>,
) -> Result<Json<OidcDiscovery>> {
    Ok(Json(discover(&request.issuer).await?))
}

#[derive(Debug, Deserialize)]
struct MetadataRequest {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    xml: Option<String>,
}

#[permission("org.sso.manage")]
async fn read_saml_metadata(
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<MetadataRequest>,
) -> Result<Json<SamlIdpMetadata>> {
    let xml = match (request.xml, request.url) {
        (Some(xml), _) if !xml.trim().is_empty() => xml,
        (_, Some(url)) if !url.trim().is_empty() => metadata::fetch(&url).await?,
        _ => return Err(Error::invalid("provide the metadata URL or its XML")),
    };

    Ok(Json(metadata::parse(&xml)?))
}
