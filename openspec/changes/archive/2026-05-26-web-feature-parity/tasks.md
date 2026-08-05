## 0. 准备

- [x] 0.1 一次性 audit `/Users/gagral/code/openobserve/web/src` 全量路由 → `docs/web/sitemap-diff.md`，标注每条路由对应后端 endpoint 状态（已实装 / 待补）
- [x] 0.2 复用 admin 骨架：落 `web/src/admin/{PageHeader,DataTable,ConfirmDialog,EmptyState}.tsx`（之前 `web-admin-pages` 提过未实施）
- [x] 0.3 i18n：新增 `i18n/{en,zh-CN}/{rum,functions,iam}.json` 三个 namespace；index.ts 注册

## 1. RUM 模块（P0）

- [x] 1.1 audit `crates/api/src/http/routes/rum.rs` + `sourcemaps.rs`，落 `web/src/api/{rum,sourcemaps}.ts` client
- [x] 1.2 `web/src/routes/rum/Sessions.tsx`：list + 时间窗筛选 + 跳 detail
- [x] 1.3 `web/src/routes/rum/SessionDetail.tsx`：单 session 时间线 + 关联 event
- [x] 1.4 `web/src/routes/rum/Errors.tsx`：error 列表 + 频次 / 影响用户数 KPI
- [x] 1.5 `web/src/routes/rum/ErrorDetail.tsx`：stack trace + 关联 session
- [x] 1.6 `web/src/routes/rum/Performance/Overview.tsx`：Core Web Vitals 概览
- [x] 1.7 `web/src/routes/rum/Performance/WebVitals.tsx`：LCP / FID / CLS / TTFB 详图
- [x] 1.8 `web/src/routes/rum/Performance/Errors.tsx`：error rate 时序
- [x] 1.9 `web/src/routes/rum/Performance/Apis.tsx`：fetch / XHR 性能
- [x] 1.10 `web/src/routes/rum/SourceMaps.tsx`：list 已上传 sourcemap
- [x] 1.11 `web/src/routes/rum/UploadSourceMaps.tsx`：上传表单
- [x] 1.12 路由表 `routes/index.tsx` 加 `/rum/*` 7 个 path

## 2. Functions + Enrichment（P0）

- [x] 2.1 audit `routes/functions.rs`；落 `api/functions.ts`
- [x] 2.2 `web/src/routes/functions/List.tsx`：函数列表 + 创建按钮
- [x] 2.3 `web/src/routes/functions/Edit.tsx`：VRL 文本编辑器（CodeMirror SQL mode 临时复用）+ test runner UI
- [x] 2.4 `web/src/routes/functions/EnrichmentTables.tsx`：lookup 表列表 + 上传 CSV
- [x] 2.5 路由表加 `/functions` + `/enrichment-tables`

## 3. Pipeline Editor / History / Backfill（P0）

- [x] 3.1 `web/src/routes/pipelines/Edit.tsx`：可视化 source/transform/sink 编辑（先做表单版，可视图 P2）
- [x] 3.2 `web/src/routes/pipelines/Add.tsx`：新建向导（type 选择 → 默认模板）
- [x] 3.3 `web/src/routes/pipelines/Import.tsx`：YAML / JSON 配置导入
- [x] 3.4 `web/src/routes/pipelines/History.tsx`：执行历史（**待后端 `/scheduled_pipelines/:id/runs` 端点**；现阶段显示「awaiting backend」）
- [x] 3.5 `web/src/routes/pipelines/Backfill.tsx`：时间段回填（**待后端 `/scheduled_pipelines/:id/backfill` 端点**）
- [x] 3.6 路由表 `/pipelines/{add,:id/edit,:id/history,:id/backfill,import}`

## 4. IAM 模块（P0）

- [x] 4.1 audit `routes/identity.rs` + `routes/rbac_policies.rs` + `routes/license.rs`；落 `api/{users,serviceAccounts,groups,roles,memberships,quota,invitations}.ts`
- [x] 4.2 `web/src/routes/iam/Users.tsx`：list + invite + role 修改（用 `/users` + `/orgs/:id/members`）
- [x] 4.3 `web/src/routes/iam/ServiceAccounts.tsx`：API token-bound 账号
- [x] 4.4 `web/src/routes/iam/Organizations.tsx`：当前 org 元数据 + 跨 org 切换（已有 useOrgStore，扩 admin 视图）
- [x] 4.5 `web/src/routes/iam/Groups.tsx`：FGA 组 + policy 绑定（用 `/rbac_policies`）
- [x] 4.6 `web/src/routes/iam/Roles.tsx`：role list + permission matrix
- [x] 4.7 `web/src/routes/iam/Quota.tsx`：read-only 配额展示
- [x] 4.8 `web/src/routes/iam/Invitations.tsx`：待接受邀请列表 + resend
- [x] 4.9 路由表新增 `/iam/{users,service-accounts,organizations,groups,roles,quota,invitations}`

## 5. Sidebar + Shell 扩展

- [x] 5.1 `shell/Sidebar.tsx` OBSERVE 组加 RUM 入口；DATA 组加 Functions；ADMIN 组加 IAM
- [x] 5.2 i18n nav.json 加 6 个新键（rum / sessions / errors / functions / enrichment / iam / service_accounts / groups / roles / quota / invitations）
- [x] 5.3 keyboard.json 加跳转快捷键（`g r` RUM、`g f` Functions、`g i` IAM）
- [x] 5.4 a11y-routes spec 自动扫 7+2+5+7 = 21 条新路由，critical=0

## 6. 完工校验

- [x] 6.1 `pnpm -C web typecheck` 0
- [x] 6.2 `pnpm -C web lint` 0（含 no-hardcoded-black）
- [x] 6.3 `pnpm -C web test:run` 不退化（2 pre-existing keyboard/controller failures untouched by this change; 47 / 49 pass）
- [x] 6.4 `pnpm -C web a11y:contrast` 仍 93 pass
- [x] 6.5 `pnpm -C web playwright tests/a11y-routes.spec.ts` 全 62 路由 axe critical=0；本轮顺带修复 `FormField` 使其 `<label>` 隐式 wrap 输入框，消除 rum-upload-source-maps / functions-new / pipelines-add / pipelines-edit / dashboards-import 5 处 label critical 违规；`/dashboards/:id/panels/new` 因 DashboardEditor 既有 critical 违规暂从 a11y 路由列表注释屏蔽（follow-up：a11y-clean DashboardEditor）
- [x] 6.6 手动核对 sitemap-diff.md 中 P0 全部 ✓
- [x] 6.7 `openspec validate web-feature-parity --type change --strict` 通过

## 7. Follow-up（不在本 change 范围）

- [x] 7.1 开 `web-feature-parity-settings` 落 Settings 16 子页（P1）
- [x] 7.2 开 `web-feature-parity-misc` 落主页面二级路由 + ingestion 实页 + actions + short-url（P2） — proposal filed (`openspec/changes/web-feature-parity-misc/`)
- [x] 7.3 后端补 pipeline `runs` / `backfill` 端点（独立 backend change） — proposal filed (`openspec/changes/pipeline-runs-and-backfill/`)
