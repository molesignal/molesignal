## Context

前两个前端 change 已把 API 客户端通到真后端 + 多语言/主题基础设施做好。Admin 域（teams / groups / license）的后端 endpoints 早已 ready 但前端是 PagePlaceholder。Admin 页面是典型 CRUD：list + detail + create/edit form + delete + bulk action，做好一套骨架就能复制粘贴。

## Goals / Non-Goals

**Goals:**
- 3 个新页面（Teams / Groups / License）都跑通真实 API、能 CRUD、能权限 gating
- 抽出 `admin/PageHeader` + `admin/DataTable` + `admin/ConfirmDialog` 三个共享组件让后续 admin 页直接复用
- 所有新页面文案走 i18n / 颜色走 token / 过 a11y axe critical=0
- /settings 内有 sub-nav 一眼看到所有 admin 入口

**Non-Goals:**
- 不实装其他 admin sub-route（profile / SSO / api tokens / audit log）—— 只放 PagePlaceholder
- 不重做 license billing flow（License 页只 read-only + 跳 marketplace；下单走 marketplace）
- 不引第三方 table 库（@tanstack/table 已在依赖里，直接用）

## Decisions

### D1：共享骨架 3 个组件

- `PageHeader`：title + subtitle + 右侧 actions slot；统一垂直节奏
- `DataTable<T>`：受控 sort/filter/paginate；接 `columns: ColumnDef<T>[]` 用 @tanstack/table；空状态 / loading / error 三态
- `ConfirmDialog`：destructive action 二次确认；title / description / confirm-label / variant=destructive

3 个新页面都用这套；后续 admin 续写继续复用。

### D2：role gating 实装在 RequireAuth 之外的 RouteGuard

新组件 `RouteGuard({ requireRole: 'Owner'|'Admin' })`：
- role 不够 → 渲染 `<NoAccess />`（"需要管理员权限" 友好文案 + return-to-home 链接）
- role 够 → 正常渲染 children

每个 `/settings/<admin>` 路由用 `<RouteGuard requireRole="Admin">` 包裹。

### D3：CRUD 表单走 react-hook-form + zod

`@hookform/resolvers` + `zod` 已经能装上不增 vendor 太多。每个 form 定义 zod schema → infer type → hook-form。错误显示走 shadcn Form 组件。

### D4：License 页 read-only + 跳 marketplace

License 不在 web 改（合规 + 计费走 marketplace 链接）。本 change 只 read `/api/v1/license`，展示 plan / quota / billing-period / 历史 invoice 表；"Upgrade" 按钮 `nav('/marketplace/upgrade')` 或外链 marketplace URL。

### D5：i18n 文案放 `admin.json` namespace

`i18n/en/admin.json`、`i18n/zh-CN/admin.json`；命名空间 `admin.teams.*` / `admin.groups.*` / `admin.license.*`。

## Risks / Trade-offs

**[R1] DataTable 抽象做不到位 → 后续每页都"绕过"它**
→ Mitigation：本 change 在 3 页里同时使用 + 互相比较，确认 API surface 够通用；如有 escape hatch 需求，提供 render-prop / slot

**[R2] react-hook-form + zod 学习曲线**
→ Mitigation：先在 Teams 页写一个 reference 模板；其他页复制粘贴

**[R3] license 数据敏感（plan / quota），缓存策略不当会让 admin 看到陈旧数字**
→ Mitigation：react-query `staleTime: 0` + `refetchOnMount: 'always'` 进 license 页强制 fresh

**[R4] e2e 套件需要为 3 个新 route 加 axe 扫**
→ Mitigation：`a11y-routes.spec.ts` 的 11 route 数组加 3 个 entries（变 14 route），mockBackend 加对应 endpoints
