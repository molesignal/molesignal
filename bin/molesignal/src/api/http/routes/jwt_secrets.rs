// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! JWT signing secrets rotate / list（auth-hardening signing-secrets）。
//!
//! Platform settings managers only. `secret` raw bytes 永不出库 / 不打 log。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::{
    api::AppState, app::iam::IamContext, domain::iam::permission,
    infra::persistence::repositories::signing_secrets::rotate_jwt_secret, shared::Result,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/jwt/secrets", get(list))
        .route("/auth/jwt/rotate", axum::routing::post(rotate))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMeta {
    pub id: String,
    pub is_primary: bool,
    pub created_at: i64,
    pub retired_at: Option<i64>,
    // 注意：**不**返 secret raw bytes
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateResp {
    pub new_kid: String,
    pub retired_kid: Option<String>,
    pub active_count: usize,
}

#[permission("sys.signing_keys.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<SecretMeta>>> {
    let rows = state.iam.signing_secrets.list_metadata("jwt").await?;

    Ok(Json(
        rows.into_iter()
            .map(|s| SecretMeta {
                id: s.id.0,
                is_primary: s.is_primary,
                created_at: s.created_at.0,
                retired_at: s.retired_at.map(|v| v.0),
            })
            .collect(),
    ))
}

#[permission("sys.signing_keys.rotate")]
async fn rotate(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<RotateResp>> {
    let (new_kid, retired_kid) = rotate_jwt_secret(state.iam.signing_secrets.as_ref()).await?;
    let active_count = state.iam.service.reload_jwt_secrets().await?;

    Ok(Json(RotateResp {
        new_kid,
        retired_kid,
        active_count,
    }))
}
