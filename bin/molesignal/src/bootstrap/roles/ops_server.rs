// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 运维监听：没有 HTTP API 的角色（intake、querier、compactor、alert）在 `[http]` 配置的
//! 地址上提供探活、就绪与指标，供编排器与监控使用。
//!
//! - `GET /api/v1/healthz`：进程内各子系统的综合健康，降级时返回 `503`。
//! - `GET /api/v1/readyz`：WAL 重放完成前返回 `503`。
//! - `GET /metrics`：Prometheus 文本格式指标。
//!
//! 监听面向内部网络，不做鉴权，也不提供业务 API。

use axum::{Router, routing::get};

use crate::{
    api::{AppState, http::routes},
    config::Settings,
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/healthz", get(routes::health::healthz))
        .route("/api/v1/readyz", get(routes::health::readyz))
        .merge(routes::metrics::scrape_routes())
        .with_state(state)
}

pub async fn serve(state: AppState, cfg: Settings) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("{}:{}", cfg.http.bind, cfg.http.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "ops http server listening");

    axum::serve(listener, build_router(state))
        .await
        .map_err(|error| anyhow::anyhow!("ops http serve: {error}"))
}
