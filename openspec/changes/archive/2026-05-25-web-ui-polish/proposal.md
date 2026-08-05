## Why

前 3 个前端 change（typescript-strict-fixes / playwright-runtime / a11y-baseline）落地了**结构性**质量：类型干净 / e2e 跑得通 / a11y 有守护。但**视觉与交互层面**还停在 web-investigation-shell M1 草稿：

- 9 色 token 中有若干对在 dark/light 不达 WCAG 4.5:1（a11y-baseline 报告会列出来）
- ⌘K palette 选中项渲染密度过紧，subtitle 在 compact 模式被截
- DrawerFrame 抽屉 cascade pop 没有过渡动画，多层叠看不清边界
- TimeSeriesPlot brush 选区视觉不够明显
- Topology DOI 着色阶梯不平滑，0.05 阈值附近 red ring 闪烁
- StatusStrip "今" 与 anchor 距离视觉不够好
- 32 viewports × theme/density baseline 抽样视觉一致性 review

本 change 不改架构，专注上述视觉 + 交互细节；**所有布局/颜色/动画决策都通过 `/ui-ux-pro-max` skill 评审**，落实后通过既有 visual + a11y baseline 守护。

## What Changes

- **修对比度不达标的 token**（依据 `web-a11y-baseline` 报告）：`/ui-ux-pro-max` 给方案 → 修 `tokens.css` → 重跑对比度脚本验证。预计 1-3 对修改。
- **CommandPalette 行渲染**：item icon + label + subtitle + kind chip + shortcut 五元素在 compact 模式下重排，subtitle 改 ellipsis-after-2-lines；ui-ux-pro-max 给排版建议
- **DrawerFrame cascade 动画**：用 framer-motion（轻包）或 CSS transition 给 push/pop 加 200ms slide；不破"≤ 6 帧"约束
- **TimeSeriesPlot brush 高亮**：选区背景 `accent / 12% alpha`；shift+drag 时显示一行 "Brush: ts1 → ts2" inline label
- **Topology DOI 平滑**：用 CSS conic gradient 或 d3-interpolate 让 0%-100% err_rate 颜色渐变；red ring 阈值从硬 `>= 0.05` 改 `>= 0.045` 加 50ms hysteresis 防抖
- **StatusStrip 间距 + anchor 视觉**：分隔符从 `|` 改 4px dot；anchor 📌 增 `min-width: 12ch` 防跳
- **重跑 visual baseline**：40 张 PNG 因 token 调整会变；本 change 末尾 `--update-snapshots` 一次性更新

不动键位、不动 store、不引重型 UI 库（仅可能加 framer-motion）。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `web-shell`: tokens.css 调整（颜色对比度）+ shell 间距规则
- `web-command-palette`: 行渲染密度调整
- `web-investigation-stack`: drawer cascade 动画
- `web-timeseries`: brush 视觉
- `web-topology`: DOI 平滑 + red ring 阈值
- `web-time-anchor`: anchor 显示 min-width

## Impact

- **代码**：`tokens.css` + `CommandPalette.tsx` + `DrawerFrame.tsx` + `TimeSeriesPlot.tsx` + `ServiceNode.tsx` + `ServiceEdge.tsx` + `StatusStrip.tsx` 7 个文件
- **依赖**：可能加 `framer-motion` 或 `@react-spring/web`（待 ui-ux-pro-max 评审决定）
- **baseline**：40 张 visual + 16 张 focus-ring 共 56 张 PNG 因 token + 间距调整需要 `--update-snapshots` 一次性 rebase
- **风险**：低 —— token 改动有 a11y-baseline 的对比度脚本守护；其他改动有 visual baseline 守护
- **跟随**：本 change land = 前端 1.0 视觉/交互正式签字
