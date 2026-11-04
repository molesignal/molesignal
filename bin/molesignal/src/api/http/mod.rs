// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Router,
    middleware::{from_fn, from_fn_with_state},
};
use http::HeaderName;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

use crate::api::AppState;

pub(crate) mod client_ip;
pub mod federation;
pub(crate) mod interface_language;
pub mod middleware;
pub mod pagination;
pub mod routes;
pub(crate) mod session_cookie;
pub(crate) mod signin_limit;
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
    build_router_with_limits(state, client_ip, None)
}

/// `rate_limits` 非空时，写入与查询路由按组织限流（router 角色）。
pub(crate) fn build_router_with_limits(
    state: AppState,
    client_ip: client_ip::ClientIpResolverHandle,
    rate_limits: Option<std::sync::Arc<middleware::RouterRateLimits>>,
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
        // /api/v1/public/automations/<id>/trigger：自动化的 API 触发，用规则自己的令牌鉴权。
        .merge(routes::agent::automations::public_routes().with_state(state.clone()))
        // /api/v1/intake/alerts：外部告警的接入地址，用告警来源自己的 Token 鉴权。
        .merge(routes::alerting::intake_routes().with_state(state.clone()))
        // /api/v1/intake/heartbeats/<token>：心跳规则的打卡地址，地址里的 Token 自鉴权。
        .merge(routes::alerting::heartbeat_routes().with_state(state.clone()))
        // /api/v1/public/tickets/jira/<connector>：Jira 的 Webhook，验连接器 Webhook 密钥签的 X-Hub-Signature。
        .merge(routes::alerting::ticket_webhook_routes().with_state(state.clone()))
        // /api/v1/public/im/teams/<connector>/messages：Teams 的回调，验 Bot Framework 的令牌。
        .merge(routes::agent::im::public_routes().with_state(state.clone()))
        // /api/v1/public/notify/<connector>/{replies,keypress}：短信回复、电话按键，验服务商签名或回调 Token。
        .merge(routes::notify::public_routes().with_state(state.clone()))
        // /api/v1/public/calendar/<token>.ics：值班日历的订阅，地址里的 Token 自鉴权，按订阅人重新核对权限。
        .merge(routes::schedule_calendar::public_routes().with_state(state.clone()))
        // Probe Artifact 上传：任务 lease token 自鉴权，不接受用户 Bearer。
        .merge(routes::synthetics::artifact_upload_routes().with_state(state.clone()));

    let r = r.merge(routes::oauth::well_known_routes().with_state(state.clone()));

    // /.well-known/acme-challenge/<token>：顶层公开（无 auth），仅
    let r = r.merge(routes::domains::challenge_routes().with_state(state.clone()));

    // 读授权在认证层之内：请求里的查询按调用者能不能读 `_audit` 执行。
    let r = r.layer(from_fn(middleware::read_grants_layer));

    // 限流在认证层之内：此时请求已带有 IamContext，限流按已认证的组织计数。
    let r = match rate_limits {
        Some(limits) => r.layer(from_fn_with_state(limits, middleware::rate_limit_layer)),
        None => r,
    };

    let web_gzip = state.platform.http_gzip;

    // 认证层注入 IamContext，供路由执行组织隔离与权限校验。
    let api = r
        .layer(from_fn_with_state(
            state.clone(),
            middleware::organization_status_layer,
        ))
        .layer(from_fn_with_state(state.clone(), middleware::auth_layer))
        .layer(from_fn(middleware::trace_context_layer))
        .layer(Extension(client_ip))
        // 任何来源都可以跨域调用 API（浏览器里的 RUM SDK、带 API Token 的脚本），但不带凭据：
        // 浏览器的登录会话在 Cookie 里，别的网站不能借它读写接口。
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::any())
                .allow_methods(AllowMethods::mirror_request())
                .allow_headers(AllowHeaders::mirror_request())
                .expose_headers([
                    HeaderName::from_static("x-request-id"),
                    HeaderName::from_static("x-trace-id"),
                ]),
        )
        .with_state(state);

    // 静态 UI 位于 API middleware 之外：登录页和 Vite chunk 不需要登录。
    web_assets::with_embedded_web(Router::new().merge(api), web_gzip)
}
