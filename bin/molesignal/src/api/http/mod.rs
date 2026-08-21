// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Router,
    middleware::{from_fn, from_fn_with_state},
};
use http::HeaderName;
use tower_http::cors::CorsLayer;

use crate::api::AppState;

pub mod billing;
pub(crate) mod client_ip;
pub mod federation;
pub mod middleware;
pub mod pagination;
pub mod routes;
pub mod validate;
pub(crate) mod web_assets;

/// 装配整个 HTTP API 的 axum Router。
pub fn build_router(state: AppState) -> Router {
    build_router_with_client_ip(state, client_ip::ClientIpResolverHandle::peer())
}

pub(crate) fn build_router_with_client_ip(
    state: AppState,
    client_ip: client_ip::ClientIpResolverHandle,
) -> Router {
    let r = Router::new()
        .nest("/api/v1", routes::api_v1(state.clone()))
        // /metrics：在 auth layer 之外（白名单路径）单独挂顶层
        .merge(routes::metrics::scrape_routes())
        // /s/<token>：仅解析受限资源分享凭证，不接受任意 URL。
        .merge(routes::resource_shares::redirect_routes().with_state(state.clone()))
        // /api/v1/files/stream/<token>：顶层挂以绕开 auth；token 自带授权
        .merge(routes::files::stream_routes().with_state(state.clone()))
        // /api/v1/public/avatars/<user>/<file>：顶层公开（无 auth），<img> 直读头像
        .merge(routes::me::avatar_serve_routes().with_state(state.clone()))
        // /api/v1/public/status-pages/<slug>：客户状态页无需 MoleSignal 登录。
        .merge(routes::status_pages::public_routes().with_state(state.clone()))
        // Probe Artifact 上传：任务 lease token 自鉴权，不接受用户 Bearer。
        .merge(routes::synthetics::artifact_upload_routes().with_state(state.clone()));

    let r = r.merge(routes::oauth::well_known_routes().with_state(state.clone()));

    // /.well-known/acme-challenge/<token>：顶层公开（无 auth），仅
    let r = r.merge(routes::domains::challenge_routes().with_state(state.clone()));

    // 层序（外→内）：auth_layer 先跑注入 IamContext，org_blocking_layer 紧随其后按 org 拦停服。
    // `.layer()` 链中先加的更内层，故 org_blocking 放在 auth 之前一行。
    let api = r
        .layer(from_fn_with_state(
            state.clone(),
            middleware::org_blocking_layer,
        ))
        .layer(from_fn_with_state(state.clone(), middleware::auth_layer))
        .layer(from_fn(middleware::trace_context_layer))
        .layer(Extension(client_ip))
        .layer(CorsLayer::very_permissive().expose_headers([
            HeaderName::from_static("x-request-id"),
            HeaderName::from_static("x-trace-id"),
        ]))
        .with_state(state);

    // 静态 UI 位于 API middleware 之外：登录页和 Vite chunk 不应要求 Bearer token。
    web_assets::with_embedded_web(Router::new().merge(api))
}
