// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Domain management CRUD（付费版独占）。
//!
//! `cfg=` + `license.has_feature("domain_management")`；OSS 完全不编译。
//! `/.well-known/acme-challenge/{token}` 不在此处挂载（要求顶层 + 无 auth），见 `mod.rs`。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    domain_management::{DOMAIN_FEATURE, hostname_valid},
    infra::persistence::repositories::domains::DomainRow,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/domains", get(list).post(create))
        .route("/domains/{id}", get(get_one).delete(delete))
        .route("/domains/{id}/renew", post(renew))
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(DOMAIN_FEATURE) {
        return Err(Error::forbidden(format!(
            "{DOMAIN_FEATURE} feature not licensed"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub hostname: String,
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub hostname: String,
    pub state: String,
    pub cert_not_after_micros: Option<i64>,
    pub last_error: Option<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(d: DomainRow) -> Resp {
    Resp {
        id: d.id.0,
        hostname: d.hostname,
        state: d.state,
        cert_not_after_micros: d.cert_not_after.map(|t| t.0),
        last_error: d.last_error,
        created_at_micros: d.created_at.0,
        updated_at_micros: d.updated_at.0,
    }
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    require_license(&state)?;
    Ok(Json(
        state
            .platform
            .domains
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[permission("org.settings.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    require_license(&state)?;
    Ok(Json(to_resp(
        state.platform.domains.get(&ctx.org_id, &Id(id)).await?,
    )))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    require_license(&state)?;
    let hostname = req.hostname.trim().to_lowercase();
    hostname_valid(&hostname).map_err(|e| Error::invalid(format!("invalid hostname: {e}")))?;
    let now = TimestampMicros::now();
    let d = DomainRow {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        hostname,
        state: "pending".to_string(),
        cert_pem: None,
        cert_not_after: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(to_resp(state.platform.domains.create(d).await?)))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    state.platform.domains.delete(&ctx.org_id, &Id(id)).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[permission("org.settings.manage")]
async fn renew(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    // 真实 ACME 续期由 background runner 处理；这里仅标 "provisioning" 触发下一轮拉取。
    state
        .platform
        .domains
        .update_state(&Id(id), "provisioning", None, None, None)
        .await?;
    Ok(Json(serde_json::json!({"renew_queued": true})))
}

/// 顶层路由：`/.well-known/acme-challenge/{token}`。
/// 不挂在 `/api/v1` 下、绕过 auth；HTTP-01 challenge 期间 LE 服务器直拉。
pub fn challenge_routes() -> Router<AppState> {
    Router::new().route("/.well-known/acme-challenge/{token}", get(serve_challenge))
}

async fn serve_challenge(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<String> {
    let ch = state
        .platform
        .domains
        .get_challenge(&token)
        .await?
        .ok_or_else(|| Error::not_found("acme challenge not found"))?;
    Ok(ch.key_authorization)
}
