// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Router 角色：把 `/api/v1/ingest/*` 与 `/api/v1/query` 反代到
//! 由 [`ClusterRegistry`] 选出的下游节点；用 `governor` 按 `(org_id, route_class)`
//! 限流，超限返 `429 Too Many Requests` + `Retry-After`。
//!
//! 当前简化：
//! - 非流式 body（用 `axum::body::to_bytes` 收完转发）
//! - `org_id` 提取：HTTP header `X-Org-Id`（生产应由 auth middleware 注入）
//! - 选下游：`pick_ingester(org_id, "default")` / `pick_querier()`

use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
};
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    state::{InMemoryState, NotKeyed},
};
use parking_lot::Mutex;

use crate::{
    api::AppState,
    config::{RouterRateLimit, Settings},
    shared::ids::Id,
};

type LimiterKey = (String, RouteClass);
type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum RouteClass {
    Ingest,
    Query,
}

#[derive(Clone)]
struct RouterState {
    app: AppState,
    limiters: Arc<Mutex<HashMap<LimiterKey, Arc<Limiter>>>>,
    rate: RouterRateLimit,
    http_client: reqwest::Client,
}

impl RouterState {
    fn limiter(&self, key: LimiterKey) -> Option<Arc<Limiter>> {
        let qps = match key.1 {
            RouteClass::Ingest => self.rate.ingest_qps,
            RouteClass::Query => self.rate.query_qps,
        };
        if qps == 0 {
            return None;
        }
        let mut guard = self.limiters.lock();
        if let Some(l) = guard.get(&key) {
            return Some(l.clone());
        }
        let burst = (qps * self.rate.burst_multiplier).max(qps);
        let quota = Quota::per_second(NonZeroU32::new(qps).unwrap())
            .allow_burst(NonZeroU32::new(burst).unwrap());
        let l = Arc::new(RateLimiter::direct(quota));
        guard.insert(key.clone(), l.clone());
        Some(l)
    }
}

pub async fn run(state: AppState, cfg: Settings) -> anyhow::Result<()> {
    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", cfg.http.bind, cfg.http.port)).await?;
    tracing::info!(addr = %listener.local_addr()?, "router listening");
    let app = build_router(state, cfg.router.rate_limit.clone())?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// 公开 Router 构造给集成测试用。
pub fn build_router(state: AppState, rate: RouterRateLimit) -> anyhow::Result<Router> {
    let rs = RouterState {
        app: state,
        limiters: Arc::new(Mutex::new(HashMap::new())),
        rate,
        http_client: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
    };
    Ok(Router::new()
        .route("/api/v1/ingest/{*rest}", any(proxy_ingest))
        .route("/api/v1/query", any(proxy_query))
        .with_state(rs))
}

async fn proxy_ingest(State(rs): State<RouterState>, req: Request) -> Response {
    let org_id = extract_org(req.headers());
    if let Some(retry_after) = rate_check(&rs, (org_id.clone(), RouteClass::Ingest)) {
        return rate_limited_response(retry_after);
    }
    let peer = match rs
        .app
        .cluster
        .registry
        .pick_ingester(&Id::from_string(org_id), "default")
        .await
    {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "no ingester available").into_response(),
    };
    forward(&rs, &peer.advertise_addr, req).await
}

async fn proxy_query(State(rs): State<RouterState>, req: Request) -> Response {
    let org_id = extract_org(req.headers());
    if let Some(retry_after) = rate_check(&rs, (org_id.clone(), RouteClass::Query)) {
        return rate_limited_response(retry_after);
    }
    let peer = match rs.app.cluster.registry.pick_querier().await {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "no querier available").into_response(),
    };
    forward(&rs, &peer.advertise_addr, req).await
}

fn extract_org(headers: &HeaderMap) -> String {
    headers
        .get("x-org-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string()
}

fn rate_check(rs: &RouterState, key: LimiterKey) -> Option<u64> {
    let l = rs.limiter(key)?;
    match l.check() {
        Ok(_) => None,
        Err(not_until) => {
            let wait = not_until.wait_time_from(DefaultClock::default().now());
            Some(wait.as_secs().max(1))
        }
    }
}

fn rate_limited_response(retry_secs: u64) -> Response {
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    resp.headers_mut().insert(
        "Retry-After",
        HeaderValue::from_str(&retry_secs.to_string()).unwrap_or(HeaderValue::from_static("1")),
    );
    resp
}

async fn forward(rs: &RouterState, peer_addr: &str, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("read body: {e}")).into_response(),
    };
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or(parts.uri.path());
    let url = format!("http://{peer_addr}{path_and_query}");
    let method = match parts.method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        _ => reqwest::Method::POST,
    };
    let mut rb = rs.http_client.request(method, &url).body(bytes.to_vec());
    for (k, v) in parts.headers.iter() {
        let name = k.as_str().to_lowercase();
        if ["host", "connection", "content-length"].contains(&name.as_str()) {
            continue;
        }
        if let Ok(vs) = v.to_str() {
            rb = rb.header(k.as_str(), vs);
        }
    }
    match crate::shared::http_trace::send(
        &rs.http_client,
        rb,
        crate::shared::http_trace::HttpTarget::Internal,
    )
    .await
    {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let bytes = resp.bytes().await.unwrap_or_default();
            (status, Body::from(bytes)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("upstream {peer_addr} ({}): {e}", uri_str(&parts.uri)),
        )
            .into_response(),
    }
}

fn uri_str(u: &Uri) -> String {
    u.to_string()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;

    #[test]
    fn extract_org_from_header() {
        let mut h = HeaderMap::new();
        h.insert("x-org-id", HeaderValue::from_static("orga"));
        assert_eq!(extract_org(&h), "orga");
        let h2 = HeaderMap::new();
        assert_eq!(extract_org(&h2), "default");
    }

    #[test]
    fn rate_limited_response_has_retry_after() {
        let resp = rate_limited_response(7);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let ra = resp.headers().get("Retry-After").unwrap();
        assert_eq!(ra.to_str().unwrap(), "7");
    }

    // 不调用 rs_with_qps（zeroed AppState 不安全），rate_check 行为用 minimal limiter 单独测：
    #[test]
    fn governor_allows_then_blocks_above_qps() {
        let quota =
            Quota::per_second(NonZeroU32::new(2).unwrap()).allow_burst(NonZeroU32::new(2).unwrap());
        let l: Arc<Limiter> = Arc::new(RateLimiter::direct(quota));
        assert!(l.check().is_ok());
        assert!(l.check().is_ok());
        // 第三次 burst 用尽
        assert!(l.check().is_err());
    }
}
