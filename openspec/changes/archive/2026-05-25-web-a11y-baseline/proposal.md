## Why

`web-investigation-shell` 列了"颜色与 a11y 检视：dark / light × compact / comfortable × 主区/palette/4 viz 共 32 张截图人工抽样，确认 9 色 token 一致、focus ring 可见、对比度合格" —— 归档时标为人工任务。

但人工抽样是不可重复也不可 CI 守护的。要真的把 a11y 当成一等指标，需要：

1. axe-core 在 Playwright 里跑过的 **critical 违规** 必须 0（已在 04-a11y 起步，但只覆盖 ⌘K + `?` 两路径）
2. WCAG **对比度 4.5:1** 在所有 9 个 token 上自动验证（生成报告 + assert）
3. **focus ring** 在 dark/light × compact/comfortable 四态下都视觉可分辨（用 visual baseline 守护 + 单独 a11y screenshot 套件）
4. **键盘可达性矩阵**：32 张人工抽样转成 32 个 `await expect(page).toHaveScreenshot()` + a11y 报告

本 change 把“人工抽样”提级到“CI 守护的 a11y baseline 套件”，同时把已有 visual 40 张 baseline 复用为 a11y 现场数据。

## What Changes

- **对比度自动检查**：`scripts/check-contrast.ts`（tsx 入口）读 `shell/tokens.css` 中 9 色 token + bg/fg variant，按 WCAG 2.1 算 4.5:1 比；fail 即 exit 非 0。CI `web.yml` ci job 加跑。
- **axe-core 全路径扫**：扩 `04-a11y-keyboard.spec.ts` → `a11y-routes.spec.ts`，对 11 条认证路由（/home /investigate /logs /metrics /traces /dashboards /alerts /streams /settings /noc + login）每条都跑 axe critical=0 断言。
- **focus ring 视觉断言**：单独 `a11y-focus-ring.spec.ts`：每个 viz route Tab 进焦点元素 → screenshot 该元素 → 与 baseline 对比。dark/light × compact/comfortable × 4 viz = 16 baselines（少于 32 因为 viz 默认状态压一压）。
- **键盘地图测试**：从 `keyboard/bindings.ts::GLOBAL_KEYMAP` 自动生成 spec：每个 binding 测一次"keydown 触发→ handler 调起→ DOM 进入预期状态"。
- 不动产品行为；仅加守护 + baseline。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `web-shell`: 增"A11y CI Gate"（对比度 + axe + focus ring + 键盘地图四项）

## Impact

- **新文件**：`web/scripts/check-contrast.ts`、`web/playwright/tests/a11y-routes.spec.ts`、`web/playwright/tests/a11y-focus-ring.spec.ts`、`web/playwright/tests/a11y-keyboard-map.spec.ts`
- **新 baseline**：focus-ring 16 张 PNG（落 `a11y-focus-ring.spec.ts-snapshots/`）
- **CI**：`web.yml` ci job 加 `pnpm -C web a11y:contrast`；playwright job 拢入新 a11y spec
- **风险**：若 9 色 token 真有不达 4.5:1 的对（dark/light 各 9 色 18 个测，预计 1-3 个会爆），需要调 tokens.css —— 此时引入 `/ui-ux-pro-max` 做对比度优化建议
- **跟随**：land 后 `web-ui-polish` 在 a11y 基线之上做布局调件
