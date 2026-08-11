// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Model pricing CRUD。
//!
//! `GET    /model_prices`               list 全表
//! `POST   /model_prices`                upsert
//! `DELETE /model_prices/{provider}/{model}` 删除
//!
//! 组织管理员可使用 `org.settings.read` 查看；平台设置管理员使用
//! `sys.settings.manage` 查看并维护全局价格表。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::model_prices::ModelPrice,
    shared::{Result, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/model_prices", get(list).post(upsert))
        .route(
            "/model_prices/{provider}/{model}",
            axum::routing::delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub struct UpsertReq {
    pub provider: String,
    pub model: String,
    pub prompt_usd_per_1k: f64,
    pub completion_usd_per_1k: f64,
}

#[permission(any("org.settings.read", "sys.settings.manage"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ModelPrice>>> {
    let rows = state.platform.model_prices.list().await?;
    Ok(Json(rows))
}

#[permission("sys.settings.manage")]
async fn upsert(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<UpsertReq>,
) -> Result<Json<ModelPrice>> {
    let p = ModelPrice {
        provider: req.provider,
        model: req.model,
        prompt_usd_per_1k: req.prompt_usd_per_1k,
        completion_usd_per_1k: req.completion_usd_per_1k,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.platform.model_prices.upsert(p).await?;
    Ok(Json(saved))
}

#[permission("sys.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((provider, model)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    state
        .platform
        .model_prices
        .delete(&provider, &model)
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
