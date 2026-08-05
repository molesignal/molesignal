## Context

`ScheduledReportRunner` MVP 在 spec M1 上线，已经按 cron 跑 + deliver webhook / s3 + email placeholder。format 字段支持 json / csv / svg / png / pdf；前三个直接生成，后两个 fallback 到 SVG。这一档差最后一公里：真把 dashboard 截成 PNG / PDF。

## Goals / Non-Goals

**Goals:**
- enterprise build 启用 renderer 后 PNG / PDF 真出图
- 单 process 多 report 时 Chrome 实例复用，不每次起新进程
- 失败可观测（timeout / crash 都落 `report_deliveries.error`）

**Non-Goals:**
- 不内置 Chrome（用户/ops 自己装 chromium，Dockerfile 给出参考）
- 不实装 chart-server 备选（重复造轮子）
- 不实装报告分享链接（短链已有 capability，前端拼）
- 不实装多页 PDF 自定义模板（v1 一份报告一张图）

## Decisions

### D1：用 `headless_chrome` crate 而不是自己驱动 CDP

`headless_chrome` 已经封了 Chrome DevTools Protocol + 进程管理 + 等待 dom 加载等常见操作；自己驱 CDP 是个月级别的工作。

### D2：内部用 `http://127.0.0.1:<api_port>/...embed?session=...`

Chrome 起在同 host 上访问自己 API，避免外网 / SSL 问题；`session` token 是 short-lived（5 min）单次使用的 JWT，复用现有 IdentityService::issue_token 接口签发。这样 renderer 内调用走真实 auth path，dashboard 数据隔离与 HTTP 路径一致。

### D3：Pool size 默认 2，硬上限 4

每个 Chrome 实例 ~200MB RAM；2 个 = 400MB，与 search_jobs worker 资源占用同档。硬上限 4 防止 ops 写飞配置。

### D4：embed 路径假设由前端提供

本 change 只关心 backend 能跑 chrome；前端要做 `/dashboards/:id/embed` 路由（去掉 sidebar / topbar / 仅 panel grid + 给 `?session=` 注入 auth）。这是单独的 frontend change。MVP fallback：截整个 dashboard 页（带 chrome），用户能看到数据。

### D5：renderer 是 trait + cfg-gate

不强制 OSS build 链 chromium。`Option<Arc<dyn ReportRenderer>>` 在 AppState 中，OSS = None；enterprise + config 开 = `Some(HeadlessChromeRenderer)`。

## Risks / Trade-offs

**[R1] Chrome 进程 OOM 杀死整个 pod**
→ Mitigation：每实例 `--max-old-space-size=512` + Docker 加 cgroup memory limit；render 失败 = single delivery fail，不是 service down。

**[R2] embed 页面不存在**
→ Mitigation：MVP 截整页 fallback；前端 follow-up 单独 PR 加 embed 路由。文档明示限制。

**[R3] Chrome version skew**
→ Mitigation：`headless_chrome` 支持任意 Chrome / Chromium 1xx+；Dockerfile pin chromium 版本号便于 reproducible。

**[R4] session token leak via render**
→ Mitigation：token TTL=300s + single-use（spawn 时签发，渲染完即 revoke）；URL 仅在本机 loopback 上访问，不出 host。

**[R5] PNG / PDF 文件大占满 object_store**
→ Mitigation：deliver=s3 时已写 `<prefix>/<report-id>-<ts>.<ext>`；后续 deliver 失败 sweep 清旧文件留 follow-up。
