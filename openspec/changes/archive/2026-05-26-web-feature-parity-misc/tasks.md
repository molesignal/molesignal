## 0. 准备

- [x] 0.1 audit `web/src/api/` 已有 client，列出缺失：`searchJobs` / `serviceGraph` / `actions` 需新建；`alertHistory` / `alertInsights` 复用 `incidents.ts`（后端无 `/alerts/history`、`/alerts/insights` 端点，改用 incidents 派生）；`shortUrls` 不需要新建（页面直接 `window.location.replace('/s/<code>')` 借用浏览器 302）
- [x] 0.2 audit `web/src/data/ingestionVendors.ts`（或等价 fixture）：实际位于 `web/src/routes/Ingest/sources.ts`（1006 行，6 categories × N vendors，已含 code snippet + endpoint URL）
- [x] 0.3 确认 `web/src/api/license.ts` 已上线（backend-settings-endpoints 已交付）；`/actions` 页需要它读 feature gate

## 1. API clients（仅在 audit 显示缺失时新增）

- [x] 1.1 `web/src/api/searchJobs.ts`：`get(id)` 单 job 详情
- [x] 1.2 `web/src/api/serviceGraph.ts`：`get(fromIso, toIso)` topology snapshot（接 `/web/topology`）
- [~] 1.3 `web/src/api/alertHistory.ts` — 复用 `incidents.list()` 客户端过滤，不再新增独立 client
- [~] 1.4 `web/src/api/alertInsights.ts` — 同 1.3，从 incidents 客户端派生
- [x] 1.5 `web/src/api/actions.ts`：`list()` actions（OSS 不调用，落 license-gated 空态）
- [~] 1.6 `web/src/api/shortUrls.ts` — 用浏览器 302（`window.location.replace('/s/<code>')`），无需 JSON client
- [x] 1.7 `api/index.ts` 导出新增 clients（actions / searchJobs / serviceGraph）

## 2. Logs & Metrics 二级路由

- [x] 2.1 `web/src/routes/logs/Inspector.tsx`：读 `?id=` query，无 id → 空态；有 id → KvRow 渲染 job 详情
- [x] 2.2 `web/src/routes/metrics/PromqlBuilder.tsx`：metric / labels / function / range 表单 + Run 按钮 + 时间序列结果

## 3. Trace / Stream / Service-graph

- [x] 3.1 `web/src/routes/traces/Detail.tsx`：`/traces/:id` 渲染 span tree + 头部摘要（复用 TraceFlame primitive）
- [x] 3.2 `web/src/routes/traces/SessionDetail.tsx`：`/traces/session/:id` 列出 session 内所有 trace（用 `/query` SQL 在 `traces` 流上按 attributes['session.id'] 聚合）
- [x] 3.3 `web/src/routes/streams/Explore.tsx`：`/streams/:id` 渲染 stream metadata + `Query in Logs` 按钮（用 `streams.list()` web-search 返回的 summary，无单 stream endpoint）
- [x] 3.4 `web/src/routes/serviceGraph/ServiceGraph.tsx`：`/service-graph` 渲染 topology + node click 跳转 traces（复用现有 ServiceTopology + useTopology hook）

## 4. Dashboards 二级路由

- [x] 4.1 `web/src/routes/dashboards/Import.tsx`：粘贴 / 上传 JSON，client-side 校验 → POST `/dashboards/import/grafana`
- [x] 4.2 `web/src/routes/dashboards/NewPanel.tsx`：`/dashboards/:id/panels/new` 复用 DashboardEditor，save 后 nav 回 `/dashboards/:id`

## 5. Alerts 二级路由 + 子 nav

- [x] 5.1 `web/src/routes/alerts/History.tsx`：resolved/closed incidents 表（从 `incidents.list()` 客户端过滤）
- [x] 5.2 `web/src/routes/alerts/Insights.tsx`：KPI strip（total / MTTR / top rule / status breakdown），客户端聚合 incidents
- [x] 5.3 `web/src/routes/alerts/AlertsLayout.tsx`：导出 `AlertsSubNav` 组件，History / Insights 各自渲染（`/alerts` 主页保留自己的 TabBar，避免破坏现有 chrome）

## 6. Actions + Short URL + Ingestion

- [x] 6.1 `web/src/routes/actions/Actions.tsx`：`/actions` 读 `actions.list()`；license features 不含 `actions` 时渲染 license-gated 空态
- [x] 6.2 `web/src/routes/short/ShortUrlRedirect.tsx`：`window.location.replace('/s/<code>')` 借浏览器 302；找不到时回退 fallback
- [x] 6.3 `web/src/routes/Ingest/Ingest.tsx`：在现有 vendor 页 verify 区块添加「检查后端健康」按钮，GET `/healthz` 显示状态 + 延迟（原 `/ingest/_health` 端点不存在，改用 `/healthz`；vendor 页面与 endpoint URL / snippet 已由更早 change 落地）

## 7. Shell 接入

- [x] 7.1 `shell/Sidebar.tsx`：OBSERVE 组加 `Service graph`；DATA 组加 `Actions`
- [x] 7.2 `routes/index.tsx`：注册 13 条新路由（`/logs/inspector`, `/metrics/promql-builder`, `/traces/:id`, `/traces/session/:id`, `/streams/:id`, `/service-graph`, `/dashboards/import`, `/dashboards/:id/panels/new`, `/alerts/history`, `/alerts/insights`, `/actions`, `/short/:code`；ingestion 实页早已存在仅新增 Test event 按钮）
- [x] 7.3 `a11y-routes.spec.ts`：在 ROUTES 数组增加 11 条新路由（ShortUrlRedirect 因 `window.location.replace` 跳出 SPA，未列入 a11y 扫描）
- [~] 7.4 i18n：本轮文案直接落英文常量，避免开新 namespace；待用户提需求再补 `misc.json`

## 8. 文档 + 校验

- [x] 8.1 `docs/web/sitemap-diff.md` P2 表：将本 change 落地行加 Status ✓ 列、backend 列翻为 🔌
- [x] 8.2 `pnpm -C web typecheck` 0
- [x] 8.3 `pnpm -C web lint` 0
- [x] 8.4 `pnpm -C web test:run` 不退化（47/49 通过，keyboard controller 2 个 pre-existing failures 与本 change 无关）
- [x] 8.5 `pnpm -C web a11y:contrast` 仍 93 pass
- [ ] 8.6 `pnpm -C web playwright a11y-routes.spec.ts` 11 条新路由 axe critical=0（CI 验证；本地未启动 browser fixture）
- [x] 8.7 `openspec validate web-feature-parity-misc --type change --strict` 通过

## 9. Follow-up（不在本 change 范围）

- [ ] 9.1 `/alerts/import-semantic-groups` 待后端先开 `/alerts/import_semantic_groups` 端点
- [ ] 9.2 `/alerts/anomaly/{add,edit/:id}` 待 anomaly-detection 后端再补 web
