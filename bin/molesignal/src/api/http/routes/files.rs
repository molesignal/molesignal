// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Async file download（spec storage 修订）。
//!
//! - `POST /api/v1/files/download` → 创建 token，返 streaming URL（local）
//!   或 pre-signed URL（s3，留 follow-up；当前返 streaming URL 兜底）
//! - `GET /api/v1/files/stream/<token>` → 公开端点（白名单），streamed bytes
//!
//! S3 pre-signed URL 需要 AWS SDK；当前统一走 streaming token，简化客户端逻辑。

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, State},
    http::header::CONTENT_TYPE,
    response::Response,
    routing::post,
};
use object_store::{ObjectStoreExt, path::Path as ObjPath};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::file_download_tokens::{FileDownloadToken, generate_token},
    shared::{Error, Result, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/files/download", post(create_download))
}

/// 公开端点（auth 白名单），顶层 merge。
pub fn stream_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/files/stream/{token}",
        axum::routing::get(stream_file),
    )
}

#[derive(Debug, Deserialize)]
pub struct DownloadReq {
    pub object_keys: Vec<String>,
    #[serde(default = "default_ttl")]
    pub expires_in_secs: i64,
}
fn default_ttl() -> i64 {
    3600
}

#[derive(Debug, Serialize)]
pub struct DownloadResp {
    pub download_url: String,
    pub token: String,
    pub expires_at_micros: i64,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn create_download(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<DownloadReq>,
) -> Result<Json<DownloadResp>> {
    if req.object_keys.is_empty() {
        return Err(Error::invalid("object_keys cannot be empty"));
    }
    if req.object_keys.len() > 100 {
        return Err(Error::invalid("max 100 object_keys per request"));
    }
    let ttl = req.expires_in_secs.clamp(60, 7 * 24 * 3600);
    let now = TimestampMicros::now();
    let expires = TimestampMicros(now.0 + ttl * 1_000_000);
    let token = generate_token();
    let row = FileDownloadToken {
        token: token.clone(),
        org_id: ctx.org_id.clone(),
        user_id: ctx.user_id.clone(),
        object_keys: req.object_keys,
        expires_at: expires,
        created_at: now,
    };
    state.storage.file_download_tokens.create(row).await?;
    Ok(Json(DownloadResp {
        download_url: format!("/api/v1/files/stream/{token}"),
        token,
        expires_at_micros: expires.0,
    }))
}

async fn stream_file(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> std::result::Result<Response, Error> {
    let row = state
        .storage
        .file_download_tokens
        .get(&token)
        .await?
        .ok_or_else(|| Error::not_found("token not found"))?;
    state
        .iam
        .service
        .ensure_organization_access(&row.org_id)
        .await?;
    if row.expires_at.0 <= TimestampMicros::now().0 {
        return Err(Error::Unauthorized("token expired".into()));
    }
    // 当前：返回第一个 object 的内容（多文件归档留 follow-up）
    let key = row
        .object_keys
        .first()
        .ok_or_else(|| Error::internal("token has no object_keys"))?;
    let path = ObjPath::parse(key).map_err(|e| Error::internal(format!("path parse: {e}")))?;
    let result = state
        .storage
        .object_store
        .get(&path)
        .await
        .map_err(|e| Error::internal(format!("object get: {e}")))?;
    let bytes = result
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("object read: {e}")))?;
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .map_err(|e| Error::internal(format!("response build: {e}")))
}
