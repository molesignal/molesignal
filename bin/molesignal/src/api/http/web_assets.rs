// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 编译进 `molesignal` 二进制的 Web UI 静态资源。
//!
//! 精确资源请求直接返回 Vite 产物；浏览器导航到 React Router 路由时回退到
//! `index.html`。后端命名空间和带扩展名的缺失资源永不回退，避免 API 404 或损坏的
//! chunk 请求被伪装成成功的 HTML 响应。

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{
        HeaderValue, Method, StatusCode,
        header::{ACCEPT, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use bytes::Bytes;

const HTML_CACHE_CONTROL: &str = "no-store, max-age=0";
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const ROOT_ASSET_CACHE_CONTROL: &str = "public, max-age=3600";
const STATIC_EXTENSIONS: &[&str] = &[
    "css", "ico", "jpeg", "jpg", "js", "json", "map", "mjs", "png", "svg", "ttf", "txt", "wasm",
    "webp", "woff", "woff2", "xml",
];
const BACKEND_PREFIXES: &[&str] = &[
    "/api",
    "/metrics",
    "/.well-known",
    "/docs/inbound-mcp",
    "/s",
    "/healthz",
    "/readyz",
];

struct EmbeddedWebAsset {
    path: &'static str,
    body: &'static [u8],
    content_type: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_web_assets.rs"));

/// 给已经装配好 API 路由和中间件的顶层 Router 增加未鉴权的 Web UI fallback。
///
/// 必须在 API middleware 装配完成后调用，登录页和静态 chunk 才不会被 Bearer 鉴权拦截。
pub(crate) fn with_embedded_web(router: Router) -> Router {
    router.fallback(serve)
}

async fn serve(request: Request) -> Response {
    let method = request.method();
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = request.uri().path();
    let asset_path = path.strip_prefix('/').unwrap_or(path);
    if let Some(asset) = find_asset(if asset_path.is_empty() {
        "index.html"
    } else {
        asset_path
    }) {
        return asset_response(asset, method == Method::HEAD);
    }

    if is_backend_path(path) || looks_like_missing_asset(path) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let accepts_html = request
        .headers()
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
    if path == "/" || accepts_html || request.headers().get(ACCEPT).is_none() {
        return asset_response(index_asset(), method == Method::HEAD);
    }

    StatusCode::NOT_FOUND.into_response()
}

fn find_asset(path: &str) -> Option<&'static EmbeddedWebAsset> {
    EMBEDDED_WEB_ASSETS
        .binary_search_by(|asset| asset.path.cmp(path))
        .ok()
        .map(|index| &EMBEDDED_WEB_ASSETS[index])
}

fn index_asset() -> &'static EmbeddedWebAsset {
    find_asset("index.html").expect("build.rs guarantees an embedded index.html")
}

fn asset_response(asset: &'static EmbeddedWebAsset, head_only: bool) -> Response {
    let cache_control = if asset.path == "index.html" {
        HTML_CACHE_CONTROL
    } else if asset.path.starts_with("assets/") {
        IMMUTABLE_CACHE_CONTROL
    } else {
        ROOT_ASSET_CACHE_CONTROL
    };
    let body = if head_only {
        Body::empty()
    } else {
        Body::from(Bytes::from_static(asset.body))
    };
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(asset.content_type));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&asset.body.len().to_string())
            .expect("usize always formats as a valid Content-Length"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn is_backend_path(path: &str) -> bool {
    BACKEND_PREFIXES.iter().any(|prefix| {
        path == *prefix
            || path
                .strip_prefix(*prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn looks_like_missing_asset(path: &str) -> bool {
    if path.starts_with("/assets/") {
        return true;
    }
    let segment = path.rsplit_once('/').map_or(path, |(_, segment)| segment);
    let Some((_, extension)) = segment.rsplit_once('.') else {
        return false;
    };
    STATIC_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Request, header},
    };
    use tower::ServiceExt as _;

    use super::*;

    async fn request(method: Method, path: &str, accept: Option<&str>) -> Response {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(accept) = accept {
            builder = builder.header(header::ACCEPT, accept);
        }
        Router::new()
            .fallback(serve)
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn root_serves_embedded_index_without_cache() {
        let response = request(Method::GET, "/", None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(response.headers()[CACHE_CONTROL], HTML_CACHE_CONTROL);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.starts_with(b"<!doctype html>"));
    }

    #[tokio::test]
    async fn browser_navigation_falls_back_to_index() {
        for path in ["/investigate/logs", "/status/acme.example"] {
            let response = request(Method::GET, path, Some("text/html")).await;
            assert_eq!(response.status(), StatusCode::OK, "path: {path}");
            assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        }
    }

    #[tokio::test]
    async fn embedded_chunks_are_immutable_and_keep_their_content_type() {
        assert!(
            EMBEDDED_WEB_ASSETS
                .iter()
                .all(|asset| !asset.path.ends_with(".map")),
            "source maps must not inflate the executable"
        );
        let asset = EMBEDDED_WEB_ASSETS
            .iter()
            .find(|asset| asset.path.starts_with("assets/") && asset.path.ends_with(".js"))
            .expect("Vite build must contain a JavaScript chunk");
        let response = request(Method::GET, &format!("/{}", asset.path), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(response.headers()[CACHE_CONTROL], IMMUTABLE_CACHE_CONTROL);
    }

    #[tokio::test]
    async fn backend_and_missing_asset_paths_never_return_the_spa() {
        for path in ["/api/v1/missing", "/metrics/missing", "/assets/missing.js"] {
            let response = request(Method::GET, path, Some("text/html")).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "path: {path}");
        }
    }

    #[tokio::test]
    async fn head_returns_the_index_length_without_a_body() {
        let response = request(Method::HEAD, "/", None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            index_asset().body.len().to_string()
        );
        assert!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
