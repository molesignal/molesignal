// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 页面什么时候可以出图，什么时候应当让报表失败。
//!
//! 报表页（前端 `web/src/report-render`）通过 `<html data-report-render-state>` 说明自己的
//! 状态：到 `ready` 才出图；到 `error` 就带着 `data-report-render-error` 里的原因失败，而不是
//! 把一张写着「出错了」的页面打印成 magic bytes 正确、内容却不可用的文件。仪表盘和已保存
//! 视图的报表页都遵守这个协议。

use std::time::Duration;

use headless_chrome::Tab;

use crate::RenderError;

/// 报表页最多用这么久取数。
const READY_TIMEOUT_MILLIS: u64 = 15_000;

/// 报表页说 `ready` 之后，留给浏览器完成最后一次绘制的时间。
const READY_SETTLE: Duration = Duration::from_millis(200);

/// 不是报表页的页面没有就绪信号可等，留一个异步请求稳定下来的窗口。
const UNSIGNALLED_SETTLE: Duration = Duration::from_millis(750);

/// 报表页被带到登录页：渲染用的登录态没有写入、已经过期，或者被后端拒绝。
const SIGNED_OUT: &str =
    "the report page asked to sign in: the render session is missing, expired or was rejected";

/// 等到页面可以出图；页面说明自己出错、被带去登录或一直没有就绪时返回原因。
pub(super) fn wait_until_printable(tab: &Tab, url: &str) -> Result<(), RenderError> {
    if is_report_page_url(url) {
        wait_for_report_ready(tab)?;
        std::thread::sleep(READY_SETTLE);
    } else {
        std::thread::sleep(UNSIGNALLED_SETTLE);
    }

    ensure_page_renderable(tab)
}

/// 地址是否是报表页：查询参数里有 `report_render=1`。定时投递、即时导出和分享快照用的地址
/// 都带着它，无论报表引用的是仪表盘还是已保存视图。
fn is_report_page_url(url: &str) -> bool {
    let Some((_, rest)) = url.split_once('?') else {
        return false;
    };

    let query = rest.split_once('#').map_or(rest, |(query, _)| query);

    query.split('&').any(|pair| pair == "report_render=1")
}

fn wait_for_report_ready(tab: &Tab) -> Result<(), RenderError> {
    let expression = ready_expression();

    let value = match tab.evaluate(&expression, true) {
        Ok(value) => value,
        Err(_) => {
            // 整页跳转会销毁正在等待的执行上下文（登录态被后端拒绝时，前端整页跳去登录页）。
            // 在新文档上再等一次，由它说明自己是什么页面。
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

/// 轮询页面状态的脚本：就绪时得到空串，否则得到失败原因。
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
        // 仪表盘和已保存视图的报表页都要等就绪信号。
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?report_render=1"
        ));
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/saved-views?view=v1&report_render=1&time=now-24h..now"
        ));
        assert!(is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?time=now-7d..now&report_render=1#panel-3"
        ));
    }

    #[test]
    fn a_page_that_is_not_a_report_has_no_signal_to_wait_for() {
        assert!(!is_report_page_url("http://127.0.0.1:5173/dashboards/d1"));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/saved-views?view=v1"
        ));
        // 参数要完整等于 report_render=1：别的取值、别的参数里出现这段文字、片段里出现都不算。
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?report_render=10"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?report_render=0"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?not_report_render=1"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1#report_render=1"
        ));
        assert!(!is_report_page_url(
            "http://127.0.0.1:5173/dashboards/d1?next=a#x?report_render=1"
        ));
    }

    #[test]
    fn the_ready_script_names_the_sign_in_page_and_the_time_limit() {
        let script = ready_expression();

        assert!(script.contains("location.pathname === '/signin'"));
        assert!(script.contains(SIGNED_OUT));
        assert!(script.contains("did not become ready within 15000ms"));
        // 格式化之后花括号要成对，否则浏览器解析不了这段脚本。
        assert_eq!(script.matches('{').count(), script.matches('}').count());
    }
}
