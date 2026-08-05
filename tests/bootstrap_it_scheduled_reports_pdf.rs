// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Integration test：scheduled-reports headless Chrome 端到端冒烟测试。
//!
//! **要求**：
//! - host 上装 chromium / Google Chrome
//! - `--features ` 启用  build（headless renderer 才注入）
//!
//! 默认 `#[ignore]`，CI 在装 chromium 的镜像里手动 `cargo test --test
//! it_scheduled_reports_pdf -- --ignored`。
//!
//! 用 axum 起一个 fake `/dashboards/:id` 前端路由返简单 HTML，然后调
//! `HeadlessChromeRenderer::render` 截图，校验 PNG / PDF magic bytes。

use std::time::Duration;

use axum::{Router, extract::Path, routing::get};
use molesignal::{
    report_renderer::{HeadlessChromeRenderer, RendererConfig},
    shared::{ReportFormat, ReportRenderer, Viewport},
};
use tokio::net::TcpListener;

async fn spawn_fake_embed_server() -> u16 {
    let app = Router::new().route(
        "/dashboards/{id}",
        get(|Path(id): Path<String>| async move {
            if id == "error" {
                return axum::response::Html(
                    "<html data-report-render-state='error' \
                     data-report-render-error='dashboard model is invalid'>\
                     <body>Unexpected Application Error!</body></html>"
                        .to_owned(),
                );
            }
            axum::response::Html(
                "<html><body><h1 id='result' style='font-size:48px'></h1>\
                 <script>document.getElementById('result').textContent = \
                 `scheduled-reports-headless smoke · ${window.innerWidth}px`;\
                 document.documentElement.dataset.reportRenderState = 'ready';\
                 </script></body></html>"
                    .to_owned(),
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    // 等监听就绪。
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn png_render_returns_image_bytes() {
    let port = spawn_fake_embed_server().await;
    let renderer = HeadlessChromeRenderer::new(RendererConfig {
        concurrent_renders: 1,
        render_timeout_secs: 30,
        viewport: Viewport {
            width: 800,
            height: 600,
        },
    });
    let url = format!("http://127.0.0.1:{port}/dashboards/d1?report_render=1");
    let bytes = renderer
        .render(&url, ReportFormat::Png, Viewport::default(), None)
        .await
        .expect("png render must succeed");
    assert!(bytes.len() > 100, "png output suspiciously small");
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'],
        "expected PNG magic bytes"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn pdf_render_starts_with_pdf_magic() {
    let port = spawn_fake_embed_server().await;
    let renderer = HeadlessChromeRenderer::new(RendererConfig::default());
    let url = format!("http://127.0.0.1:{port}/dashboards/d1?report_render=1");
    let browser_auth_storage = r#"{"state":{"token":"report-render-test-token","ctx":{"user_id":"u1","org_id":"o1"}},"version":0}"#;
    let bytes = renderer
        .render(
            &url,
            ReportFormat::Pdf,
            Viewport::default(),
            Some(browser_auth_storage),
        )
        .await
        .expect("pdf render must succeed");
    assert!(bytes.starts_with(b"%PDF-"), "expected %PDF- header");
    let artifact_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create PDF test artifact directory");
    std::fs::write(artifact_dir.join("scheduled-report-smoke.pdf"), &bytes)
        .expect("write PDF test artifact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn application_error_page_is_not_exported_as_pdf() {
    let port = spawn_fake_embed_server().await;
    let renderer = HeadlessChromeRenderer::new(RendererConfig::default());
    let url = format!("http://127.0.0.1:{port}/dashboards/error?report_render=1");
    let error = renderer
        .render(&url, ReportFormat::Pdf, Viewport::default(), None)
        .await
        .expect_err("application error page must fail instead of returning a PDF");
    assert!(
        error.to_string().contains("dashboard model is invalid"),
        "unexpected render error: {error}"
    );
}
