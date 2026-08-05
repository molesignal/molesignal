## Status: superseded

Most of this proposal landed under different changes after the IAM module
took shape:

- Admin skeleton (`PageHeader` / `DataTable` / `ConfirmDialog`) → `web-feature-parity`
- Settings sub-nav + nested routes → `web-feature-parity-settings`
- Groups management → `routes/iam/Groups.tsx` (via `web-feature-parity`)
- License read view → `web-feature-parity-settings` + `backend-settings-endpoints` (`/license` endpoint)

Remaining scope (Teams CRUD HTTP route + `/settings/teams` page + a generic
`RouteGuard` component) needs a fresh proposal because:

1. The backend `/api/v1/identity/teams` HTTP route was never exposed (only the
   `TeamRepository` trait + Pg impl exist).
2. Groups ended up under `/iam/groups`, not `/settings/groups`, so the
   `/settings/*` shape this proposal assumed is out of date.

This change is archived in-place to preserve the original rationale. File a
follow-up like `web-iam-teams` if Teams management becomes user-visible.

## Why

backend 已落 `identity` / `fga-policies` / `license` 三个能力（见 `openspec/specs/`），但前端没对应管理页：
- **Teams**：跨多个用户/服务身份的协作单位（用 `identity` 的 team 端点）；前端今天进不去
- **Groups**：FGA RBAC 用 `groups` 来分配 policy；现在只能通过 API 配
- **License**：当前 plan、配额使用、到期时间、续费入口都在 `license` capability 里，前端没暴露

`web-backend-integration` 把 API 路径 audit 通 + 加了 useOrgStore；本 change 在它基础上写实页面，let admins manage these in-app instead of 走 API client。

## What Changes

- **新增 3 个路由 + 页面**：`/settings/teams`、`/settings/groups`、`/settings/license`
  - Teams：list + create + edit + delete + member 管理（add user / remove user / change role）
  - Groups：list + create + edit + delete + policy 绑定（关联到 FGA policy）
  - License：read-only 显示当前 plan / quota / 到期 / 历史 invoice；含 `Upgrade plan` 按钮跳 marketplace
- **Settings 子菜单导航**：左侧 Settings 路由内加 sub-nav（Profile / Teams / Groups / License / SSO / ...）；本 change 实装前 3 个 sub-route，余下 sub-route 是 PagePlaceholder
- **统一 admin 页面骨架**：抽 `web/src/admin/PageHeader.tsx` + `web/src/admin/DataTable.tsx`（基于 shadcn Table），让 3 个新页 + 后续 admin 页复用
- **权限 gating**：useAuthStore.ctx.role 不是 `Owner|Admin` 时，Teams/Groups/License 显示 read-only state + 「需要管理员权限」提示

## Capabilities

### New Capabilities

- `web-admin-teams`: Teams 管理页（list / create / edit / delete / members）
- `web-admin-groups`: Groups 管理页 + 与 FGA policy 关联
- `web-admin-license`: License 当前态展示 + Upgrade 跳转

### Modified Capabilities

- `web-shell`: `/settings/*` 子路由 + Settings sub-nav；role-gated visibility

## Impact

- **代码**：`web/src/routes/Settings/{Teams,Groups,License}/*.tsx`（每页一个 list + 一个 detail/edit form）+ `web/src/admin/{PageHeader,DataTable,ConfirmDialog}.tsx`（共享骨架）+ `web/src/api/{teams,groups,license}.ts`（新 client）
- **依赖**：无新外部包；用现有 shadcn Table / Dialog / Form
- **i18n**：所有新文案走 `t()` keys 在 `i18n/en/admin.json` + `zh-CN/admin.json`
- **a11y**：每页过 axe critical=0（a11y-routes spec 自动覆盖新加路由）
- **风险**：3 页 CRUD 是重复的样板代码，DataTable 抽象做不好的话会成"看不见的复杂"；本 change 在 3 页之间反复打磨抽象，留给后续 admin 页直接复用
- **跟随**：land 后剩余 admin sub-routes（profile / sso / api-tokens / audit log 等）按同一模板续写
