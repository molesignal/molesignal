// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/web/investigation/blob` 存 / `GET /:id` 取 ——
//! 投资栈超 4 KB 时把 payload 放到这里，URL 仅持有 blob_id。
//!
//! 内容外置到对象存储(S3)：`investigations/{org}/{id}.json`，PG 行 `investigation_blobs`
//! 只存 `object_key` 指针；旧行 payload 在 PG（fetch 双读兜底）。compactor 每天清理
//! 7 天前的行并顺带删 S3 对象（roles/compactor.rs）。

use axum::{
    Extension, Router,
    extract::{Path, State},
    response::Json,
    routing::{get, post},
};
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/investigation/blob", post(create))
        .route("/investigation/blob/{id}", get(fetch))
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest(pub Value);

#[derive(Debug, Serialize)]
pub struct CreateResponse {
    pub blob_id: String,
}

/// payload 体积上限：内容现在落对象存储，不再受 PG 行宽约束，从 64 KiB 放宽到 1 MiB
/// （16×；保持在 axum 默认 2 MiB body limit 之下，无需额外 DefaultBodyLimit 层）。
/// 若以后要支持更大快照，给本路由加 `DefaultBodyLimit::max(..)` 即可。前端 > 4 KiB 才走 blob。
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, Error> {
    let bytes = serde_json::to_vec(&req.0)
        .map_err(|e| Error::invalid(format!("serialize payload: {e}")))?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::invalid(format!(
            "payload exceeds {MAX_PAYLOAD_BYTES} bytes"
        )));
    }

    let id = Id::new();
    let object_key = format!("investigations/{}/{}.json", ctx.org_id.0, id.0);
    let path =
        ObjPath::parse(&object_key).map_err(|e| Error::internal(format!("blob path: {e}")))?;
    state
        .storage
        .object_store
        .put(&path, PutPayload::from(bytes))
        .await
        .map_err(|e| Error::internal(format!("blob store put: {e}")))?;
    state
        .storage
        .investigation_blobs
        .create(&id, &ctx.org_id, &object_key)
        .await?;
    Ok(Json(CreateResponse { blob_id: id.0 }))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn fetch(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    let stored = state
        .storage
        .investigation_blobs
        .fetch(&ctx.org_id, &Id(id))
        .await?;
    if let Some(key) = stored.object_key {
        let path = ObjPath::parse(&key).map_err(|e| Error::internal(format!("blob path: {e}")))?;
        let result = state
            .storage
            .object_store
            .get(&path)
            .await
            .map_err(|e| Error::internal(format!("blob store get: {e}")))?;
        let bytes = result
            .bytes()
            .await
            .map_err(|e| Error::internal(format!("blob read: {e}")))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| Error::internal(format!("blob parse: {e}")))?;
        return Ok(Json(value));
    }
    // 旧行：payload 还在 PG。
    Ok(Json(stored.legacy_payload.unwrap_or(Value::Null)))
}
