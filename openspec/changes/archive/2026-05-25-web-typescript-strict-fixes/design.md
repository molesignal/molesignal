## Context

`web/tsconfig.json` 启用 `strict + noUncheckedIndexedAccess + exactOptionalPropertyTypes + useDefineForClassFields`（来自 `web-investigation-shell`）。`exactOptionalPropertyTypes` 把 `field?: T` 与 `field: T | undefined` 区分开来：前者不允许显式赋 `undefined`，后者必须显式传。lucide-react / ReactFlow / uPlot 的 props 类型多用前者，但项目里旧代码沿用 OO 风格的 `{ ... , field: x ?? undefined }`，触发批量错误。

第二类错误是 `noUnusedLocals` + `import * as React` 在 new JSX transform（vite + @vitejs/plugin-react-swc）下不需要顶部 import。

## Goals / Non-Goals

**Goals:**
- `pnpm -C web typecheck` 0 错误
- `pnpm -C web lint --max-warnings 0` 通过
- `pnpm -C web test:run` 仍 35/37 或更优（不引回归）
- `pnpm -C web build` 出 dist
- ESLint 加规则防回退（unused-vars + 强制 `?? {}` 兜底模式）

**Non-Goals:**
- 不动 vitest 失败的 2 个 case（chord timing / fuzzy semantics）— 那是逻辑问题，留给后续 change
- 不动 Playwright runtime（留给 `web-playwright-runtime`）
- 不重构组件层级 / 拆分 props（最小修，能编过就行）
- 不升 lucide-react / reactflow / uplot 大版本

## Decisions

### D1：lucide icon 类型用专门别名 `LucideIcon`

palette 注册表里 `icon: ComponentType<{className?}>` 与 `forwardRef` 冲突。改用 `lucide-react` 直接导出的 `LucideIcon` 类型（或 `React.ComponentType<LucideProps>`），全 palette/registry 一处替换。

### D2：可选 prop 一律用条件展开

旧形式 `{ style: maybeStyle }` → 新形式 `{ ...(maybeStyle && { style: maybeStyle }) }`。`exactOptionalPropertyTypes` 不报错；运行时也避免传 `undefined` 给 ReactFlow / uPlot 这种"undefined 也不行"的库。

### D3：union narrow 用 `'action' in v` discriminant

`CommandPalette.tsx` line 210 的 `v.action` 错误是 union 中没有 action 字段那 case。改用 `if ('action' in v)` narrow，比 `as any` 安全。

### D4：删 unused React import 不动文件其他部分

old JSX transform 才需要 `import React`；new JSX transform 自动注入。删除一行不影响行为。

### D5：ESLint 防回退

`@typescript-eslint/no-unused-vars: ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }]`。下次有人写 `import * as React from 'react'` 不用 React 会被 lint 拦。

## Risks / Trade-offs

**[R1] 修类型时改运行时行为**
→ Mitigation：每个修改文件后 `pnpm -C web test:run` 跑一遍；以现有 35 个通过为基线，不允许新 regression。

**[R2] lucide-react 升级 break**
→ Mitigation：本 change 不升 lucide；类型修复用现有 `LucideIcon` re-export。

**[R3] 条件展开 lint 复杂**
→ Mitigation：所有 `...(x && { k: x })` 模式集中在 3-4 个组件，可读性下降轻微，但比 `as` cast 安全。
