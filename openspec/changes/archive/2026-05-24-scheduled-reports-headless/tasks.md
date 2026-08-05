## 1. 新 enterprise crate

- [x] 1.1 `enterprise/crates/report-renderer/Cargo.toml`：deps `tokio` + `bytes` + `headless_chrome = "1"` + `async-trait` + `parking_lot`
- [x] 1.2 `enterprise/Cargo.toml workspace.members` 加 `crates/report-renderer`
- [x] 1.3 主仓 `[patch."ssh://git@github.com/molesignal/molesignal-enterprise.git"]` 加同 entry

## 2. ReportRenderer trait + impl

- [x] 2.1 `enterprise/crates/report-renderer/src/lib.rs`：`ReportRenderer` trait + `HeadlessChromeRenderer { pool: Arc<Mutex<Vec<Browser>>>, semaphore: Arc<Semaphore>, timeout: Duration }`
- [x] 2.2 `async fn render(target_url, format, viewport, session_token)`：拿 permit → 取 / 起 Chrome instance → load page → `tokio::time::timeout` 包 → `print_to_pdf` 或 `capture_screenshot`
- [x] 2.3 instance crash 自动从池里移除
- [x] 2.4 配置 clamp：pool ≤ 4，timeout ≤ 60s

## 3. wire 注入

- [x] 3.1 `crates/api/src/state.rs::AppState` 加 `pub report_renderer: Option<Arc<dyn ReportRenderer>>`
- [x] 3.2 `crates/bootstrap/src/wire.rs` cfg=enterprise + `settings.scheduled_reports.renderer.enabled`：构造 HeadlessChromeRenderer 注入
- [x] 3.3 OSS 编译：直接 None

## 4. Runner 接入

- [x] 4.1 `crates/bootstrap/src/workers/scheduled_reports.rs::render`：format=png|pdf 且 `state.report_renderer.is_some()` → 调 renderer；else SVG fallback + warn once
- [x] 4.2 失败时 `report_deliveries.error` 记 timeout / chrome crash / page error
- [x] 4.3 短 session token：调 `identity.issue_token(user, org, role)` with TTL=300s

## 5. 配置 + 部署

- [x] 5.1 `crates/config/src/settings.rs::ScheduledReportsSettings.renderer: RendererSettings { enabled, concurrent_renders, render_timeout_secs, viewport_width, viewport_height }`
- [x] 5.2 `deploy/docker/Dockerfile` enterprise build stage：`RUN apk add --no-cache chromium nss freetype harfbuzz ca-certificates ttf-freefont`
- [x] 5.3 `deploy/k8s/95-ingress.yaml`：scheduled_reports role pod template 加 chromium image 标注

## 6. 测试

- [x] 6.1 unit：`HeadlessChromeRenderer::render` 在 chrome 不存在时返友好错误（不 panic）
- [x] 6.2 unit：timeout 触发时返 `Err(RenderError::Timeout)` + Chrome 实例从池移除
- [x] 6.3 unit：format=svg 路径仍走 SVG（renderer 不被调）
- [x] 6.4 integration test `it_scheduled_reports_pdf.rs`（require_docker + chrome）：起 axum + 注入 fake dashboard 路由返简单 HTML → renderer 截图 → 验 bytes 前缀

## 7. 编译矩阵

- [x] 7.1 `cargo check --workspace` (OSS) clean
- [x] 7.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 7.3 `cd enterprise && cargo test -p molesignal-enterprise-report-renderer` 单测全绿
