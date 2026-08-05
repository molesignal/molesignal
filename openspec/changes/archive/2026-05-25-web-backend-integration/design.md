## Context

`web/` 早期开发为了不被后端节奏卡住，在 axios 客户端 + react-query 之上跑了一套基于 `playwright/fixtures/mockBackend` 的确定性 mock。这套 mock 也被 `vite dev server` 间接依赖：vite proxy 把 `/api` 转到 `localhost:5080`（Rust backend），但本地没启 backend 时，前端任何调用都 502。Login 表单还残留早期"用户选 workspace"的 UX，但后端 `crates/api/src/http/routes/auth.rs` 早就改成 JWT 含 org_id；前端选的 workspace 字段被服务端忽略。

backend 已落的 `/api/v1/orgs` 端点提供 org 列表 + 当前 org 元数据，但前端没接，导致用户跨 org 时只能改 URL / 重新登录。

## Goals / Non-Goals

**Goals:**
- web/src 全部 axios 调用对齐 `crates/api/src/http/routes/*` 真实端点（path / method / params）
- Login 简化到 email + password（+ "Continue offline"），删除 workspace 字段
- 提供 org switcher UI + zustand store，切换时正确清缓存 / 重置 stack
- 401 处理唯一入口，回 `/login?next=` 不丢上下文
- `pnpm dev` 在有 backend 时直连，无 backend 时通过 `pnpm dev:mock` 用 mockBackend 跑

**Non-Goals:**
- 不重写 axios → fetch / RTK-Query（保持 react-query + axios 不变）
- 不做 org RBAC（按 role 隐藏菜单留给 `web-admin-pages`）
- 不重做多端 SSO / 第三方登录 UI（用现有 Login 即可）
- 不动 e2e / visual / a11y baseline 套件（仍跑 mockBackend）

## Decisions

### D1：所有 11 个 `api/*.ts` 文件做一次"端点真实性 audit"

每个 `http.get/post/put/delete` 路径都跟 `routes/<feature>.rs` 的 `actix-web` 注解逐条对照。fix mismatch；删除任何 `// TODO: backend` 注释 + 假数据。结果落 `tasks.md` 一一勾。

### D2：org switcher 由独立 store + 独立 hook 提供

新增 `web/src/stores/useOrgStore.ts`：
- `orgs: Org[]`
- `currentOrgId: string | null`（与 `useAuthStore.ctx.org_id` 保持同步）
- `loadOrgs(): Promise<void>`（hit `/api/v1/orgs`）
- `switchOrg(id): Promise<void>`：调 `/api/v1/orgs/{id}/select` → 拿到新 JWT → `useAuthStore.setSession` → `queryClient.clear()` → `useInvestigationStack.reset()` → `nav('/home')`

StatusStrip 的 org 文案变 dropdown trigger；item 列表来自 `useOrgStore`。

### D3：401 拦截器统一规则

`lib/http.ts` 现有的 401 处理已经在做 "logout + nav to /login"；本 change 把切 org 失败也走同一通路（switch 失败 → 401 → 回 login）。`?next=` 始终带当前 pathname + search，确保用户回登录后能回到原页。

### D4：Workspace 字段移除是 FE-only BREAKING

后端从未真正读 Workspace 字段；用户 UI 上选了也没用。删了后无 API 兼容性问题。但 e2e 测试 / 文档 / 视觉 baseline 都可能引用，需要一并 update（00-smoke 测 click "Continue offline" 不动；visual baseline 因为 login 长得不一样要 rebase）。

### D5：dev 启动两条路径

- `pnpm dev`：vite + proxy `/api` → `localhost:5080`；假定本地 backend 已启动（开发者自己负责）
- `pnpm dev:mock`：起 mockBackend express + vite，proxy `/api` → mockBackend 端口；用于 backend down 或纯前端调试

后者新增脚本 `scripts/dev-mock.ts`：拆 mockBackend `registerRoutes` 出来，独立用作 dev server backend。

## Risks / Trade-offs

**[R1] backend 路径与前端 audit 出来的 mismatch 一次过修可能漏**
→ Mitigation：tasks.md 把每个 `api/*.ts` 文件单独立 task，apply 时逐个对照 `crates/api/src/http/routes/<file>.rs`；CI 加一个 axios mock interceptor 在 vitest 里跑通"假调用一次每个 endpoint"的烟雾测试

**[R2] org 切换时 react-query cache.clear() 会闪空状态**
→ Mitigation：UI 在 switch 期间显示 toast `Switching org…` + loading state；切换完成 nav 到 /home 让组件自然重 fetch

**[R3] mockBackend 重构成 dev backend 后被 e2e + dev 双引用，diverge 风险**
→ Mitigation：mockBackend 仍是单一 source；e2e + dev 都 import `registerRoutes`；dev 模式只多加一段 cors header；vitest 加合约测试确保两路径行为一致

**[R4] 删 workspace 字段会让既有用户的 localStorage 残留无意义键**
→ Mitigation：`ThemeBootstrap` 启动时清理已知陈旧键（`molesignal-workspace`）。
