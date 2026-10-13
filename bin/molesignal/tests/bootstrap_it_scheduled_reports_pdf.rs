// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Integration test：scheduled-reports headless Chrome 端到端冒烟测试。
//!
//! **要求**：
//! - host 上装 chromium / Google Chrome
//! - 配置启用 scheduled report renderer
//!
//! 默认 `#[ignore]`，CI 在装 chromium 的镜像里手动 `cargo test --test
//! bootstrap_it_scheduled_reports_pdf -- --ignored`。
//!
//! 用 axum 起一个 fake `/dashboards/:id` 前端路由返简单 HTML，然后调
//! `HeadlessChromeRenderer::render` 截图，校验 PNG / PDF magic bytes，以及出图尺寸跟随
//! 页面内容而不是视口。`/dashboards/guarded` 像真实前端一样先看登录态：没有就去登录页，
//! 用来验证定时投递带着登录态渲染。

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{Router, extract::Path, routing::get};
use molesignal::{
    app::reporting::ReportRenderSession,
    bootstrap::workers::scheduled_reports::ScheduledReportRunner,
    infra::persistence::repositories::scheduled_reports::{
        DeliveryStatus, ReportDelivery, ReportRecipient, ScheduledReport, ScheduledReportRepository,
    },
    report_renderer::{HeadlessChromeRenderer, RendererConfig},
    shared::{
        Error, ReportFormat, ReportRenderer, Result, Viewport, ids::Id, time::TimestampMicros,
    },
};
use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory};
use tokio::net::TcpListener;

/// 真实前端的两种去登录页的方式：没有登录态时路由在同一份文档里换到登录页；登录态被后端
/// 拒绝（401）时整页跳转。有可用的登录态才显示内容并说 `ready`。
const GUARDED_REPORT_PAGE: &str = r#"<!doctype html><html><body style='margin:0'>
<main id='content'></main>
<script>
  const stored = localStorage.getItem('molesignal-auth');
  const session = stored ? JSON.parse(stored) : null;
  const token = session?.state?.token;
  const signin = '/signin?next=' + encodeURIComponent(location.pathname + location.search);
  const content = document.getElementById('content');
  if (!token) {
    history.replaceState(null, '', signin);
    content.textContent = 'Sign in to MoleSignal';
  } else if (token === 'rejected-token') {
    location.assign(signin);
  } else {
    content.textContent = 'report rendered as ' + session.state.ctx.user_id;
    content.style.cssText = 'display:block;height:500px;background:#dbeafe';
    document.documentElement.dataset.reportRenderState = 'ready';
  }
</script></body></html>"#;

fn browser_session(token: &str) -> String {
    format!(
        r#"{{"state":{{"token":"{token}","ctx":{{"user_id":"ada","org_id":"o1"}}}},"version":0}}"#
    )
}

async fn spawn_fake_embed_server() -> u16 {
    let app = Router::new()
        .route(
            "/signin",
            get(|| async {
                axum::response::Html(
                    "<!doctype html><html><body>Sign in to MoleSignal</body></html>",
                )
            }),
        )
        .route(
            "/dashboards/{id}",
            get(|Path(id): Path<String>| async move {
                if id == "guarded" {
                    return axum::response::Html(GUARDED_REPORT_PAGE.to_owned());
                }
                if id == "error" {
                    return axum::response::Html(
                        "<html data-report-render-state='error' \
                     data-report-render-error='dashboard model is invalid'>\
                     <body>Unexpected Application Error!</body></html>"
                            .to_owned(),
                    );
                }
                // 报表页的形状：文档随内容增高，高度可以带小数，也可以比视口矮。doctype 不能省：
                // 怪异模式下 body 总是撑满视口，量不出比视口矮的内容。
                if let Some(height) = report_document_height(&id) {
                    return axum::response::Html(format!(
                        "<!doctype html><html data-report-render-state='ready'>\
                     <body style='margin:0'>\
                     <div style='height:{height}px;overflow:auto;\
                     background:linear-gradient(#dbeafe,#1d4ed8)'>report document</div>\
                     </body></html>"
                    ));
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

fn report_document_height(id: &str) -> Option<&'static str> {
    match id {
        "tall" => Some("2400.4"),
        "short" => Some("300"),
        _ => None,
    }
}

/// PNG 的像素尺寸（IHDR 的宽、高）。
fn png_size(png: &[u8]) -> (u32, u32) {
    let field = |offset: usize| {
        u32::from_be_bytes(
            png[offset..offset + 4]
                .try_into()
                .expect("IHDR field is four bytes"),
        )
    };

    (field(16), field(20))
}

/// PDF 每一页的尺寸（MediaBox 的宽、高，单位 pt）。
fn pdf_page_sizes(pdf: &[u8]) -> Vec<(f64, f64)> {
    let text = String::from_utf8_lossy(pdf);

    text.match_indices("/MediaBox")
        .filter_map(|(index, _)| {
            let rest = &text[index..];
            let numbers: Vec<f64> = rest[rest.find('[')? + 1..rest.find(']')?]
                .split_whitespace()
                .filter_map(|number| number.parse().ok())
                .collect();

            (numbers.len() == 4).then(|| (numbers[2], numbers[3]))
        })
        .collect()
}

async fn render_report_document(id: &str, format: ReportFormat) -> Vec<u8> {
    let port = spawn_fake_embed_server().await;
    let renderer = HeadlessChromeRenderer::new(RendererConfig::default());
    let url = format!("http://127.0.0.1:{port}/dashboards/{id}?report_render=1");

    renderer
        .render(&url, format, Viewport::default(), None)
        .await
        .expect("report document must render")
        .to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn png_covers_the_whole_report_document() {
    // 视口是 1280x800：比它高的内容整份截下来（小数高度向上取整），比它矮的不留空白。
    let tall = render_report_document("tall", ReportFormat::Png).await;
    let short = render_report_document("short", ReportFormat::Png).await;

    assert_eq!(png_size(&tall), (1280, 2401));
    assert_eq!(png_size(&short), (1280, 300));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn pdf_holds_the_whole_report_document_on_one_page() {
    let tall = pdf_page_sizes(&render_report_document("tall", ReportFormat::Pdf).await);
    let short = pdf_page_sizes(&render_report_document("short", ReportFormat::Pdf).await);

    // 1px = 0.75pt；纸张比内容多 1px，避免内容被推到第二页。
    assert_eq!(tall.len(), 1, "tall report must not paginate: {tall:?}");
    assert!((tall[0].0 - 960.0).abs() < 0.5, "{tall:?}");
    assert!((tall[0].1 - 2402.0 * 0.75).abs() < 0.5, "{tall:?}");

    assert_eq!(short.len(), 1, "{short:?}");
    assert!((short[0].1 - 301.0 * 0.75).abs() < 0.5, "{short:?}");
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
        .render(&url, ReportFormat::Png, Viewport::CONFIGURED, None)
        .await
        .expect("png render must succeed");
    assert!(bytes.len() > 100, "png output suspiciously small");
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'],
        "expected PNG magic bytes"
    );
    // 调用方不指定尺寸时用 renderer 配置的视口；这个页面不比视口高。
    assert_eq!(png_size(&bytes), (800, 600));
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
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-artifacts");
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

async fn render_guarded_page(
    format: ReportFormat,
    session: Option<&str>,
) -> (Result<Vec<u8>>, Duration) {
    let port = spawn_fake_embed_server().await;
    let renderer = HeadlessChromeRenderer::new(RendererConfig::default());
    let url = format!("http://127.0.0.1:{port}/dashboards/guarded?report_render=1");
    let started = Instant::now();

    let rendered = renderer
        .render(&url, format, Viewport::default(), session)
        .await
        .map(|bytes| bytes.to_vec());

    (rendered, started.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn a_report_page_rendered_with_a_session_is_signed_in() {
    let session = browser_session("render-token");
    let (rendered, _) = render_guarded_page(ReportFormat::Png, Some(&session)).await;

    // 500px 高的内容只在登录后才有：登录态确实进了页面。
    let png = rendered.expect("a signed-in report page must render");
    assert_eq!(png_size(&png), (1280, 500));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn a_report_page_rendered_without_a_session_fails_with_the_reason() {
    let (rendered, took) = render_guarded_page(ReportFormat::Pdf, None).await;

    let error = rendered.expect_err("a page that asks to sign in must not be exported");
    assert!(
        error.to_string().contains("asked to sign in"),
        "unexpected render error: {error}"
    );
    // 认出登录页就失败，不等满就绪协议的 15 秒。
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn a_report_page_whose_session_is_rejected_fails_with_the_reason() {
    // 后端拒绝 token 时前端整页跳去登录页，正在等待就绪的执行上下文随之销毁。
    let session = browser_session("rejected-token");
    let (rendered, _) = render_guarded_page(ReportFormat::Pdf, Some(&session)).await;

    let error = rendered.expect_err("a rejected session must not be exported");
    assert!(
        error.to_string().contains("asked to sign in"),
        "unexpected render error: {error}"
    );
}

// ───────────── 定时投递：worker 带着登录态渲染并投递 ─────────────

/// 只有一份报表的内存仓储，记下投递记录。
struct MemoryReports {
    report: Mutex<ScheduledReport>,
    deliveries: Mutex<Vec<ReportDelivery>>,
}

impl MemoryReports {
    fn holding(report: ScheduledReport) -> Arc<Self> {
        Arc::new(Self {
            report: Mutex::new(report),
            deliveries: Mutex::new(Vec::new()),
        })
    }

    fn deliveries(&self) -> Vec<ReportDelivery> {
        self.deliveries.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ScheduledReportRepository for MemoryReports {
    async fn create(&self, report: ScheduledReport) -> Result<ScheduledReport> {
        Ok(report)
    }
    async fn update(&self, report: ScheduledReport) -> Result<ScheduledReport> {
        Ok(report)
    }
    async fn get(&self, _: &Id, id: &Id) -> Result<ScheduledReport> {
        self.get_by_id(id).await
    }
    async fn get_by_id(&self, _: &Id) -> Result<ScheduledReport> {
        Ok(self.report.lock().unwrap().clone())
    }
    async fn list(&self, _: &Id) -> Result<Vec<ScheduledReport>> {
        self.list_enabled_all().await
    }
    async fn list_enabled_all(&self) -> Result<Vec<ScheduledReport>> {
        Ok(vec![self.report.lock().unwrap().clone()])
    }
    async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
        Ok(())
    }
    async fn touch_last_run(&self, _: &Id, at: TimestampMicros) -> Result<()> {
        self.report.lock().unwrap().last_run_at = Some(at);
        Ok(())
    }
    async fn record_delivery(&self, delivery: ReportDelivery) -> Result<()> {
        self.deliveries.lock().unwrap().push(delivery);
        Ok(())
    }
    async fn list_deliveries(&self, _: &Id, _: &Id) -> Result<Vec<ReportDelivery>> {
        Ok(self.deliveries())
    }
}

/// 报表以谁的身份渲染由它决定：给出一份登录态，或者说明为什么没有。
enum Identity {
    Session(String),
    Unusable(&'static str),
}

#[async_trait::async_trait]
impl ReportRenderSession for Identity {
    async fn issue(&self, _: &ScheduledReport) -> Result<String> {
        match self {
            Self::Session(session) => Ok(session.clone()),
            Self::Unusable(reason) => Err(Error::forbidden(*reason)),
        }
    }
}

fn guarded_dashboard_report(format: &str) -> ScheduledReport {
    ScheduledReport {
        id: Id("weekly".into()),
        org_id: Id("o1".into()),
        name: "Weekly checkout".into(),
        dashboard_id: Some(Id("guarded".into())),
        saved_view_id: None,
        cron: "every:7d".into(),
        recipients: vec![ReportRecipient {
            kind: "s3".into(),
            target: "reports/weekly".into(),
        }],
        format: format.into(),
        time_range_json: serde_json::json!({ "preset": "previous-7-days" }),
        enabled: true,
        run_as_user_id: Some(Id("ada".into())),
        last_run_at: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

async fn run_scheduled_report(
    format: &str,
    identity: Identity,
) -> (Arc<MemoryReports>, Arc<InMemory>) {
    let port = spawn_fake_embed_server().await;
    let reports = MemoryReports::holding(guarded_dashboard_report(format));
    let store = Arc::new(InMemory::new());

    let runner = ScheduledReportRunner::new(
        reports.clone(),
        store.clone(),
        Some(
            Arc::new(HeadlessChromeRenderer::new(RendererConfig::default()))
                as Arc<dyn ReportRenderer>,
        ),
        format!("http://127.0.0.1:{port}"),
        Arc::new(identity),
    );

    assert_eq!(runner.tick_once().await.expect("tick"), 1);
    (reports, store)
}

async fn delivered_files(store: &InMemory) -> Vec<Vec<u8>> {
    let objects: Vec<_> = futures::TryStreamExt::try_collect(store.list(None))
        .await
        .expect("list delivered objects");
    let mut files = Vec::new();

    for object in objects {
        let body = store.get(&object.location).await.expect("read object");
        files.push(body.bytes().await.expect("object bytes").to_vec());
    }

    files
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn a_scheduled_report_is_rendered_signed_in_and_delivered() {
    for (format, signature) in [("pdf", &b"%PDF-"[..]), ("png", &b"\x89PNG"[..])] {
        let identity = Identity::Session(browser_session("render-token"));
        let (reports, store) = run_scheduled_report(format, identity).await;

        let files = delivered_files(&store).await;
        assert_eq!(files.len(), 1, "{format}");
        assert!(files[0].starts_with(signature), "{format}");

        let deliveries = reports.deliveries();
        assert_eq!(deliveries.len(), 1, "{format}");
        assert_eq!(deliveries[0].status, DeliveryStatus::Sent, "{format}");
        assert_eq!(deliveries[0].error, None, "{format}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires headless chromium on host"]
async fn a_scheduled_report_with_no_identity_records_why_it_was_not_delivered() {
    let identity = Identity::Unusable("the report runs as ada@example.com, who is disabled");
    let (reports, store) = run_scheduled_report("pdf", identity).await;

    assert!(delivered_files(&store).await.is_empty());
    let deliveries = reports.deliveries();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].status, DeliveryStatus::Failed);
    assert!(
        deliveries[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("ada@example.com, who is disabled")),
        "{:?}",
        deliveries[0].error
    );
}
