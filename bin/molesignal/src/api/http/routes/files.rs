// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Async file download（spec storage 修订）。
//!
//! - `POST /api/v1/files/download` → 创建 token，返 streaming URL（local）
//!   或 pre-signed URL（s3，留 follow-up；当前返 streaming URL 兜底）
//! - `GET /api/v1/files/stream/<token>` → 公开端点（白名单），streamed bytes
//!
//! S3 pre-signed URL 需要 AWS SDK；当前统一走 streaming token，简化客户端逻辑。
//!
//! 能下载的只有签发组织自己的 Catalog 对象：签发时每个 key 都要落在本组织前缀内且对象存在，
//! 兑付时再经组织限定的只读视图读取。令牌兑付不带会话，签发人失去组织成员资格或查询权限后，
//! 令牌一并失效。

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, State},
    http::header::CONTENT_TYPE,
    response::Response,
    routing::post,
};
use domain::iam::{IamContext, IamScope, permission};
use kernel::{Error, Result, time::TimestampMicros};
use object_store::{ObjectStoreExt, path::Path as ObjPath};
use postgres::repositories::file_download_tokens::{FileDownloadToken, generate_token};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::access::IamSubject,
    infra::storage::org_scoped_store::{owned_by_org, scoped_to_org},
};

/// 能签发和兑付下载令牌的权限，二者之一即可。
const DOWNLOAD_PERMISSIONS: [&str; 2] = ["streams.query", "sys.telemetry.read"];

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
#[serde(rename_all = "camelCase")]
pub struct DownloadReq {
    pub object_keys: Vec<String>,
    #[serde(default = "default_ttl")]
    pub expires_in_secs: i64,
}

fn default_ttl() -> i64 {
    3600
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResp {
    pub download_url: String,
    pub token: String,
    pub expires_at: i64,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn create_download(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<DownloadReq>,
) -> Result<Json<DownloadResp>> {
    if req.object_keys.is_empty() {
        return Err(Error::invalid("objectKeys cannot be empty"));
    }

    if req.object_keys.len() > 100 {
        return Err(Error::invalid("max 100 object_keys per request"));
    }

    let store = scoped_to_org(state.storage.object_store.clone(), &ctx.org_id);

    for key in &req.object_keys {
        let path = owned_path(&ctx.org_id, key)?;

        match store.head(&path).await {
            Ok(_) => {}
            Err(object_store::Error::NotFound { .. }) => {
                return Err(Error::not_found("object not found"));
            }
            Err(error) => return Err(Error::internal(format!("object head: {error}"))),
        }
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
        expires_at: expires.0,
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

    // 签发人此刻仍要能在这个组织里查询，令牌不比签发人活得久。
    let issuer = state
        .iam
        .access
        .capabilities(&IamSubject {
            user_id: row.user_id.clone(),
            organization_id: row.org_id.clone(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
        })
        .await
        .map_err(|_| Error::Unauthorized("token issuer no longer has access".into()))?;

    if !DOWNLOAD_PERMISSIONS
        .iter()
        .any(|permission| issuer.has(permission))
    {
        return Err(Error::Unauthorized(
            "token issuer no longer has access".into(),
        ));
    }

    // 当前：返回第一个 object 的内容（多文件归档留 follow-up）
    let key = row
        .object_keys
        .first()
        .ok_or_else(|| Error::internal("token has no object_keys"))?;

    let path = owned_path(&row.org_id, key)?;
    let store = scoped_to_org(state.storage.object_store.clone(), &row.org_id);

    let result = store.get(&path).await.map_err(|error| match error {
        object_store::Error::NotFound { .. } => Error::not_found("object not found"),
        error => Error::internal(format!("object get: {error}")),
    })?;

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

/// 解析 `key`，它必须落在 `org` 的 Catalog 前缀内。拒绝信息不带 key。
fn owned_path(org: &kernel::ids::Id, key: &str) -> Result<ObjPath> {
    if !owned_by_org(org, key) {
        return Err(Error::forbidden(
            "object is outside the organization's storage prefix",
        ));
    }

    ObjPath::parse(key).map_err(|_| Error::invalid("invalid object key"))
}

#[cfg(test)]
mod tests {
    use kernel::ids::Id;

    use super::*;

    #[test]
    fn only_the_organization_s_own_catalog_objects_can_be_downloaded() {
        let org = Id::from_string("org-a");

        assert!(owned_path(&org, "artifacts/v1/org-a/logs/ds/seg/a.parquet").is_ok());

        for key in [
            "artifacts/v1/org-b/logs/ds/seg/a.parquet",
            "manifests/v1/org-ab/logs/1.parquet",
            "search_jobs/v1/org-a/job-1.ndjson",
            "blobs/v1/org-a/replay",
            "artifacts/v1/org-a/../org-b/a.parquet",
        ] {
            assert!(
                matches!(owned_path(&org, key), Err(Error::Forbidden(_))),
                "{key}"
            );
        }
    }
}
