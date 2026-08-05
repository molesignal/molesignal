## Why

`web-investigation-shell` 归档时把 `pnpm typecheck` 拒于门外的一批 TS strict 错误（`exactOptionalPropertyTypes` 不满足、`noUnusedLocals` 残留 React import、`HitTester` 等内部类型 leak 到 props）标为"预先存在、不在本 change 范围"。但本质上这是 web/ 启用严格 tsconfig 后没人系统补的一笔技术债。

后续 4 个前端 change（playwright-runtime / a11y-baseline / ui-polish）的 CI gate 都依赖 `pnpm typecheck` 为绿；这道墙不拆，下面三个都跑不动 CI。本 change 单独处理这道墙，不带任何 UI 行为变更。

## What Changes

- 修齐 `web/src/` 当前 `pnpm typecheck` 报的所有错误（按文件清单）：
  - `src/api/web.ts`：`signal` 可选字段在 `AxiosRequestConfig` 上 strict-mode 收敛，CorrelationContext 转换泛型显式化
  - `src/correlation/LinkMenu.tsx`：`parentFrameId: string | undefined` 通过条件展开消除 `BaseProps` 不兼容；`time_range_override` 改 `Partial` 或拆 union
  - `src/investigation/{DrawerFrame,FilterChipStrip,StackPortal,frames/TraceFrame}.tsx`：删未使用的 `import * as React`（new JSX transform 不需要）
  - `src/palette/CommandPalette.tsx`：lucide `ForwardRefExoticComponent` 与 `ComponentType<{className?}>` 类型不兼容 → 收敛到 `LucideIcon` 类型；`v.action` 在 union 上不存在 → 加 narrowing；`subtitle: string \| undefined` 显式
  - `src/palette/registry.tsx`：同 lucide icon 收敛
  - `src/viz/timeseries/TimeSeriesPlot.tsx`：`label: string | undefined` strict 不通过 → 条件展开 `axes[1]` 配置；line 160 `cursor.focus` 字段错误 → 改正确的 uPlot API
  - `src/viz/topology/ServiceEdge.tsx`：`style: CSSProperties | undefined` ↔ ReactFlow `EdgeProps.style` 不兼容 → 用 ?? `{}` 兜底
  - `src/viz/topology/ServiceNode.tsx` / `ServiceTopology.tsx` / `viz/trace/Tooltip.tsx` / `frames/TraceFrame.tsx`：删未用 React import + 未用形参
  - `src/viz/trace/TraceFlame.tsx`：`onSpanClick: ((span) => void) | undefined` strict mode 不能直接传 `Props`；`searchMatches: Set<string> | undefined` 同样问题 → 拆 props 类型 + 条件展开
- 在 `web/eslint.config.mjs` 加 `no-unused-vars`（含 `_` 前缀豁免）规则确保后续不再回退。
- 单元 + 集成测试不动；仅类型层面修复 + 必要 import 删除。
- 把 `15.1` follow-up 任务从 web-investigation-shell 余留清单中取下（spec 文本提一句指向本 change 即可）。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
<!-- 无：本 change 不改任何 Requirement 行为，仅修类型层面 -->

## Impact

- **代码**：`web/src/` 约 12 个文件类型层面修复；无运行时改动。
- **构建**：`pnpm -C web typecheck && pnpm -C web lint && pnpm -C web test:run && pnpm -C web build` 全绿，与 CI yaml 期望一致。
- **风险**：极低 —— 不改任何 prop / handler / API 调用，仅消除类型噪音 + 删未用 import。回归通过既有 vitest 覆盖。
- **跟随**：本 change land 后，`web-playwright-runtime` 才能真实在 CI 跑 typecheck job。
