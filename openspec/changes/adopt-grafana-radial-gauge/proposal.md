## Why

MoleSignal 的 dashboard gauge 目前是一个只表达单值比例的 CSS 半圆原型，缺少阈值分段、刻度、响应式排版和稳定的数值展示。Grafana 已在 Apache-2.0 许可的 `packages/grafana-ui` 中提供成熟的 RadialGauge 原语，可以在不引入 AGPL 核心 panel 代码或 Grafana runtime 的前提下借鉴并改造成 MoleSignal 原生实现。

## What Changes

- 将 dashboard gauge 从内联 CSS 原型替换为独立、可测试的 SVG radial gauge 模块。
- 支持字段最小值/最大值、阈值颜色、阈值刻度、数值映射结果以及紧凑尺寸下的响应式降级。
- 保持现有 MoleSignal `DataFrame`、`FieldConfig`、dashboard schema 和查询执行链路不变。
- 固定参考来源为 Grafana v13.1.0，并记录 Apache-2.0 来源、修改范围和第三方声明。
- 不引入 `@grafana/ui`、`@grafana/scenes`、Grafana theme/runtime，也不复制 `public/app/plugins/panel/gauge` 下的 AGPL 代码。
- 为 gauge 的数学计算、颜色解析和组件渲染补充单元测试与可访问性断言。

## Capabilities

### New Capabilities

- `dashboard-gauge-visualization`: Dashboard gauge 的径向渲染、阈值语义、响应式行为和可访问性契约。

### Modified Capabilities

无。

## Impact

- 前端：`web/src/dashboard-engine/visualizations` 下新增独立 gauge 模块，并由 visualization registry 接入。
- 模型：复用现有 `FieldConfig.min/max/thresholds/mappings`，不改变持久化 schema 或 HTTP API。
- 依赖：不新增运行时 npm 依赖；继续使用 React、SVG 和 MoleSignal 主题 token。
- 合规：新增第三方来源说明，保留 Apache-2.0 归属；Grafana AGPL panel 源码明确排除在实现范围之外。
