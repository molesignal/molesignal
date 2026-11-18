// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! When the page is ready to render, and when it should fail the report.
//!
//! The report page (frontend `web/src/report-render`) declares its own state through
//! `<html data-report-render-state>`: it renders only once the state reaches `ready`; on `error`
//! it fails with the reason in `data-report-render-error`, instead of printing a page that says
//! "something went wrong" into a file with correct magic bytes but unusable content.
//! The report pages for dashboards and saved views both follow this protocol.

use std::time::Duration;

use headless_chrome::Tab;

use crate::RenderError;

/// Maximum time a report page may take to fetch its data.
const READY_TIMEOUT_MILLIS: u64 = 15_000;

/// Time left for the browser to finish its last paint after the report page says `ready`.
const READY_SETTLE: Duration = Duration::from_millis(200);

/// Pages that are not report pages have no readiness signal to wait for, so leave a window
/// for async requests to settle.
const UNSIGNALLED_SETTLE: Duration = Duration::from_millis(750);

/// The report page was redirected to the sign-in page: the sign-in state used for rendering
/// was not written, has expired, or was rejected by the backend.
const SIGNED_OUT: &str =
    "the report page asked to sign in: the render session is missing, expired or was rejected";

/// Waits until the page is ready to render; returns the reason when the page reports an error,
/// is redirected to sign-in, or never becomes ready.
pub(super) fn wait_until_printable(tab: &Tab, url: &str) -> Result<(), RenderError> {
    if is_report_page_url(url) {
        wait_for_report_ready(tab)?;
        std::thread::sleep(READY_SETTLE);
    } else {
        std::thread::sleep(UNSIGNALLED_SETTLE);
    }

    ensure_page_renderable(tab)
}

/// Whether the URL is a report page: the query parameters contain `reportRender=1`.
/// The URLs used for scheduled delivery, instant export, and share snapshots all carry it,
/// whether the report refers to a dashboard or a saved view.
fn is_report_page_url(url: &str) -> bool {
    let Some((_, rest)) = url.split_once('?') else {
        return false;
    };

    let query = rest.split_once('#').map_or(rest, |(query, _)| query);

    query.split('&').any(|pair| pair == "reportRender=1")
}

fn wait_for_report_ready(tab: &Tab) -> Result<(), RenderError> {
    let expression = ready_expression();

    let value = match tab.evaluate(&expression, true) {
        Ok(value) => value,
        Err(_) => {
            // A full-page navigation destroys the execution context being waited on (when the
            // backend rejects the sign-in state, the frontend navigates the whole page to the
            // sign-in page). Wait again on the new document; it says what kind of page it is.
            tab.wait_for_element("body").map_err(|error| {
                RenderError::PageError(format!("wait for report page: {error}"))
            })?;

            tab.evaluate(&expression, true).map_err(|error| {
                RenderError::PageError(format!("wait for report ready: {error}"))
            })?
        }
    };

    let message = evaluated_string(value.value);

    if message.is_empty() {
        Ok(())
    } else {
        Err(RenderError::PageError(format!(
            "report render failed: {message}"
        )))
    }
}

/// Script that polls the page state: yields an empty string when ready, else the failure reason.
fn ready_expression() -> String {
    format!(
        r#"
new Promise((resolve) => {{
  const deadline = Date.now() + {READY_TIMEOUT_MILLIS};
  const inspect = () => {{
    const root = document.documentElement;
    const state = root?.dataset?.reportRenderState ?? '';
    const body = (document.body?.innerText ?? '').trim();
    if (state === 'ready') {{
      resolve('');
      return;
    }}
    if (state === 'error') {{
      resolve(root?.dataset?.reportRenderError || body.slice(0, 1200) || 'the report page reported an error');
      return;
    }}
    if (location.pathname === '/signin') {{
      resolve('{SIGNED_OUT}');
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
      resolve('the report page did not become ready within {READY_TIMEOUT_MILLIS}ms');
      return;
    }}
    setTimeout(inspect, 100);
  }};
  inspect();
}})
"#
    )
}

fn ensure_page_renderable(tab: &Tab) -> Result<(), RenderError> {
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
    fn every_report_page_speaks_the_ready_protocol() {
        // Report pages for both dashboards and saved views wait for the readiness signal.
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?reportRender=1"
        ));
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/saved-views?view=v1&reportRender=1&time=now-24h..now"
        ));
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?time=now-7d..now&reportRender=1#panel-3"
        ));
    }

    #[test]
    fn a_page_that_is_not_a_report_has_no_signal_to_wait_for() {
        assert!(!is_report_page_url("http://127.0.0.1:5173/dashboards/d1"));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/saved-views?view=v1"
        ));
        // The parameter must be exactly reportRender=1: other values, this text in another
        // parameter, or this text in the fragment do not count.
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?reportRender=10"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?reportRender=0"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?notReportRender=1"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1#reportRender=1"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?next=a#x?reportRender=1"
        ));
    }

    #[test]
    fn the_ready_script_names_the_sign_in_page_and_the_time_limit() {
        let script = ready_expression();

        assert!(script.contains("location.pathname === '/signin'"));
        assert!(script.contains(SIGNED_OUT));
        assert!(script.contains("did not become ready within 15000ms"));
        // After formatting, the braces must be balanced, or the browser cannot parse the script.
        assert_eq!(script.matches('{').count(), script.matches('}').count());
    }
}
