## Context

前 3 个前端 change 把"基础设施层面"的债清完：

| 前置 change | 提供的基础 |
|---|---|
| `web-typescript-strict-fixes` | `pnpm typecheck/lint/build` 全绿 |
| `web-playwright-runtime` | e2e + perf 套件确定性 + CI 上传 trace |
| `web-a11y-baseline` | 对比度脚本 + axe critical=0 + focus-ring 16 baseline + 键盘地图 |

到这里所有"会自动告警"的护栏都立起来了。本 change 在它们之上做**视觉 polish**，每一步都能被基线守护。

## Goals / Non-Goals

**Goals:**
- `pnpm a11y:contrast` 全绿（修齐 dark/light 对比度问题）
- 7 个组件的视觉 polish 与 `/ui-ux-pro-max` 评审过的 mock 一致
- 56 张 baseline PNG 全部 rebase 完且不再 flake
- 不引入对 vitest 现有 35/37 case 的 regression

**Non-Goals:**
- 不重新设计 IconRail / palette 信息架构（这是 web-investigation-shell M2 范畴）
- 不做 i18n（i18n 是独立 change，留 follow-up）
- 不重写 9 色 token（仅微调对比度，不增减色相）
- 不接 storybook（评审走 ui-ux-pro-max + visual baseline 即可）

## Decisions

### D1：所有视觉决策走 `/ui-ux-pro-max` skill

每个子任务对应一个 ui-ux-pro-max 调用，输入"当前组件代码 + a11y baseline 报告 + 设计意图"，拿回具体改动建议（颜色 hex / 间距 px / 动画 ms）。本 change 不在 design.md 里 hard-code 视觉数值；apply 阶段调 skill 实时拿。

### D2：动画库选 framer-motion（若需要）

理由：跟 shadcn-ui new-york 风格的 anti-glassmorphism 一致；体积 < 50 KB gzipped；DOM 友好（不破 a11y）。备选 `@react-spring/web` 更小但 API 学习曲线陡。先评审。

### D3：baseline rebase 走"一次性"模式

token + 间距改完后，最后一个 commit 是 `pnpm playwright test --update-snapshots`，让 56 张 PNG 一次性更新；PR 描述中明确列出哪些 baseline 因为哪个改动变更。

### D4：red ring 阈值从硬阈到 hysteresis

避免 5.0% ↔ 4.99% 来回切。`0.045 enter / 0.05 exit`：err_rate 上升到 5% 触发 red ring，下降到 4.5% 才消失。

### D5：StatusStrip 不重做，只调间距

不动信息架构（org / cluster / window / ⌘K / avatar 5 元素顺序固定）；分隔符从 `|` 改 4px • dot，section gap 12px → 16px，让眼睛能扫。

## Risks / Trade-offs

**[R1] framer-motion 体积**
→ Mitigation：若 ui-ux-pro-max 评审认为 CSS transition 足够，不引入此包；vendor chunk size 守护通过 vite manualChunks。

**[R2] baseline rebase 难以 review**
→ Mitigation：PR 描述里贴 before/after 对比图（GitHub 自动 diff PNG）；reviewer 抽样 5-10 张关键路径即可。

**[R3] /ui-ux-pro-max 建议与现有 tokens 偏离**
→ Mitigation：skill 调用前在 prompt 里钉死"只能改 hex 不能加色相"；评审输出格式 `(old: #hex, new: #hex, ratio: x→y)` 便于 diff review。

**[R4] perf 影响（动画消耗 GPU）**
→ Mitigation：本 change 末尾跑一次 `--grep @perf` 确认 budget 不退化。
