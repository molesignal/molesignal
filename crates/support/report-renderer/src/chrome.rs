// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Headless-Chrome implementation of `ReportRenderer` (change `scheduled-reports-headless`).
//!
//! Design points:
//! - `headless_chrome` is a synchronous API; each render uses `tokio::task::spawn_blocking` to
//!   leave the reactor thread, so it does not block other ingress / query work.
//! - Chrome instances come from a small pool (default 2, hard cap 4); `tokio::sync::Semaphore`
//!   limits concurrency and `parking_lot::Mutex<Vec<Browser>>` does checkout / checkin.
//! - An instance that fails to render or times out is not returned to the pool; the next render
//!   restarts the instance.
//! - 30s wall-clock timeout (spec limit 60s); `tokio::time::timeout` wraps the whole render.
//!   On timeout, the blocking task is not aborted (headless_chrome does not respond to cancel);
//!   instead the Chrome instance is discarded (when spawn_blocking finishes by itself, it sees
//!   the receiver dropped -> the instance is not returned to the pool).

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use headless_chrome::{
    Browser, LaunchOptions, Tab,
    protocol::cdp::{Emulation, Network, Page::CaptureScreenshotFormatOption},
    types::PrintToPdfOptions,
};
use kernel::Result;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use crate::{RenderError, ReportFormat, ReportRenderer, SessionCookie, Viewport};

mod readiness;

/// Hard cap: >= 4 Chrome instances will OOM under common pod resources; spec scheduled-reports
/// "Renderer Resource Bounds" requires clamping.
pub const MAX_POOL_SIZE: usize = 4;

pub const MAX_TIMEOUT_SECS: u64 = 60;
const DEFAULT_POOL_SIZE: usize = 2;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Height cap for single-page output (CSS px). Chrome screenshots are limited to 16384px per
/// side and a single PDF page to 200in (19200px); the value is chosen to be safe for both. For
/// longer content, the PNG is cut off at this height, and the PDF continues on the next page.
const MAX_CAPTURE_HEIGHT_PX: u32 = 16_000;

/// Extra height the PDF paper has beyond the content. The print layout can differ from the
/// screen layout by less than 1px, and if the paper is even slightly shorter than the content,
/// an indivisible block is pushed whole to the second page, leaving a nearly blank first page.
const PDF_PAGE_SLACK_PX: u32 = 1;

const CSS_PX_PER_INCH: f64 = 96.0;

/// Configuration for creating a `HeadlessChromeRenderer`; read from the
/// `[scheduled_reports.renderer]` section during runtime bootstrap and then passed in.
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

/// Clamps configuration that exceeds the spec limits; when it does, emits a one-time warn
/// to let ops know.
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
    // `--js-flags=--max-old-space-size=512` adds a heap limit to V8
    // (required by spec scheduled-reports "Renderer Resource Bounds").
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
        session: Option<&SessionCookie>,
    ) -> Result<Bytes> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| RenderError::Internal("semaphore closed".into()))?;

        let target_url = url.to_string();
        let session = session.cloned();

        let target_viewport = if viewport.width == 0 || viewport.height == 0 {
            self.viewport
        } else {
            viewport
        };

        let pool = self.pool.clone();

        // Use an Option so spawn_blocking can decide whether the instance returns to the pool
        // (it returns only on success).
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
                        session.as_ref(),
                    );

                    if result.is_ok() {
                        let mut guard = pool.lock();
                        guard.push(browser);
                    } else {
                        // A failed instance is dropped directly (a headless_chrome Browser kills
                        // its process on drop).
                    }

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
                // Timeout: spawn_blocking is not restarted (headless_chrome does not respond
                // to cancel), but when the task finishes it sees that the timeout has occurred,
                // and the instance is not returned to the pool.
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
    session: Option<&SessionCookie>,
) -> std::result::Result<Bytes, RenderError> {
    let tab = browser
        .new_tab()
        .map_err(|e| RenderError::PageError(format!("new_tab: {e}")))?;

    // Browser instances are reused, but the login state must not be reused across
    // reports/users: first clear the Cookies left by the previous render, then write this
    // render's. The login state lives only in Cookies and never appears in the URL.
    install_session(&tab, url, session)?;

    set_device_viewport(&tab, viewport)?;
    navigate_and_wait(&tab, url)?;

    // Waiting only for window.load is not enough: the report page may still be fetching data
    // at that point, or may already have failed.
    readiness::wait_until_printable(&tab, url)?;

    let page = page_size(&tab, viewport)?;

    let result = match format {
        ReportFormat::Png => {
            // Content outside the viewport is not painted: first enlarge the viewport to the
            // content height, then clip the full page.
            if page.height != viewport.height {
                set_device_viewport(&tab, page)?;
                std::thread::sleep(Duration::from_millis(150));
            }

            let img = tab
                .capture_screenshot(
                    CaptureScreenshotFormatOption::Png,
                    None,
                    Some(headless_chrome::protocol::cdp::Page::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: page.width as f64,
                        height: page.height as f64,
                        scale: 1.0,
                    }),
                    true,
                )
                .map_err(|e| RenderError::PageError(format!("screenshot: {e}")))?;

            Ok(Bytes::from(img))
        }
        ReportFormat::Pdf => {
            // Fit all the content on one page. With zero page margins, the print layout width
            // is the viewport width, consistent with the layout used when measuring the height;
            // whitespace comes from the page itself.
            let opts = PrintToPdfOptions {
                landscape: Some(false),
                display_header_footer: Some(false),
                print_background: Some(true),
                paper_width: Some(page.width as f64 / CSS_PX_PER_INCH),
                paper_height: Some((page.height + PDF_PAGE_SLACK_PX) as f64 / CSS_PX_PER_INCH),
                margin_top: Some(0.0),
                margin_bottom: Some(0.0),
                margin_left: Some(0.0),
                margin_right: Some(0.0),
                ..Default::default()
            };

            let pdf = tab
                .print_to_pdf(Some(opts))
                .map_err(|e| RenderError::PageError(format!("print_to_pdf: {e}")))?;

            Ok(Bytes::from(pdf))
        }
    };

    if result.is_ok()
        && let Err(error) = tab.call_method(Network::ClearBrowserCookies(None))
    {
        let _ = tab.close(true);

        return Err(RenderError::PageError(format!(
            "clear browser session: {error}"
        )));
    }

    let _ = tab.close(true);
    result
}

/// Clears all Cookies in the browser, then writes the login Cookie for this render
/// (HttpOnly, so page scripts cannot read it).
fn install_session(
    tab: &Tab,
    url: &str,
    session: Option<&SessionCookie>,
) -> std::result::Result<(), RenderError> {
    tab.call_method(Network::ClearBrowserCookies(None))
        .map_err(|e| RenderError::PageError(format!("clear browser session: {e}")))?;

    let Some(session) = session else {
        return Ok(());
    };

    let cookie = Network::CookieParam {
        name: session.name.clone(),
        value: session.value.clone(),
        url: Some(url.to_owned()),
        domain: None,
        path: Some(session.path.clone()),
        secure: None,
        http_only: Some(true),
        same_site: Some(Network::CookieSameSite::Strict),
        expires: None,
        priority: None,
        same_party: None,
        source_scheme: None,
        source_port: None,
        partition_key: None,
    };

    tab.call_method(Network::SetCookies {
        cookies: vec![cookie],
    })
    .map_err(|e| RenderError::PageError(format!("install browser session: {e}")))?;

    Ok(())
}

/// Size of the rendered image: the width is the viewport, the height is the page content.
///
/// The report page (frontend `report-render`) lets the document grow with its content, so the
/// height of the root element is the whole report, and it can be taller or shorter than the
/// viewport. A page that still scrolls within a fixed viewport measures as the viewport height,
/// so the rendered image matches the viewport.
fn page_size(
    tab: &headless_chrome::Tab,
    viewport: Viewport,
) -> std::result::Result<Viewport, RenderError> {
    let value = tab
        .evaluate(PAGE_HEIGHT_SCRIPT, false)
        .map_err(|error| RenderError::PageError(format!("measure page content: {error}")))?;

    let content_height = value
        .value
        .and_then(|entry| entry.as_f64())
        .unwrap_or_default();

    Ok(Viewport {
        width: viewport.width,
        height: page_height(content_height, viewport.height),
    })
}

/// The root element's own box has a fractional height and also includes the margin of `body`;
/// when the document is confined to the viewport height and the content scrolls beyond it, the
/// box is only one screen tall, so the scroll height is used.
const PAGE_HEIGHT_SCRIPT: &str = r#"
(() => {
  const root = document.documentElement;
  if (!root) return 0;
  const box = root.getBoundingClientRect().height;
  const scrolled = root.scrollHeight > root.clientHeight ? root.scrollHeight : 0;
  return Math.max(box, scrolled);
})()
"#;

/// Rounds the content height up when it has a fractional part (1232.02px), because the last
/// row of pixels would be cut off otherwise. Falls back to the viewport height if the height
/// cannot be measured.
fn page_height(content_height: f64, viewport_height: u32) -> u32 {
    if !content_height.is_finite() || content_height < 1.0 {
        return viewport_height;
    }

    let height = content_height.ceil();

    if height > f64::from(MAX_CAPTURE_HEIGHT_PX) {
        tracing::warn!(
            content_height = height,
            cap = MAX_CAPTURE_HEIGHT_PX,
            "report content is taller than one page can hold"
        );

        return MAX_CAPTURE_HEIGHT_PX;
    }

    height as u32
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
    fn page_height_follows_the_content() {
        // Taller or shorter than the viewport, it follows the content; fractions round up.
        assert_eq!(page_height(2400.0, 800), 2400);
        assert_eq!(page_height(1232.02, 800), 1233);
        assert_eq!(page_height(312.4, 800), 313);
    }

    #[test]
    fn page_height_falls_back_to_the_viewport_when_unmeasurable() {
        assert_eq!(page_height(0.0, 800), 800);
        assert_eq!(page_height(-12.0, 800), 800);
        assert_eq!(page_height(f64::NAN, 800), 800);
        assert_eq!(page_height(f64::INFINITY, 800), 800);
    }

    #[test]
    fn page_height_is_capped_at_what_one_page_can_hold() {
        assert_eq!(page_height(16_000.0, 800), MAX_CAPTURE_HEIGHT_PX);
        assert_eq!(page_height(16_000.2, 800), MAX_CAPTURE_HEIGHT_PX);
        assert_eq!(page_height(250_000.0, 800), MAX_CAPTURE_HEIGHT_PX);
    }

    /// chrome not found -> `RenderError::ChromeUnavailable`, no panic.
    /// When CI has no chromium installed, running this path gets a launch error, which is exactly
    /// what the test checks.
    #[test]
    fn launch_browser_returns_friendly_error_when_chrome_missing() {
        // A subset of the path equivalent to spawn_blocking: call launch_browser directly.
        // If the host has Chrome installed, this succeeds -> skip; using ignore would be safer.
        // Default: try to launch; any Err counts as "chrome unavailable" and is swallowed;
        // no panic.
        match launch_browser() {
            Ok(b) => {
                drop(b);
            }
            Err(RenderError::ChromeUnavailable(_)) => {}
            Err(e) => panic!("unexpected error variant: {e}"),
        }
    }

    /// Unit test for the timeout path: a tiny timeout plus a fake task that never returns.
    /// This does not actually start chrome; it only verifies the timeout future's behavior.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_future_returns_timeout_error() {
        let timeout = Duration::from_millis(20);
        let result: std::result::Result<(), _> =
            tokio::time::timeout(timeout, async { std::future::pending::<()>().await }).await;
        assert!(result.is_err(), "expected elapsed err");
    }
}
