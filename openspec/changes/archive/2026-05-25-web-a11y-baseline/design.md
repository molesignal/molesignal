## Context

`web-investigation-shell` 落了 `shell/tokens.css` —— 9 色 × 2 theme（dark/light）+ bg/muted/fg variant + 2 density 模式。其中 9 色 token 定义了：`bg surface primary accent red green yellow blue purple`，但**对比度仅靠 author 手感**，没有自动验证。

此前已经做了 `openspec validate --strict`（pure spec 校验），15.7 标记人工 a11y 抽样。本 change 把"对比度 + focus ring + 键盘地图 + axe critical"统一到一个 a11y baseline。

类比 `05-visual.spec.ts` 用 40 张 PNG 守护视觉回归 —— a11y 走同样套路但更窄：只截 focus ring + 单独算对比度。

## Goals / Non-Goals

**Goals:**
- WCAG 2.1 AA 对比度（4.5:1 正文，3:1 大字号）自动校验通过 / 报告残留差距
- 11 条认证路由 axe critical violation = 0
- 16 张 focus ring baseline PNG 稳定
- 全部 GLOBAL_KEYMAP binding 都有 e2e 覆盖
- a11y CI 失败时报告里直指"哪个 token 对哪个 token 不够 X.XX:1"

**Non-Goals:**
- 不做完整 WCAG AAA（7:1）—— AA 已足够 SRE 工具档位
- 不做屏幕阅读器 narrator 内容审计（要人耳听，不可自动）
- 不做色觉障碍模拟（color blindness simulation）— 单独 follow-up
- 不重做 tokens.css —— 若发现对比度不达标，本 change 只报告；调整 token 走 `web-ui-polish`

## Decisions

### D1：对比度用 `wcag-contrast` npm 包

成熟、零依赖；输入两个 hex 返 ratio。本 change 自己解析 `tokens.css` 出 9 色 hex，跟 fg/bg variant 笛卡尔积比对。

### D2：focus ring baseline 用 element-screenshot 不是 full-page

`page.locator(elementWithFocus).screenshot()` 只截焦点元素 + 8px padding，避免 layout shift 触发噪音。

### D3：键盘地图测试从 `GLOBAL_KEYMAP` 生成

`web/src/keyboard/bindings.ts::GLOBAL_KEYMAP` 已经是单一可信源（`dump-keymap.ts` 已用它生文档）。spec 文件用 `import { GLOBAL_KEYMAP }` + `for (const binding of GLOBAL_KEYMAP) test(...)` 自动展开，新 binding 自带测试。

### D4：axe 跑 11 条路由是宽 net

login / home / investigate / logs / metrics / traces / dashboards / alerts / streams / settings / noc。每条 `axe-core/playwright::analyze()` 跑全规则集；assert `critical = 0`。`moderate` / `minor` 不强制 0，报告里输出。

### D5：发现对比度不达标 → 不在本 change 修

本 change 是"守护"而非"修复"。若 dark/yellow on dark/bg 不达 4.5:1，报告里指出，**留给 `/ui-ux-pro-max` 在 `web-ui-polish` 里调 token**。

## Risks / Trade-offs

**[R1] axe-core 在 11 条路由跑慢（~30s）**
→ Mitigation：playwright `--workers=4` 并行；fixture mock backend 即时应答；预计单条 < 3s。

**[R2] 对比度算法选择**
→ Mitigation：用 `wcag-contrast` 标准实装，避免 ad-hoc YIQ 公式。

**[R3] focus ring 在不同 theme 颜色不同（accent 不同）→ baseline 翻 4 倍**
→ Mitigation：可以接受；16 张 PNG ≈ 4 MB，不算大。

**[R4] 键盘地图生成测试 race condition**
→ Mitigation：每个 binding 测用 `await page.keyboard.press(...)` + 显式等下一帧 `await page.waitForLoadState('domcontentloaded')`。
