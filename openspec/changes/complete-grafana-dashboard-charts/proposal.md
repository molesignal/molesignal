## Why

除 `time_series` 和已经完成的 radial gauge 外，MoleSignal 的 dashboard 图表仍是集中在单个超限文件中的 DOM/CSS 原型：缺少真实尺度、负值基线、时间宽度、响应式排版、稳定颜色和可访问语义。现在需要一次性收口剩余图表，让 dashboard editor 中已经暴露的 visualization 类型具备一致、可测试的生产级行为。

## What Changes

- 将 `stat`、`bar_gauge`、`bar_chart`、`heatmap`、`state_timeline` 从 `visualizations.tsx` 拆为独立模块。
- 为数值 reduction、范围归一化、阈值区间、稳定系列颜色和容器尺寸建立共享 primitives，并让 gauge 复用其中适合的能力。
- 基于 Grafana v13.1.0 Apache-2.0 `BigValue`、`BarGauge`、`Sparkline` 的布局与数值比例思路改造 Stat 和 Bar Gauge，不引入 Grafana runtime。
- 为 Bar Chart 实现类别/系列建模、正负值零基线、横纵方向、分组宽度、轴标签和数值标签。
- 为 Heatmap 实现按字段组织的矩阵、有限列聚合、全局色阶、行/时间标签和空值语义。
- 为 State Timeline 实现基于真实时间跨度的区段、连续状态合并、稳定状态颜色、值标签和图例。
- 补充响应式、双主题、empty state、可访问性、数学边界与 DataFrame 集成测试。
- 将新图表完整接入既有 `DashboardRenderer`、Dashboard 编辑器 live preview 和公开分享链路，补齐稀疏持久化 options、类型切换以及 loading/error 状态。
- 移除 Dashboard 编辑器独立的 `Layout` 假预览页面；进入编辑路由后直接在真实 `DashboardRenderer` 画布上选择、拖动、调整尺寸和打开 panel editor。
- 将 Dashboard 变量的 Query/options 原始 JSON 编辑器替换为 Query type 选择框和类型对应的结构化字段。
- 将 Dashboard settings 收口为 `General`、`Variables`、`Annotations`、`Links` 四个结构化页面，移除 `JSON model`；同时把 annotation、transformation、data link、visualization option、field config 和 override 中残留的 JSON 文本区替换为对应控件。
- 扩展第三方声明，明确 Apache-2.0 来源和未使用的 AGPL panel 源码范围。

## Capabilities

### New Capabilities

- `dashboard-chart-visualizations`: Dashboard Stat、Bar Gauge、Bar Chart、Heatmap 和 State Timeline 的数据语义、渲染、响应式及可访问性契约。

### Modified Capabilities

无。

## Impact

- 前端：`web/src/dashboard-engine/visualizations/` 下新增共享模块和五个图表专属目录，registry 改为导入独立实现；既有 Dashboard 查看、原位编辑和分享表面继续共用该 registry，编辑路由不再维护独立的 Layout 渲染器。
- 兼容性：visualization type、持久化 schema、查询协议和既有 options 保持兼容；新增 options 均提供默认值。
- 依赖：不新增 npm 运行时依赖，不引入 `@grafana/*`。
- 合规：只改造 Grafana `packages/grafana-ui` Apache-2.0 代码思路；Bar Chart、Heatmap、State Timeline 使用 MoleSignal 原创实现，不复制 Grafana AGPL 核心 panel。
