## Why

`web-ui-polish` 把 9 色 token 在 dark/light 两个主题上调齐到 WCAG AA，但目前的"换肤"能力只有：
1. dark/light 二选一（StatusStrip 右侧的太阳/月亮按钮）
2. compact/comfortable 二选一密度（只在 palette 里有命令）

实际产品需要更多：用户能在多套**配色模板**（不只 dark/light，比如 "high-contrast"、"warm-amber"）之间切；颜色模式按钮要明显（不止藏 StatusStrip 角落）；多语言要可切（en/zh-CN 至少两种）。

另外 `web-ui-polish` rebase 后还有零星 light 模式残留黑色文字 / 边框（visual baseline 没覆盖到的 corner case，多是 inline style 或自定义组件用了硬编码 hex）。本 change 一起清掉。

## What Changes

- **多配色模板**：tokens.css 拆分 `--palette-*` 变量到 `tokens-default.css` / `tokens-high-contrast.css` / `tokens-warm.css`；ThemeBootstrap 加 `palette: 'default'|'high-contrast'|'warm'` state（zustand `useThemeStore`），与 `theme: dark|light` 正交
- **header settings menu**：StatusStrip 加一个 settings dropdown（齿轮图标）含 Theme(dark/light)、Palette(3 选)、Density(compact/comfortable)、Language(en/zh-CN) 4 个 sub-group；替代当前藏在 palette / 单按钮分散的入口（按钮本身保留）
- **i18n 装 `react-i18next`** + 落 2 套 locale（en/zh-CN）；UI 文案抽到 `web/src/locales/<lang>.json`；含 nav 项、key shortcut help、palette、状态文字、error toasts、a11y aria-label
- **黑色残留扫**：grep `text-black|#000|color:\s*black|border-black` 在 light 模式 visual baseline 上对 4 个 viz 子页面手动 review，把硬编码颜色换成 tokens（`text-foreground` / `border-border` 等）
- 不重做 9 色 token 数值（high-contrast palette 是另一套 9 色 alias，不动 default 的）
- 不引大 i18n CMS（locale json 手写）

## Capabilities

### New Capabilities

- `web-i18n`: 多语言基础设施 —— locale 加载 / 语言切换 / 缺词 fallback；与 a11y / 键盘地图集成
- `web-theming`: 配色模板（palette）+ theme + density 三维独立切换，CSS 变量分层规范

### Modified Capabilities

- `web-shell`: StatusStrip 右侧加 Settings dropdown 整合 4 个 UI 切换；ThemeBootstrap 接 useThemeStore 多状态

## Impact

- **代码**：`web/src/shell/tokens.css` 拆 + 加 2 个 palette；`web/src/shell/ThemeBootstrap.tsx`、`web/src/shell/StatusStrip.tsx`、`web/src/stores/useThemeStore.ts`（新）、`web/src/i18n/{index.ts,en.json,zh-CN.json}`（新）、各组件文案抽 `useTranslation()`
- **依赖**：`pnpm add react-i18next i18next`（核心 + react 绑定，~15 KB gzipped）
- **baseline**：visual + focus-ring 56 张 PNG 因 settings dropdown / 文字位置可能微调需 rebase；按 web-ui-polish 同样套路一次性更新
- **a11y**：language toggle 触发 `document.documentElement.lang` 同步；Settings dropdown 走 shadcn DropdownMenu 已通过 axe 验证
- **风险**：i18n 引入要求每段新 UI 文案走 `t()`，是后续 code review 关注点；palette 第三套配色（high-contrast / warm）也要过对比度脚本
- **跟随**：land 后 `web-admin-pages` 写新页面时直接用 i18n + tokens；后续 RTL 语言支持单独 follow-up
