## 1. API + correlation

- [x] 1.1 `src/api/web.ts` line 97 / 101：`AxiosRequestConfig.signal` 条件展开；`get<T>` 调用显式约束 T = CorrelationContext，去 cast
- [x] 1.2 `src/correlation/LinkMenu.tsx` 54/76/105：`parentFrameId` 通过 `...(parentFrameId && { parentFrameId })` 展开；`time_range_override` 同模式

## 2. Investigation frames（删未用 React import）

- [x] 2.1 `src/investigation/DrawerFrame.tsx` 删 line 2 `import * as React`
- [x] 2.2 `src/investigation/FilterChipStrip.tsx` 同
- [x] 2.3 `src/investigation/StackPortal.tsx` 同
- [x] 2.4 `src/investigation/frames/TraceFrame.tsx` 同

## 3. Palette

- [x] 3.1 `src/palette/CommandPalette.tsx` line 33-38：icon 字段类型从 `ComponentType<{className?}>` 换 `LucideIcon`
- [x] 3.2 `src/palette/CommandPalette.tsx` line 88：subtitle 条件展开
- [x] 3.3 `src/palette/CommandPalette.tsx` line 210：`v.action` 加 `if ('action' in v)` narrow
- [x] 3.4 `src/palette/CommandPalette.tsx` line 231：push 调用条件展开 parent_frame_id
- [x] 3.5 `src/palette/registry.tsx` line 51/60：icon 收敛到 `LucideIcon`

## 4. Visualizations

- [x] 4.1 `src/viz/timeseries/TimeSeriesPlot.tsx` line 97：axes[1] label 条件展开
- [x] 4.2 `src/viz/timeseries/TimeSeriesPlot.tsx` line 160：`cursor.focus.stroke` 改对的 uPlot 字段（focus 仅有 `prox`/`bias`，stroke 应在 series 上）
- [x] 4.3 `src/viz/topology/ServiceEdge.tsx` line 45/51：`style: style ?? {}` 兜底；markerEnd 同
- [x] 4.4 `src/viz/topology/ServiceNode.tsx` line 1：删 unused React
- [x] 4.5 `src/viz/topology/ServiceTopology.tsx` line 92：unused 形参 `n` 改 `_n` 或删
- [x] 4.6 `src/viz/trace/Tooltip.tsx` line 1：删 unused React
- [x] 4.7 `src/viz/trace/TraceFlame.tsx` line 59：`onSpanClick` 条件展开
- [x] 4.8 `src/viz/trace/TraceFlame.tsx` line 108：`searchMatches` 条件展开
- [x] 4.9 `src/viz/timeseries/axisModes.ts` line 33/34：`forward`/`backward` 闭包形参 `v` 加 type annotation

## 5. ESLint + CI

- [x] 5.1 `web/eslint.config.mjs` 加 `@typescript-eslint/no-unused-vars` 规则（`argsIgnorePattern: '^_', varsIgnorePattern: '^_'`）
- [x] 5.2 验证现有 vitest 测试不被规则误伤：`pnpm -C web lint --max-warnings 0` 通过

## 6. 完工校验

- [x] 6.1 `pnpm -C web typecheck` 0 错误
- [x] 6.2 `pnpm -C web lint` 0 错误 0 warning
- [x] 6.3 `pnpm -C web test:run` ≥ 35 通过（不破现有 vitest 套件）
- [x] 6.4 `pnpm -C web build` 出 dist
- [x] 6.5 `openspec validate web-typescript-strict-fixes --type change --strict` 通过

## 7. 实施期间发现并修齐的同源债务（tasks.md 起草时遗漏）

> 这些修复同属 proposal "lint --max-warnings 0 通过、build gate 全绿"目标，是任务原列表的真正前置条件。一并完成以闭环 6.1–6.5。

### 7.1 缺失依赖（lint 自始不可执行的根因）
- [x] 7.1.1 `pnpm -C web add -D @eslint/js@^9.0.0`（eslint.config.mjs 第 1 行 import 但 package.json 未声明，旧 lint 报 `ERR_MODULE_NOT_FOUND`）
- [x] 7.1.2 `pnpm -C web add -D eslint-import-resolver-typescript`（`import/order` 规则下 ts-resolver 缺失，每个 ts 文件都报 "invalid interface loaded as resolver"）

### 7.2 typecheck 残余错误（tasks 未列但同属 exactOptionalPropertyTypes / noUnusedLocals 同类）
- [x] 7.2.1 `src/shell/IconRail.tsx` `RailItem.icon` 同 lucide-react `ForwardRefExoticComponent` 不兼容 `ComponentType<{className?}>` → 收敛到 `LucideIcon`
- [x] 7.2.2 `src/shell/ui/context-menu.tsx` line 85 `checked: CheckedState | undefined` → 条件展开
- [x] 7.2.3 `src/shell/ui/dropdown-menu.tsx` line 86 同上
- [x] 7.2.4 `src/shell/UrlHydration.ts` line 67 Frame 解码：`time_range_override / anchor_override / parent_frame_id` 三字段条件展开
- [x] 7.2.5 `src/viz/log/LogStream.tsx` line 32 `hoverTimerRef` 改用 `ReturnType<typeof setTimeout>`（`setTimeout` 在 node 类型下返回 `Timeout` 非 `number`）
- [x] 7.2.6 `src/viz/log/useStreamingLogs.ts` line 30 `version` 未用 → 解构改 `[, setVersion]`；line 52 `body: undefined` 不兼容 fetch 的 `BodyInit | null` → 条件展开

### 7.3 lint 残余错误
- [x] 7.3.1 `eslint.config.mjs` globals 补 `atob / btoa / getComputedStyle / self / React / MessageEvent`（消除 13 处 no-undef）
- [x] 7.3.2 `eslint.config.mjs` Playwright/scripts override 加 `'no-empty-pattern': 'off'` + `'react-hooks/rules-of-hooks': 'off'`（Playwright fixture 习惯用 `async ({}, use) => ...`，`use` 是 fixture lifecycle 不是 React Hook）
- [x] 7.3.3 `eslint.config.mjs` shell/ui 例外名单加入 `src/shell/FormDrawer.tsx`（FormDrawer 是 shell-level Radix Dialog 组合，shell/ui/dialog 不覆盖其用法）
- [x] 7.3.4 `src/routes/Logs.tsx` lines 444-452 JSON-like 文本里 8 个裸 `"` → `&quot;`
- [x] 7.3.5 `src/routes/Ingest/Ingest.tsx` line 99 同上
- [x] 7.3.6 `src/routes/Login.tsx` line 206 `function decodeContext(token: string): import('@/stores/auth').AuthContext` → `import type { AuthContext }` + 改签名
- [x] 7.3.7 `playwright/tests/05-visual.spec.ts` line 148 同上模式 → `import type { Page } from '@playwright/test'`
- [x] 7.3.8 `src/shell/Topbar.tsx` line 77 JSX 注释 `{/* global search */}` → `{/* Global search button */}`（小写 `global X` 被 ESLint 当作 `/* global X */` directive，声明了一个名为 `search` 的全局变量进而触发 unused 报错）
- [x] 7.3.9 `src/shell/ui/command.tsx` line 42 `cmdk-input-wrapper=""` 是 cmdk 库约定 attribute → 单行 `// eslint-disable-next-line react/no-unknown-property`
- [x] 7.3.10 `src/keyboard/HelpOverlay.tsx` line 16 `bindings` 在条件下重算，破坏 useMemo 依赖 → 把 `bindings` 计算挪进 useMemo 内部，依赖换为 `[open, ctrl, scope]`，删外层重复变量
- [x] 7.3.11 `src/viz/log/useStreamingLogs.ts` line 103 useEffect deps 含 `JSON.stringify(body ?? {})`（react-hooks/exhaustive-deps 不认这种动态表达式）→ 单行 `// eslint-disable-next-line` + 解释注释

### 7.4 vitest / build 配置
- [x] 7.4.1 `vitest.config.ts` 加 `exclude: ['node_modules/**', 'dist/**', 'playwright/**']`（原配置导致 vitest 收集 7 个 `.spec.ts` Playwright 文件并失败）
- [x] 7.4.2 `package.json` `build` 脚本：`tsc -b` 改 `tsc -b --noEmit`（原命令把 `.js`/`.js.map`/`.d.ts` emit 到源目录，污染下一次 lint）
- [x] 7.4.3 `eslint.config.mjs` `ignores` 加 `src/**/*.{js,js.map,d.ts}`、`playwright/**/*.{js,js.map,d.ts}`、`scripts/**/*.{js,js.map,d.ts}`、`vite.config.{js,js.map,d.ts}`、`vitest.config.{js,js.map,d.ts}`（兜底：本机/CI 若残存旧 emit 产物不再让 lint 误判）
