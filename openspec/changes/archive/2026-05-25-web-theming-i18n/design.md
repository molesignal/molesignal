## Context

`web-ui-polish` 调好 dark/light 两主题 + compact/comfortable 两密度的 token 数值，但开关入口分散：
- theme 在 StatusStrip 角落小按钮
- density 只在 ⌘K palette 里
- 无 palette template 概念
- 无 i18n（UI 全英文硬编码）

产品要求：让用户在 header 显眼位置一次性切换 theme/palette/density/language；并补 zh-CN（初版）；并清掉 light 模式偶尔露黑色的 corner case。

## Goals / Non-Goals

**Goals:**
- 用户在 header 一个 settings dropdown 里能切 theme/palette/density/language 四个维度
- i18n 装 react-i18next，en/zh-CN 两套 locale，全部 UI 文案过 `t()`
- 3 套 palette 都过 `pnpm a11y:contrast`（default 已绿；新增 high-contrast / warm 一并测）
- light 模式黑色残留全清，visual baseline 重 rebase 后稳定

**Non-Goals:**
- 不引 i18n CMS / 工具链（手写 json + 编辑器对照）
- 不做 RTL（阿拉伯 / 希伯来语）—— ar/he locale 留 follow-up
- 不上线"用户自定义 palette"（仅 3 个预设）
- 不重做 dark/light token 数值

## Decisions

### D1：palette × theme 正交分层

```
:root {
  /* layer 0: 物理色（palette 决定） */
  --palette-bg-0: ...;
  --palette-orange: ...;
  ...
}
/* layer 1: 语义色（theme 决定，引用 layer 0） */
[data-theme="dark"] { --bg: var(--palette-bg-0-dark); ... }
[data-theme="light"] { --bg: var(--palette-bg-0-light); ... }
```

切 palette 换 layer 0 的 css `<link>`，切 theme 改 `data-theme` 属性。两者独立。

### D2：i18n 用 react-i18next

成熟、与 React 习惯一致；plugin 生态足够；与 react-router / suspense 兼容。bundle 影响小（核心 ~13KB gzip）。备选 `@lingui/react` 更现代但要 babel/swc 插件改造，对现项目太重。

### D3：locale 数据结构按 feature 切片

```
web/src/i18n/
  index.ts
  en/
    common.json
    palette.json
    keyboard.json
    alerts.json
    ...
  zh-CN/
    common.json
    ...
```

按 feature 分文件，避免一个 5000 行的大 json；命名空间用 `t('common.cancel')` / `t('palette.placeholder')`。

### D4：language 持久化 + a11y 联动

`useThemeStore.language` 持久到 localStorage `molesignal-lang`；切换时 `document.documentElement.lang = newLang` 同步给屏幕阅读器。SystemLang 检测：首次加载用 `navigator.language` 猜测 zh-CN / en，用户切了之后 lock。

### D5：黑色残留清扫策略

不靠人工抽样：
1. 在 light theme 截 8 张关键页面（4 viz + 4 admin/route）→ 与 dark 同视图对比，找"两图都黑"的元素（dark 看不出黑色 vs 黑底；light 看就刺眼）
2. grep `color:\s*black|#000|text-black|border-black|bg-black` 在 `web/src/**/*.tsx` 输出 hit list
3. 每个 hit 改 token 引用（`text-foreground` `bg-bg` `border-border` 等）

修完重 rebase 56 张 baseline。

## Risks / Trade-offs

**[R1] i18n 抽文案是高接触面改动（几乎每个 .tsx）**
→ Mitigation：分 PR / 分组提交（chrome / palette / routes / errors 4 批）；先抽公共文案 `common.json`，feature 文案随用随抽

**[R2] palette 切换有可能因 css `<link>` 加载延迟造成 FOUC**
→ Mitigation：3 个 palette `.css` 在 build 时全部内联到 dist，运行时根据 `<html data-palette=...>` 选 ruleset 不重加载文件

**[R3] zh-CN 翻译初版会有不达意词**
→ Mitigation：本 change 给 en/zh-CN 都"覆盖"完整但允许 zh-CN review 后续单独 PR 修；非阻塞 land

**[R4] 黑色残留扫漏**
→ Mitigation：a11y-focus-ring 16 张 baseline 已覆盖 light 模式 viz；StatusStrip 颜色变了会被 visual baseline diff 抓；剩下的人工 review 期可补
