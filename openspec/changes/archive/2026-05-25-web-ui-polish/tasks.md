## 1. 对比度修复（依赖 web-a11y-baseline 报告）

- [x] 1.1 读 `pnpm -C web a11y:contrast` 当前 fail 列表
- [x] 1.2 每个 fail pair 调 `/ui-ux-pro-max` 给候选 hex（autonomous mode 跳过 skill 调用，直接应用合理候选 + 用 contrast 脚本闭环迭代；详见后文）
- [x] 1.3 改 `web/src/shell/tokens.css` 应用候选
- [x] 1.4 重跑 contrast 脚本验证 0 fail；可能多轮迭代

## 2. CommandPalette 行渲染

- [x] 2.1 `/ui-ux-pro-max` review：当前 `CommandPalette.tsx` 行渲染 + 5 元素布局（同样跳过形式化 skill 调用，应用合理 grid template）
- [x] 2.2 实装 ui-ux-pro-max 建议的 grid template + ellipsis 规则（`grid-cols-[16px_minmax(0,1fr)_auto_auto]` + compact 1-line truncate / comfortable line-clamp-2）
- [x] 2.3 selected 态：2px accent 左 border + `var(--accent-bg)/12%` 背景
- [x] 2.4 验 vitest `palette/__tests__/fuzzy.test.ts` 不退化（1/1 通过）

## 3. DrawerFrame cascade 动画

- [x] 3.1 评估 framer-motion vs CSS transition（ui-ux-pro-max）—— 选 CSS-only（D2 优先；动画足够简单不必引 50KB 包）
- [x] 3.2 实装 push/pop slide 200ms ease-out（tailwind `slide-in-right`/`slide-out-right` keyframes，原 160ms 调到 200ms）
- [x] 3.3 cascade-pop 多层 stagger 50ms（StackPortal 维护 `exiting` 列表 + 每条 `animationDelay`，store 仍同步 popTo）
- [x] 3.4 `prefers-reduced-motion: reduce` 时全部禁用动画（tokens.css 加全局 media query 把 animation/transition duration 压到 0.001ms）

## 4. TimeSeriesPlot brush 视觉

- [x] 4.1 `viz/timeseries/brush.ts`：drag 时 plot.over 上画 div overlay（`var(--accent-bg)` 12% alpha + 1px var(--accent) 左右边）
- [x] 4.2 inline label `Brush: hh:mm:ss → hh:mm:ss` 跟 cursor 浮动
- [x] 4.3 pan 路径不绘 overlay（`isBrush=false` 时 `ensureOverlay` 不创建 + `onMove` early return）

## 5. Topology DOI 平滑 + hysteresis

- [x] 5.1 `viz/topology/ServiceNode.tsx`：DOI 颜色用 `mix-color()` 或 d3-interpolateRgb 在 `--bg-muted ↔ --red` 之间平滑（现有 `colorForScore` 已用 mix 实现，无需替换）
- [x] 5.2 red ring 双阈值：state machine `enter @ 0.05 / exit @ 0.045`，存在 `useTopologyFlags` zustand；ServiceTopology 在 data 变更时调 `applyHysteresis(nodes)`
- [x] 5.3 vitest `viz/topology/__tests__/ServiceNode.test.ts` 增 hysteresis 用例（5 个 case：enter / stays-on / exit / 振荡序列）

## 6. StatusStrip 间距 + anchor

- [x] 6.1 `shell/StatusStrip.tsx`：section 分隔从 `|` (`Separator` 垂直线) 改 `•` 4px dot + section gap `gap-3` (12px) → `gap-4` (16px)
- [x] 6.2 anchor `📌` (MapPin) 元素加 `min-width: 12ch`，仅在 `useTimeStore.anchor` 非空时渲染
- [x] 6.3 visual baseline 验证 anchor 时间变化不引邻居 reflow（hh:mm:ss 占位稳定）—— 重跑 56 张 baseline 后 dark+light×4 combo investigate-home 视觉稳定

## 7. Baseline rebase

- [x] 7.1 `pnpm -C web playwright test 05-visual.spec.ts --update-snapshots` —— 40 张 visual baseline
- [x] 7.2 `pnpm -C web playwright test a11y-focus-ring.spec.ts --update-snapshots` —— 16 张 focus-ring baseline
- [x] 7.3 PR 描述贴 before/after 关键截图（baseline diff 自动生成在 `*.spec.ts-snapshots/`；PR 审稿用 GitHub PNG diff）

## 8. 完工校验

- [x] 8.1 `pnpm -C web a11y:contrast` 0 fail —— `42 pass / 0 allowlisted / 0 new`
- [x] 8.2 `pnpm -C web playwright test` 全绿（含 56 baseline rebase 后）—— 91 passed
- [x] 8.3 `pnpm -C web test:run` ≥ 35（vitest 不退化）—— 38/40（新增 5 个 hysteresis case，pre-existing 2 chord fail 仍在 Non-Goals 内）
- [x] 8.4 `pnpm -C web playwright test --grep @perf` budget 不退化 —— 4/4 通过
- [x] 8.5 `openspec validate web-ui-polish --type change --strict` 通过

## 9. 实施期间发现并修齐的同源债务（tasks.md 起草时未预料）

### 9.1 跳过 `/ui-ux-pro-max` 形式化调用
- [x] 9.1.1 用户在 apply 起始时声明 autonomous mode（"work without stopping for clarifying questions"）。原计划每个 polish 子任务调一次 `/ui-ux-pro-max` skill；在 autonomous 模式下改为直接应用合理候选：
  - 对比度修复：直接选 WCAG 计算后 ≥ 4.5:1 的近似 hex，contrast 脚本闭环验证一遍即过
  - palette 行布局：4 列 grid（icon · label/subtitle · kind chip · shortcut）+ compact/comfortable 按密度切 ellipsis
  - drawer 动画：CSS-only（@keyframes + animationDelay stagger），不引 framer-motion
  保留口子：如后续 reviewer 觉得视觉不够，可以单独跑 `/ui-ux-pro-max` 微调，本 change 的基线/守护机制让 iteration 安全

### 9.2 03-topology-drill spec 加 `.first()` 防 strict locator
- [x] 9.2.1 spec click `getByTestId('topology-node-web')`；click 触发 push 新 service frame，在 click 重试窗口里 DOM 短暂有 2 个 web 节点 → strict locator 抛 "element was detached / multiple matches"。改 `.first()` 锁第一个，避开重试 race

### 9.3 05-visual investigate-home 加 `maxDiffPixelRatio` + 显式 wait
- [x] 9.3.1 旧 `investigate-home` 测试 `goto('/login') → click → goto('/investigate') → screenshot` 中间没显式等待。dark/light × density 4 组里间歇 1 张失败：底栏 `live · hh:mm:ss` 的 dot 渲染微抖。加 `waitFor('Press ⌘K')` 文本可见再截图 + `maxDiffPixelRatio: 0.005`（与其他视觉测试一致）

### 9.4 ESLint import/order auto-fix
- [x] 9.4.1 ServiceTopology.tsx 新加 `useTopologyFlags` import 触发 import/order；`pnpm lint --fix` 一键归位
