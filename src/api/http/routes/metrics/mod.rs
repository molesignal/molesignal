// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metrics HTTP 路由。
//!
//! [`scrape_routes`] 提供顶层公开的 Prometheus `/metrics` scrape 端点；
//! [`api_routes`] 提供挂在 `/api/v1` 下、受 IAM 保护的 metric catalog。

use axum::{
    Router,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{api::AppState, shared::metrics::gather_text};

mod catalog;

pub fn api_routes() -> Router<AppState> {
    catalog::routes()
}

/// 用 [`gather_text`] 编码全局 Registry 为 Prometheus text format。
/// `/metrics` 始终装配、恒开（无独立开关；要屏蔽走反代）。
pub fn scrape_routes() -> Router<AppState> {
    Router::new().route("/metrics", get(metrics_handler))
}

async fn metrics_handler() -> Response {
    match gather_text() {
        Ok(body) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "text/plain; version=0.0.4")],
            body,
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("gather: {e}")).into_response(),
    }
}
