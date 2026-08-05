## 0. 准备

- [x] 0.1 新增 i18n namespace：`i18n/{en,zh-CN}/settings-admin.json`（16 个 section 文案）+ `index.ts` 注册
- [x] 0.2 新增 API clients（按需）：`api/{alertTemplates,connectors,cipherKeys,storageProviders,clusters,domains,runningQueries}.ts`（alert channels 复用已有 `channels.ts`）
- [x] 0.3 `api/index.ts` 导出新 clients

## 1. SettingsLayout + 路由表

- [x] 1.1 `web/src/routes/settings/SettingsLayout.tsx`：内嵌 sidebar（5 个 group × 16 项）+ `<Outlet />` + 高亮当前路径
- [x] 1.2 `routes/index.tsx` 中移除现有 `path: 'settings/*'` wildcard；改为以 `SettingsLayout` 为父路由 + 16 条子路由 + index redirect 到 `general`
- [x] 1.3 删除现有 `routes/Settings.tsx` 文件（沉到子页）

## 2. ACCOUNT 组（3 页）

- [x] 2.1 `routes/settings/General.tsx`：用 `users.get(currentId)` 拉资料；偏好项本地保存
- [x] 2.2 `routes/settings/Organization.tsx`：从 `useOrgStore` 读当前 org，展示元数据
- [x] 2.3 `routes/settings/License.tsx`：`EmptyState awaitingBackend`，描述待 `/license` 端点

## 3. DATA PLANE 组（4 页）

- [x] 3.1 `routes/settings/StorageSettings.tsx`：`storageProviders.list()` + 上传表单
- [x] 3.2 `routes/settings/PipelineDestinations.tsx`：`connectors.list/create/remove`
- [x] 3.3 `routes/settings/Nodes.tsx`：`clusters.list()` 列出节点 + 健康灯
- [x] 3.4 `routes/settings/Correlation.tsx`：只读展示当前 correlation 配置

## 4. ALERTS 组（2 页）

- [x] 4.1 `routes/settings/AlertDestinations.tsx`：复用已有 `channels.ts`（list/remove；create 引导到 Alerts 页 / API）
- [x] 4.2 `routes/settings/AlertTemplates.tsx`：列表（endpoint 缺则 `EmptyState awaitingBackend`）

## 5. SECURITY 组（4 页）

- [x] 5.1 `routes/settings/CipherKeys.tsx`：list / create / rotate / delete，rotate 调 `POST /cipher_keys/:name/rotate`
- [x] 5.2 `routes/settings/RegexPatterns.tsx`：`EmptyState awaitingBackend`
- [x] 5.3 `routes/settings/DomainManagement.tsx`：`domains.list/create/renew`
- [x] 5.4 `routes/settings/OrganizationManagement.tsx`：`orgs.list/create` + 成员管理；区分 owner / admin 才显示创建

## 6. ML OPS 组（3 页）

- [x] 6.1 `routes/settings/AiToolsets.tsx`：`EmptyState awaitingBackend`
- [x] 6.2 `routes/settings/ModelPricing.tsx`：`EmptyState awaitingBackend`
- [x] 6.3 `routes/settings/QueryManagement.tsx`：`runningQueries.list()` + cancel；endpoint 缺则 `awaitingBackend`

## 7. 索引 + 链接更新

- [x] 7.1 `routes/settings/index.ts` 导出 16 个 Section 组件
- [x] 7.2 `shell/Sidebar.tsx` ADMIN > Settings 入口 `to` 由 `/settings` 改为 `/settings/general`（保留 icon / label）
- [x] 7.3 `docs/web/sitemap-diff.md`：把 P1 全部 16 条标 ✓

## 8. 校验

- [x] 8.1 `pnpm -C web typecheck` 0
- [x] 8.2 `pnpm -C web lint` 0（含 no-hardcoded-black 135 个 .tsx 0 violation）
- [x] 8.3 `pnpm -C web test:run` 47 / 49（仅 2 个 pre-existing keyboard/controller 失败，未触及该模块；未退化）
- [x] 8.4 `pnpm -C web a11y:contrast` 仍 93 pass
- [x] 8.5 `playwright/tests/a11y-routes.spec.ts` 加 16 条 settings 路径（spec 已注册；本地 sandbox 无 browser fixture，留 CI 跑实际 axe）
- [x] 8.6 `openspec validate web-feature-parity-settings --type change --strict` 通过
