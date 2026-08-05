## Why

ScheduledReportRunner 已在跑（spec M1），但 `render()` 只能产 JSON / CSV / 极简 SVG。用户最想要的 PDF / PNG 报告（"周一早 9 点把昨天 dashboard 截图发邮件"）当前会退化到 JSON，体验残废。OO 是用 headless Chrome 截图，我们做同样的选择（避免发明新的图表 server）。

## What Changes

- enterprise crate 新建 `enterprise/crates/report-renderer/`，独立 cfg-gate（feature `report-renderer` 默认关闭）。
- 内部用 `headless_chrome` crate 起 Chrome instance 池（`max_concurrent_renders=2`）。
- 给 `ReportRenderer` trait + `HeadlessChromeRenderer` impl：输入 `dashboard_url + session_token + viewport`，输出 PDF bytes / PNG bytes。
- `ScheduledReportRunner::render` 在 `format=png|pdf` 时调 `state.report_renderer.render(...)`；无 renderer 时 fallback 到现有 SVG placeholder + warn 一次。
- 内部 dashboard URL 构造：`http://localhost:<api_port>/dashboards/<id>/embed?session=<token>` —— 后续前端要补一个 `/embed` 模式让 Chrome 头像看到无 chrome（没有 sidebar / topbar，仅 panel grid）。
- Chrome 启动失败 / timeout / 空白页 → 录 `report_deliveries.error` + status=failed，不阻塞下一份报告。
- 资源限制：每次 render 30s wall clock + 1GB memory；超时杀进程。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `scheduled-reports`: render 引擎从 SVG-only 升级到 PDF/PNG via headless Chrome。

## Impact

- **新 crate**：`enterprise/crates/report-renderer/`（独立 cfg gate）；deps：`headless_chrome = "1"`、`tokio = "1"`、`bytes`。运行时要求 host 装 Chrome 或 Chromium（Dockerfile 加一行 `RUN apk add chromium`）。
- **AppState**：可选 `report_renderer: Option<Arc<dyn ReportRenderer>>`。
- **wire**：cfg=enterprise + `[scheduled_reports.renderer.enabled]=true` → 构造 HeadlessChromeRenderer；否则 None。
- **Web**：前端需要新增 dashboard embed 模式（属于另一个 frontend change，不在此 backend change 范围）；本 change 假设 URL `/dashboards/<id>/embed` 存在并能渲染。MVP fallback：若 embed 页面不存在，截整个 dashboard 页（带 chrome），用户能看到数据即可。
- **Docker / k8s**：`deploy/docker/Dockerfile` enterprise build 阶段加 chromium 安装；k8s `60-compactor.yaml` enterprise 模式 image 同步。
- **测试**：单测覆盖 `HeadlessChromeRenderer` 失败路径（chrome 找不到 / timeout）+ render result mock；integration test 留 follow-up（要 docker chrome）。
