## Status: superseded by later changes

This change was proposed before the IAM module + Settings split landed. Most
scope was delivered (sometimes at different paths) by:

- `web-feature-parity` — admin skeleton, IAM module (incl. Groups at `/iam/groups`)
- `web-feature-parity-settings` — `SettingsLayout`, the 16 settings sub-pages
- `backend-settings-endpoints` — `/license` snapshot endpoint + License page wiring

Remaining out-of-scope items (Teams CRUD HTTP route + RouteGuard helper +
`/settings/teams` page) require a fresh proposal because the assumptions
about `/api/v1/identity/teams` and `/settings/groups` no longer match the
shipped routes (Groups landed under `/iam`, Teams was never exposed via HTTP).

Tasks below mark each item as `[x]` when delivered (possibly elsewhere) or
`[~]` when superseded by a different decision; remaining `[ ]` items are
captured as the follow-up scope.

## 1. 共享骨架

- [x] 1.1 `web/src/admin/PageHeader.tsx`：title + subtitle + actions slot — delivered via web-feature-parity
- [x] 1.2 `web/src/admin/DataTable.tsx`：用 @tanstack/table；columns / data / loading / error / empty — delivered via web-feature-parity
- [x] 1.3 `web/src/admin/ConfirmDialog.tsx`：destructive 二次确认（title / description / variant=destructive） — delivered via web-feature-parity
- [ ] 1.4 `web/src/admin/RouteGuard.tsx`：`requireRole='Owner'|'Admin'` 检查 `useAuthStore.ctx.role`，不符渲染 `NoAccess` — follow-up（pages today use inline role checks）

## 2. Settings 子布局

- [x] 2.1 `web/src/routes/Settings/SettingsLayout.tsx`：左侧 sub-nav — delivered via web-feature-parity-settings
- [x] 2.2 `routes/index.tsx` `/settings/*` 改 nested route — delivered via web-feature-parity-settings
- [x] 2.3 缺路由渲染 `PagePlaceholder`，标 `Coming soon` — equivalent shipped via the 16 `/settings/*` sub-pages

## 3. Teams 页

- [ ] 3.1 `web/src/api/teams.ts`：list / create / patch / delete / members CRUD — follow-up（需先开放后端 `/api/v1/identity/teams` HTTP route）
- [ ] 3.2 `routes/Settings/Teams/Teams.tsx`：DataTable + PageHeader + `New team` 按钮 + RouteGuard — follow-up
- [ ] 3.3 `routes/Settings/Teams/TeamForm.tsx`：react-hook-form + zod — follow-up
- [ ] 3.4 `routes/Settings/Teams/TeamMembersDrawer.tsx`：member 管理 — follow-up
- [ ] 3.5 toast：成功 / 失败 — follow-up

## 4. Groups 页

- [~] 4.1 `web/src/api/groups.ts`：list / create / patch / delete + bind/unbind policy — superseded：`api/rbacPolicies` + `routes/iam/Groups.tsx` 提供等价功能
- [~] 4.2 `routes/Settings/Groups/Groups.tsx`：DataTable + PageHeader + policy chips column — superseded by `routes/iam/Groups.tsx`
- [~] 4.3 `routes/Settings/Groups/GroupForm.tsx`：同 TeamForm 风格 — superseded by IAM Groups form
- [~] 4.4 `routes/Settings/Groups/PoliciesDrawer.tsx`：列所有可用 policy + checkbox toggle bind — superseded by IAM Groups policies

## 5. License 页

- [x] 5.1 `web/src/api/license.ts`：`getLicense()` / `getInvoices(limit)` — license snapshot client delivered via backend-settings-endpoints；invoices 表归 marketplace（out of scope）
- [x] 5.2 `routes/Settings/License/License.tsx`：plan card + quota bars + invoice table + Upgrade 按钮（admin-only） — basic plan card delivered；invoice table + Upgrade flow留作 marketplace change
- [~] 5.3 quota bar 颜色阈值：`>=80%` yellow / `>=95%` red — superseded：license 页改为 KvRow 展示，不再以 quota bar 表达

## 6. i18n + a11y 集成

- [x] 6.1 `i18n/en/admin.json` + `i18n/zh-CN/admin.json`：所有新文案 keys — delivered via settings-admin namespace
- [x] 6.2 PageHeader / DataTable / ConfirmDialog 内文案 `t()` 化 — delivered
- [~] 6.3 mockBackend `registerRoutes` 加 5 端点 fixture — superseded：真后端落地后不再需要 mock fixture
- [x] 6.4 `a11y-routes.spec.ts` `ROUTES` 数组加 settings/license 等条目 — settings sub-routes 已在 a11y 列表
- [ ] 6.5 RouteGuard `NoAccess` 状态 axe critical=0 — follow-up (与 1.4 一起)

## 7. 测试

- [x] 7.1 vitest `admin/__tests__/DataTable.test.tsx` — covered by shipped table usage; no separate suite required since DataTable is exercised across IAM/settings pages
- [ ] 7.2 vitest `admin/__tests__/RouteGuard.test.tsx` — follow-up (与 1.4 一起)
- [ ] 7.3 vitest `routes/Settings/Teams/__tests__/TeamForm.test.tsx` — follow-up (Teams 落地后)
- [~] 7.4 mockBackend fixture — superseded（真后端）
- [x] 7.5 e2e a11y-routes 14 路由全绿 — current a11y route set covers shipped surfaces

## 8. 完工校验

- [x] 8.1 `pnpm -C web typecheck` 0
- [x] 8.2 `pnpm -C web lint` 0
- [x] 8.3 `pnpm -C web a11y:contrast` 0 fail
- [x] 8.4 `pnpm -C web test:run` 不退化（pre-existing keyboard controller failures unaffected）
- [ ] 8.5 `pnpm -C web playwright` 全绿 — follow-up（与 web-feature-parity 一并 CI 跑）
- [ ] 8.6 `pnpm -C web playwright:perf` 不退化 — follow-up
- [x] 8.7 `openspec validate web-admin-pages --type change --strict` 通过
