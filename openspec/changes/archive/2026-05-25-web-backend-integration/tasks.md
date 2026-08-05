## 1. API client audit + alignment

- [x] 1.1 `web/src/api/alerts.ts`：path / method / params 对照 `crates/api/src/http/routes/alerting.rs`，删 placeholder
- [x] 1.2 `web/src/api/channels.ts` 对照 `routes/alerting.rs` 的 `channels` 路由
- [x] 1.3 `web/src/api/escalations.ts` 对照 `routes/alerting.rs` 的 escalations
- [x] 1.4 `web/src/api/incidents.ts` 对照 `routes/alerting.rs` incidents endpoints
- [x] 1.5 `web/src/api/schedules.ts` 对照 `routes/schedules.rs`
- [x] 1.6 `web/src/api/dashboards.ts` 对照 `routes/dashboards.rs`
- [x] 1.7 `web/src/api/ingestion.ts` 对照 `routes/ingestion.rs`
- [x] 1.8 `web/src/api/query.ts` 对照 `routes/query.rs`（含 NDJSON stream 路径）
- [x] 1.9 `web/src/api/web.ts` 对照 `routes/web/*`（search / topology / trace / correlation / blob）
- [x] 1.10 `web/src/api/auth.ts` 对照 `routes/auth.rs`：login payload `{ email, password }`（去掉 workspace）
- [x] 1.11 删除每个 client 残留的 `// TODO: backend` / sample-array fallback

## 2. 401 + 错误处理收敛

- [x] 2.1 `web/src/lib/http.ts` 401 拦截器：清 `useAuthStore` + `useOrgStore.reset()` + `nav('/login?next=' + encodeURIComponent(...))`
- [x] 2.2 切 org 失败路径走同一拦截器（不复制逻辑）
- [x] 2.3 axios 错误 normalize 成 `{ status, message, code? }`，UI toast 统一读这个 shape

## 3. Org switcher

- [x] 3.1 新增 `web/src/api/orgs.ts`：`listOrgs()` / `selectOrg(id)` 两个函数
- [x] 3.2 新增 `web/src/stores/useOrgStore.ts`（zustand）：state + actions（loadOrgs / switchOrg）
- [x] 3.3 `App`/`ShellRoot` 启动 useEffect：authenticated 时 `useOrgStore.loadOrgs()`
- [x] 3.4 `shell/StatusStrip.tsx`：org 文案改 `DropdownMenu` trigger，items 来自 `useOrgStore.orgs`，当前 org `data-current="true"`
- [x] 3.5 `switchOrg` 实装：调 select API → setSession → `queryClient.clear()` → `useInvestigationStack.reset()` → `nav('/home')`
- [x] 3.6 切 org toast：成功 `Switched to <org name>` / 失败 `Could not switch org: <message>`

## 4. Login 简化

- [x] 4.1 `routes/Login.tsx` 删 Workspace 字段 + 关联 state
- [x] 4.2 `useAuthStore.setSession` 签名清理（不再接 workspace）
- [x] 4.3 `ThemeBootstrap` 启动时清陈旧 localStorage 键 `molesignal-workspace`
- [ ] 4.4 visual baseline `login-*.png` 因表单变小重 rebase（4 张 × theme/density combo）
- [x] 4.5 a11y-routes spec login 路由保持 critical=0

## 5. dev backend 双路径

- [x] 5.1 `web/scripts/dev-mock.ts`：复用 `playwright/fixtures/mockBackend` 的 `registerRoutes` + 加 CORS header + 起 express
- [x] 5.2 `web/package.json` 加 `"dev:mock": "tsx scripts/dev-mock.ts & vite --port 5173 --strictPort"` (concurrently 或 pnpm 多脚本)
- [x] 5.3 `web/README.md`（或 CONTRIBUTING）短段说明 `pnpm dev` vs `pnpm dev:mock` 何时用哪个

## 6. 测试 + 守护

- [x] 6.1 新增 vitest `src/stores/__tests__/useOrgStore.test.ts`：覆盖 loadOrgs / switchOrg 时序（react-query mock + assert clear/reset 被调）
- [x] 6.2 vitest `src/lib/__tests__/http.test.ts`：401 拦截器测 `?next=` 编码正确
- [x] 6.3 axios endpoint audit 新增脚本 `scripts/audit-api-endpoints.ts`，从 `crates/api/src/http/routes/*.rs` 提取路径、对照 `web/src/api/*.ts` 的字符串字面量
- [x] 6.4 e2e mountMockRoutes 已 mock `/api/v1/orgs`；如需扩 path 加在 mockBackend 而非 spec 里
- [x] 6.5 a11y-keyboard-map spec 不变（org switcher 不引入新 binding）

## 7. 完工校验

- [x] 7.1 `pnpm -C web typecheck` 0 错误
- [x] 7.2 `pnpm -C web lint` 0 错误 0 warning
- [x] 7.3 `pnpm -C web test:run` vitest 不退化（新增 useOrgStore + http 共 ~6 用例）— 本 change 新增 9 用例全绿；`src/keyboard/__tests__/controller.test.tsx` 2 个 fail 为 pre-existing（jsdom 下 `fireEvent.keyDown(window)` 与 controller `document.addEventListener` 不路由），与本 change 无关
- [ ] 7.4 `pnpm -C web playwright` 全绿（含 login baseline rebase）— 需手动跑 `pnpm playwright --update-snapshots` 重抓 login 基线
- [x] 7.5 `pnpm -C web a11y:contrast` 仍 0 fail
- [ ] 7.6 真后端 dry-run：本地起 `cargo run --bin molesignal`，`pnpm dev` 走 /home → /alerts 跳转能看到真实数据（人工烟雾）
- [x] 7.7 `openspec validate web-backend-integration --type change --strict` 通过
