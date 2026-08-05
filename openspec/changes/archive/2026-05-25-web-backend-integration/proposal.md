## Why

前 4 个前端 change（typescript-strict-fixes → playwright-runtime → a11y-baseline → ui-polish）落地了基础设施 + 视觉 polish，但 web 仍跑在 `playwright/fixtures/mockBackend` 那套确定性 mock 上 —— `pnpm dev` 启动后所有 `/api/v1/*` 调用要么打到 vite proxy 的 `localhost:5080`（dev backend 未必常驻），要么走假数据。Login 页还残留早期 multi-tenant 草稿的 `Workspace` 选择框（实际上 org 由 JWT/cookie 决定，前端选 workspace 没有用）；status strip 也没有"切组织"入口，跟后端 `/api/v1/orgs` 已经返回 org 列表的能力对不齐。

本 change 是产品化第一步：让 web/src 不再依赖任何 mock 才能跑通真实用户路径（登录 → 列出 orgs → 在 org 间切换 → 跑查询 → 看 dashboards / alerts / incidents 等），并把已实装但前端没接入的 `/api/v1/*` 端点全部接通。

## What Changes

- **wire 所有 web/src/api 模块到真实后端**：alerts / channels / dashboards / escalations / incidents / ingestion / query / schedules / web 等 11 个文件，确认 axios 客户端接到 `/api/v1/*`，path / params 跟 `crates/api/src/http/routes/*` 对齐；删除任何 placeholder 数据 / fallback 假数据
- **新增 `/api/v1/orgs` 客户端 + zustand `useOrgStore`**：listing / current / switch；与 `useAuthStore` 联动（切 org 时刷新 JWT 中的 org_id claim、清空 react-query cache、reset investigation stack）
- **StatusStrip 加 org switcher dropdown**：左侧 org_id 文案变成可点的 trigger；下拉显示 `useOrgStore.list` + 当前 org 高亮 + Esc 关闭；切换后导航回 `/home`
- **Login 页删除 Workspace 字段** **BREAKING**（FE-only）：表单只保留 email + password；offline-dev 流仍可用；`useAuthStore.setSession` 不再接 workspace 参数
- **api 401 处理统一**：`lib/http.ts` 拦截器在收到 401 时清 auth state + 回登录页，并把当前 URL 当 `?next=` 带走；切 org 失败、token 过期都走同一路径
- **mockBackend fixture 范围收窄到测试**：mockBackend 不再被 `pnpm dev` 引用（vite 的 `proxy` 配置文档化），保持 e2e/visual 套件继续使用 mockBackend；新增 `pnpm dev:mock` 脚本在没有 dev backend 时启 mockBackend 作为 dev API

## Capabilities

### New Capabilities

- `web-org-switching`: org 列表 / 当前 org / 切换 org 的客户端状态机 + UI 入口；与 auth / 缓存清理 / 投资栈 reset 的联动规则

### Modified Capabilities

- `web-shell`: Login 表单字段集（去掉 Workspace）；StatusStrip 新增 org switcher trigger 在 org_id 文案位置；401 统一回 `/login?next=...`
- `web-shell-crud`: 各 CRUD 调用端点路径与 `crates/api/src/http/routes/*` 真实路径对齐（消除前端假定路径）

## Impact

- **代码**：`web/src/api/*.ts`（11 个）、`web/src/lib/http.ts`、`web/src/routes/Login.tsx`、`web/src/shell/StatusStrip.tsx`、`web/src/stores/{auth,useOrgStore}.ts`、`web/src/routes/index.tsx`（加 `/orgs` route）；新增 `web/src/api/orgs.ts`
- **dev 体验**：`pnpm dev` 默认假定本地 backend 在 5080 端口（已有 vite proxy）；如无可用 backend，用 `pnpm dev:mock` 启 mockBackend 作 dev API（新加脚本 + 小 express 入口复用 playwright fixture）
- **测试**：e2e + visual + a11y baseline 不动 —— 都已通过 mountMockRoutes 隔离；新增 vitest 覆盖 useOrgStore 的 switch 时序（清缓存 + reset stack）
- **风险**：BREAKING 仅在 FE — Login 表单变了，使用 offline-dev 流的人不受影响；线上用户首次升级会丢一个不再读取的 workspace 选项；后端 API 路径若 mismatch 会在 dev 启动后即时暴露
- **跟随**：land 后 `web-theming-i18n` 才能在真实 org_id-aware 上下文里加多语言；`web-admin-pages` 需要 useOrgStore 来知道在哪个 org 范围下管理 teams/groups/license
