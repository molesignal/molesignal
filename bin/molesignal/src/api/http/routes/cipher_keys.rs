// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cipher keys CRUD。
//!
//! Owner-only。`raw_key` 从不出库（response 已 `#[serde(skip_serializing)]`）。
//! create 接收 base64 encoded 32B raw key（前端生成）；rotate 同。
//!
//! `POST /cipher_keys` create
//! `POST /cipher_keys/:name/rotate` rotate
//! `GET  /cipher_keys` list
//! `GET  /cipher_keys/:name` get_latest
//! `DELETE /cipher_keys/:name` delete

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cipher_keys", get(list).post(create))
        .route("/cipher_keys/{name}", get(get_latest).delete(delete))
        .route("/cipher_keys/{name}/rotate", post(rotate))
        // 字段级加密 DEK 轮换（服务端生成 key、写新版本、即时刷新解密映射）。
        .route("/field_encryption/rotate", post(rotate_field_default))
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    /// base64 standard，必须解码后正好 32 字节。
    pub key_material_b64: String,
}

#[derive(Debug, Deserialize)]
pub struct RotateReq {
    pub key_material_b64: String,
}

#[derive(Debug, Serialize)]
pub struct KeyResp {
    pub id: String,
    pub name: String,
    pub alg: String,
    pub version: i32,
    pub created_at_micros: i64,
    pub rotated_at_micros: Option<i64>,
}

fn decode_b64_32(s: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| Error::invalid(format!("key_material b64: {e}")))?;
    if bytes.len() != 32 {
        return Err(Error::invalid(format!(
            "key_material must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn to_resp(k: &crate::infra::cipher::CipherKey) -> KeyResp {
    KeyResp {
        id: k.id.0.clone(),
        name: k.name.clone(),
        alg: k.alg.clone(),
        version: k.version,
        created_at_micros: k.created_at.0,
        rotated_at_micros: k.rotated_at.map(|t| t.0),
    }
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<KeyResp>>> {
    let keys = state.storage.cipher_keys.list(&ctx.org_id).await?;
    Ok(Json(keys.iter().map(to_resp).collect()))
}

#[permission("org.settings.read")]
async fn get_latest(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(name): Path<String>,
) -> Result<Json<KeyResp>> {
    let k = state
        .storage
        .cipher_keys
        .get_latest(&ctx.org_id, &name)
        .await?;
    Ok(Json(to_resp(&k)))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<KeyResp>> {
    let raw = decode_b64_32(&req.key_material_b64)?;
    let k = state
        .storage
        .cipher_keys
        .create(&ctx.org_id, &req.name, &raw)
        .await?;
    // 任何 key 增删 / 轮换都影响该 org 的字段解密映射，失效缓存即时生效。
    state.storage.field_keys.invalidate(&ctx.org_id).await;
    Ok(Json(to_resp(&k)))
}

#[permission("org.settings.manage")]
async fn rotate(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(name): Path<String>,
    Json(req): Json<RotateReq>,
) -> Result<Json<KeyResp>> {
    let raw = decode_b64_32(&req.key_material_b64)?;
    let k = state
        .storage
        .cipher_keys
        .rotate(&ctx.org_id, &name, &raw)
        .await?;
    state.storage.field_keys.invalidate(&ctx.org_id).await;
    Ok(Json(to_resp(&k)))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state.storage.cipher_keys.delete(&ctx.org_id, &name).await?;
    state.storage.field_keys.invalidate(&ctx.org_id).await;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[derive(Debug, Serialize)]
pub struct FieldKeyResp {
    pub key_id: String,
    pub version: i32,
}

/// 轮换该 org 的字段加密 DEK（`__field_default__`）：服务端生成新 raw key、写新版本、
/// 即时失效解密缓存。新写入用新版本，历史密文仍可解。`raw_key` 永不出库。
#[permission("org.settings.manage")]
async fn rotate_field_default(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<FieldKeyResp>> {
    let k = state.storage.field_keys.rotate_default(&ctx.org_id).await?;
    Ok(Json(FieldKeyResp {
        key_id: k.key_id,
        version: k.version,
    }))
}
