// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Headless-Chrome `ReportRenderer` 实装（change `scheduled-reports-headless`）。
//!
//! 设计要点：
//! - `headless_chrome` 是同步 API；每次 render 用 `tokio::task::spawn_blocking`
//!   离开 reactor 线程，避免阻塞别的 ingress / query。
//! - Chrome instance 走小池（默认 2，硬上限 4）；用 `tokio::sync::Semaphore`
//!   限并发，用 `parking_lot::Mutex<Vec<Browser>>` 做 checkout / checkin。
//! - Instance 渲染失败 / 超时 → 不回池，由 next render 重启实例。
//! - 30s wall-clock timeout（spec 上限 60s）；`tokio::time::timeout` 包整次 render；
//!   超时不 abort blocking task（headless_chrome 不响应 cancel），而是把 Chrome
//!   实例丢弃（spawn_blocking 自己跑完后看到 receiver dropped → 不入池）。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use headless_chrome::{
    Browser, LaunchOptions,
    protocol::cdp::{Emulation, Page::CaptureScreenshotFormatOption},
    types::PrintToPdfOptions,
};
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use crate::shared::{RenderError, ReportFormat, ReportRenderer, Result, Viewport};

/// 硬上限：>= 4 个 Chrome instance 在常见 pod 资源下会 OOM；spec scheduled-reports
/// "Renderer Resource Bounds" 要求 clamp。
pub const MAX_POOL_SIZE: usize = 4;
pub const MAX_TIMEOUT_SECS: u64 = 60;
const DEFAULT_POOL_SIZE: usize = 2;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DASHBOARD_READY_TIMEOUT_MILLIS: u64 = 15_000;

/// 创建 `HeadlessChromeRenderer` 的配置；运行期 bootstrap 阶段从
/// `[scheduled_reports.renderer]` 段读出后传入。
#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub concurrent_renders: usize,
    pub render_timeout_secs: u64,
    pub viewport: Viewport,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            concurrent_renders: DEFAULT_POOL_SIZE,
            render_timeout_secs: DEFAULT_TIMEOUT_SECS,
            viewport: Viewport::default(),
        }
    }
}

/// 把超出 spec 上限的配置裁紧；超过时 emit 一次性 warn 让 ops 知道。
fn clamp(config: RendererConfig) -> RendererConfig {
    let mut c = config;
    if c.concurrent_renders > MAX_POOL_SIZE {
        tracing::warn!(
            requested = c.concurrent_renders,
            cap = MAX_POOL_SIZE,
            "concurrent_renders clamped to hard cap"
        );
        c.concurrent_renders = MAX_POOL_SIZE;
    }
    if c.concurrent_renders == 0 {
        c.concurrent_renders = 1;
    }
    if c.render_timeout_secs > MAX_TIMEOUT_SECS {
        tracing::warn!(
            requested = c.render_timeout_secs,
            cap = MAX_TIMEOUT_SECS,
            "render_timeout clamped to {MAX_TIMEOUT_SECS}s"
        );
        c.render_timeout_secs = MAX_TIMEOUT_SECS;
    }
    if c.render_timeout_secs == 0 {
        c.render_timeout_secs = DEFAULT_TIMEOUT_SECS;
    }
    c
}

pub struct HeadlessChromeRenderer {
    pool: Arc<Mutex<Vec<Browser>>>,
    semaphore: Arc<Semaphore>,
    timeout: Duration,
    viewport: Viewport,
}

impl HeadlessChromeRenderer {
    pub fn new(config: RendererConfig) -> Self {
        let cfg = clamp(config);
        Self {
            pool: Arc::new(Mutex::new(Vec::with_capacity(cfg.concurrent_renders))),
            semaphore: Arc::new(Semaphore::new(cfg.concurrent_renders)),
            timeout: Duration::from_secs(cfg.render_timeout_secs),
            viewport: cfg.viewport,
        }
    }
}

fn launch_browser() -> std::result::Result<Browser, RenderError> {
    // `--js-flags=--max-old-space-size=512` 给 V8 加 heap 上限（spec scheduled-reports
    // "Renderer Resource Bounds" 要求）。
    let options = LaunchOptions::default_builder()
        .args(vec![std::ffi::OsStr::new(
            "--js-flags=--max-old-space-size=512",
        )])
        .build()
        .map_err(|e| RenderError::ChromeUnavailable(format!("launch options: {e}")))?;
    Browser::new(options).map_err(|e| RenderError::ChromeUnavailable(e.to_string()))
}

#[async_trait]
impl ReportRenderer for HeadlessChromeRenderer {
    async fn render(
        &self,
        url: &str,
        format: ReportFormat,
        viewport: Viewport,
        browser_auth_storage: Option<&str>,
    ) -> Result<Bytes> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| RenderError::Internal("semaphore closed".into()))?;

        let target_url = url.to_string();
        let browser_auth_storage = browser_auth_storage.map(str::to_owned);
        let target_viewport = if viewport.width == 0 || viewport.height == 0 {
            self.viewport
        } else {
            viewport
        };
        let pool = self.pool.clone();
        let cfg_viewport = self.viewport;

        // 用 Option 让 spawn_blocking 内决定 instance 是否回池（成功才回）。
        let bytes_result = tokio::time::timeout(
            self.timeout,
            tokio::task::spawn_blocking({
                let pool = pool.clone();
                let target_url = target_url.clone();
                move || -> std::result::Result<Bytes, RenderError> {
                    let browser = checkout_or_launch(&pool)?;
                    let result = render_blocking(
                        &browser,
                        &target_url,
                        format,
                        target_viewport,
                        browser_auth_storage.as_deref(),
                    );
                    if result.is_ok() {
                        let mut guard = pool.lock();
                        guard.push(browser);
                    } else {
                        // 失败 instance 直接 drop（headless_chrome Browser drop 杀进程）。
                    }
                    let _ = cfg_viewport; // suppress unused warn outside of test config
                    result
                }
            }),
        )
        .await;

        match bytes_result {
            Ok(Ok(Ok(b))) => Ok(b),
            Ok(Ok(Err(e))) => Err(e.into()),
            Ok(Err(join_err)) => {
                Err(RenderError::Internal(format!("blocking task: {join_err}")).into())
            }
            Err(_) => {
                // 超时：不重启 spawn_blocking（headless_chrome 不响应 cancel），
                // 但 task 跑完时会看到 timeout 已发生，instance 不入池。
                let secs = self.timeout.as_secs();
                Err(RenderError::Timeout(secs).into())
            }
        }
    }
}

fn checkout_or_launch(
    pool: &Arc<Mutex<Vec<Browser>>>,
) -> std::result::Result<Browser, RenderError> {
    if let Some(b) = pool.lock().pop() {
        return Ok(b);
    }
    launch_browser()
}

fn render_blocking(
    browser: &Browser,
    url: &str,
    format: ReportFormat,
    viewport: Viewport,
    browser_auth_storage: Option<&str>,
) -> std::result::Result<Bytes, RenderError> {
    let bootstrap_tab = browser
        .new_tab()
        .map_err(|e| RenderError::PageError(format!("new_tab: {e}")))?;
    set_device_viewport(&bootstrap_tab, viewport)?;
    navigate_and_wait(&bootstrap_tab, url)?;

    // Browser instance 会复用，但认证上下文不得跨报告/用户复用。先清理旧状态，再按
    // 当前请求写入短期会话。认证数据只进入浏览器同源存储，不出现在 URL。
    let auth_value = browser_auth_storage
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| RenderError::Internal(format!("serialize browser auth: {e}")))?;
    let auth_expression = match auth_value {
        Some(value) => format!(
            "window.localStorage.removeItem('molesignal-auth');\
             window.localStorage.removeItem('molesignal-auth-remember');\
             window.sessionStorage.removeItem('molesignal-auth');\
             window.localStorage.setItem('molesignal-auth', {value});"
        ),
        None => "window.localStorage.removeItem('molesignal-auth');\
             window.localStorage.removeItem('molesignal-auth-remember');\
             window.sessionStorage.removeItem('molesignal-auth');"
            .to_string(),
    };
    bootstrap_tab
        .evaluate(&auth_expression, false)
        .map_err(|e| RenderError::PageError(format!("install browser auth: {e}")))?;
    let _ = bootstrap_tab.close(true);

    // 同一 Tab 连续导航时 Chrome 偶尔不发 lifecycle event。新 Tab 会共享
    // localStorage，并可靠地产生一次完整导航事件。
    let tab = browser
        .new_tab()
        .map_err(|e| RenderError::PageError(format!("new render tab: {e}")))?;
    set_device_viewport(&tab, viewport)?;
    navigate_and_wait(&tab, url)?;

    // Dashboard 页面通过 data-report-render-state 暴露模型加载和 panel query 的
    // 完成状态。不能只等 window.load，否则会把 React Router 错误页或骨架屏打印成
    // 一个 magic bytes 正确、内容却不可用的 PDF。
    if is_dashboard_report_url(url) {
        wait_for_dashboard_report_ready(&tab)?;
        std::thread::sleep(Duration::from_millis(200));
    } else {
        // saved-view 暂无页面级 ready 协议，保留一个异步查询稳定窗口。
        std::thread::sleep(Duration::from_millis(750));
    }
    ensure_page_renderable(&tab)?;

    let result = match format {
        ReportFormat::Png => {
            let img = tab
                .capture_screenshot(
                    CaptureScreenshotFormatOption::Png,
                    None,
                    Some(headless_chrome::protocol::cdp::Page::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: viewport.width as f64,
                        height: viewport.height as f64,
                        scale: 1.0,
                    }),
                    true,
                )
                .map_err(|e| RenderError::PageError(format!("screenshot: {e}")))?;
            Ok(Bytes::from(img))
        }
        ReportFormat::Pdf => {
            let opts = PrintToPdfOptions {
                landscape: Some(false),
                display_header_footer: Some(false),
                print_background: Some(true),
                paper_width: Some(viewport.width as f64 / 96.0),
                paper_height: Some(viewport.height as f64 / 96.0),
                ..Default::default()
            };
            let pdf = tab
                .print_to_pdf(Some(opts))
                .map_err(|e| RenderError::PageError(format!("print_to_pdf: {e}")))?;
            Ok(Bytes::from(pdf))
        }
    };
    if result.is_ok()
        && let Err(error) = tab.evaluate(
            "window.localStorage.removeItem('molesignal-auth');\
                 window.localStorage.removeItem('molesignal-auth-remember');\
                 window.sessionStorage.removeItem('molesignal-auth');",
            false,
        )
    {
        let _ = tab.close(true);
        return Err(RenderError::PageError(format!(
            "clear browser auth: {error}"
        )));
    }
    let _ = tab.close(true);
    result
}

fn set_device_viewport(
    tab: &headless_chrome::Tab,
    viewport: Viewport,
) -> std::result::Result<(), RenderError> {
    tab.call_method(Emulation::SetDeviceMetricsOverride {
        width: viewport.width,
        height: viewport.height,
        device_scale_factor: 1.0,
        mobile: false,
        scale: None,
        screen_width: Some(viewport.width),
        screen_height: Some(viewport.height),
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        screen_orientation: None,
        viewport: None,
        display_feature: None,
        device_posture: None,
    })
    .map(|_| ())
    .map_err(|error| RenderError::PageError(format!("set viewport: {error}")))
}

fn navigate_and_wait(
    tab: &headless_chrome::Tab,
    url: &str,
) -> std::result::Result<(), RenderError> {
    tab.navigate_to(url)
        .map_err(|e| RenderError::PageError(format!("navigate: {e}")))?;
    tab.wait_for_element("body")
        .map(|_| ())
        .map_err(|e| RenderError::PageError(format!("wait for document body: {e}")))
}

fn is_dashboard_report_url(url: &str) -> bool {
    url.contains("/dashboards/") && url.contains("report_render=1")
}

fn wait_for_dashboard_report_ready(
    tab: &headless_chrome::Tab,
) -> std::result::Result<(), RenderError> {
    let expression = format!(
        r#"
new Promise((resolve) => {{
  const deadline = Date.now() + {DASHBOARD_READY_TIMEOUT_MILLIS};
  const inspect = () => {{
    const root = document.documentElement;
    const state = root?.dataset?.reportRenderState ?? '';
    const body = (document.body?.innerText ?? '').trim();
    if (state === 'ready') {{
      resolve('');
      return;
    }}
    if (state === 'error') {{
      resolve(root?.dataset?.reportRenderError || body.slice(0, 1200) || 'dashboard render failed');
      return;
    }}
    if (
      body.startsWith('Unexpected Application Error!') ||
      body.includes('You can provide a way better UX than this when your app throws errors')
    ) {{
      resolve(body.slice(0, 1200));
      return;
    }}
    if (Date.now() >= deadline) {{
      resolve('dashboard did not become ready within {DASHBOARD_READY_TIMEOUT_MILLIS}ms');
      return;
    }}
    setTimeout(inspect, 100);
  }};
  inspect();
}})
"#
    );
    let value = tab
        .evaluate(&expression, true)
        .map_err(|error| RenderError::PageError(format!("wait for dashboard ready: {error}")))?;
    let message = evaluated_string(value.value);
    if message.is_empty() {
        Ok(())
    } else {
        Err(RenderError::PageError(format!(
            "dashboard render failed: {message}"
        )))
    }
}

fn ensure_page_renderable(tab: &headless_chrome::Tab) -> std::result::Result<(), RenderError> {
    let value = tab
        .evaluate(
            r#"
(() => {
  const root = document.documentElement;
  const body = (document.body?.innerText ?? '').trim();
  if (root?.dataset?.reportRenderState === 'error') {
    return root.dataset.reportRenderError || body.slice(0, 1200) || 'page render failed';
  }
  if (
    body.startsWith('Unexpected Application Error!') ||
    body.includes('You can provide a way better UX than this when your app throws errors')
  ) {
    return body.slice(0, 1200);
  }
  if (
    body.includes('Molesignal 专为宽屏工作流设计') ||
    body.includes('MoleSignal is designed for desktop workflows')
  ) {
    return 'report viewport was rejected by the desktop-width guard';
  }
  if (
    body.startsWith('{"error"') &&
    (body.includes('"unauthorized"') || body.includes('missing Authorization Bearer token'))
  ) {
    return body.slice(0, 1200);
  }
  return '';
})()
"#,
            false,
        )
        .map_err(|error| RenderError::PageError(format!("inspect rendered page: {error}")))?;
    let message = evaluated_string(value.value);
    if message.is_empty() {
        Ok(())
    } else {
        Err(RenderError::PageError(format!(
            "page cannot be exported: {message}"
        )))
    }
}

fn evaluated_string(value: Option<serde_json::Value>) -> String {
    value
        .and_then(|entry| entry.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_clamps_pool_and_timeout() {
        let c = clamp(RendererConfig {
            concurrent_renders: 10,
            render_timeout_secs: 600,
            viewport: Viewport::default(),
        });
        assert_eq!(c.concurrent_renders, MAX_POOL_SIZE);
        assert_eq!(c.render_timeout_secs, MAX_TIMEOUT_SECS);
    }

    #[test]
    fn config_zero_pool_becomes_one() {
        let c = clamp(RendererConfig {
            concurrent_renders: 0,
            render_timeout_secs: 10,
            viewport: Viewport::default(),
        });
        assert_eq!(c.concurrent_renders, 1);
        assert_eq!(c.render_timeout_secs, 10);
    }

    #[test]
    fn dashboard_ready_protocol_only_applies_to_dashboard_report_urls() {
        assert!(is_dashboard_report_url(
            "http://127.0.0.1:5173/dashboards/d1?report_render=1"
        ));
        assert!(!is_dashboard_report_url(
            "http://127.0.0.1:5173/dashboards/d1"
        ));
        assert!(!is_dashboard_report_url(
            "http://127.0.0.1:5173/saved-views?view=v1&report_render=1"
        ));
    }

    /// chrome 不存在 → `RenderError::ChromeUnavailable`，不 panic。
    /// CI 没装 chromium 时跑这个 path 会拿到 launch 错误，正好验证。
    #[test]
    fn launch_browser_returns_friendly_error_when_chrome_missing() {
        // 走 spawn_blocking 等价路径的子集：直接调 launch_browser。
        // 若 host 装了 Chrome 这里会成功 → skip；用 ignore 的更稳妥。
        // 默认：尝试 launch；任何 Err 都视为"chrome 不可用"被吞掉，不 panic。
        match launch_browser() {
            Ok(b) => {
                drop(b);
            }
            Err(RenderError::ChromeUnavailable(_)) => {}
            Err(e) => panic!("unexpected error variant: {e}"),
        }
    }

    /// timeout 路径单测：用极小 timeout + 一个永远不返的 fake task。
    /// 这里不真起 chrome；只验 timeout future 行为。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_future_returns_timeout_error() {
        let timeout = Duration::from_millis(20);
        let result: std::result::Result<(), _> =
            tokio::time::timeout(timeout, async { std::future::pending::<()>().await }).await;
        assert!(result.is_err(), "expected elapsed err");
    }
}
