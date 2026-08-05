## Context

MoleSignal 当前的 `gauge` visualization 直接实现在 `web/src/dashboard-engine/visualizations.tsx` 中，使用两个 CSS 半圆近似表达进度。它只能展示一个数值和单色比例，无法准确表达阈值区间、边界刻度、超界值和窄面板布局；同时该文件已经超过项目规定的生产文件长度上限。

Grafana v13.1.0 的 `packages/grafana-ui/src/components/RadialGauge` 提供了成熟的 SVG 弧线几何和布局思路，该目录位于 Grafana 明确列出的 Apache-2.0 许可例外范围内。Grafana 的核心 gauge panel 位于 `public/app/plugins/panel/gauge`，受 AGPL-3.0 约束，不在本次改造范围内。

本次工作只借鉴并改造 Apache-2.0 组件中的通用几何思想，继续使用 MoleSignal 自有的 `DataFrame`、`FieldConfig`、主题变量、国际化和 visualization registry，不引入 Grafana runtime、theme、data model 或 React package 依赖。

## Goals / Non-Goals

**Goals:**

- 将 gauge 从超限的聚合文件中拆为职责清晰、可单测的独立模块。
- 使用 SVG radial arc 准确呈现数值比例、活动阈值颜色和阈值区间。
- 复用现有 `FieldConfig` 的 `min`、`max`、`thresholds`、`mappings`、`unit` 和 `decimals` 语义。
- 在小尺寸 panel 中保持可读，不溢出、不遮挡核心数值。
- 提供稳定的可访问名称，并遵守项目禁止 focus 外框的视觉规范。
- 固定上游来源为 Grafana v13.1.0，并记录来源、许可证和本地改动。

**Non-Goals:**

- 不移植 Grafana 的核心 gauge panel、panel editor 或 scene runtime。
- 不引入 `@grafana/ui`、`@grafana/data`、`@grafana/runtime` 或 `@grafana/scenes`。
- 不在本次切片中支持多 gauge 网格、渐变填充、发光、分段条、可拖拽编辑或动画。
- 不改变 dashboard 持久化 schema、查询协议或后端 API。
- 不让 MoleSignal 的主题和交互视觉变成 Grafana 的外观。

## Decisions

### 1. 采用本地 SVG primitive，而不是直接依赖 `@grafana/ui`

新模块使用固定 `viewBox` 和百分比宽高绘制轨道、活动弧和可选阈值环。弧线路径、角度归一化与端点计算基于 Grafana v13.1.0 Apache-2.0 RadialGauge 的实现思路重写，并保留清晰的来源说明。

直接安装 `@grafana/ui` 会带入 Grafana theme、icons、i18n 和 data contracts，使 bundle、升级和品牌边界复杂化；复制核心 panel 则触及 AGPL 代码。因此选择无新增 npm 依赖的本地 primitive。

### 2. 使用独立的 `visualizations/gauge` 目录划分职责

- `geometry.ts` 负责范围归一化、比例钳制、弧线路径和阈值区间计算。
- `RadialGauge.tsx` 只负责 SVG 呈现和可访问语义。
- `GaugeVisualization.tsx` 负责从 MoleSignal `PanelData` 选择字段、执行 reduction、格式化值并适配 visualization contract。

这种结构同时消除 `visualizations.tsx` 中的 gauge 独立职责，避免在超限文件中继续堆叠实现。

### 3. 保持 MoleSignal 的数据和字段配置语义

Gauge 默认选择第一个包含有限数值的字段，并按 panel option `calculation` 执行现有 reduction 语义。显示文本和活动颜色通过现有 `formatFieldValue` 获取，从而延续 value mapping、unit、decimals 和 threshold color 行为。

范围优先使用字段的显式 `min` / `max`；缺省值为 `0` 和 `max(100, value)`。反向范围会被交换；相等范围会围绕该值扩展至少 1 个单位，避免除零。活动弧比例始终钳制到 `[0, 1]`，但文本保留真实值。

### 4. 阈值环与活动弧分离

活动弧使用当前值对应的 threshold color；外层细环按所有 threshold step 绘制区间。absolute threshold 直接使用配置值，percentage threshold 先按归一化后的 gauge 范围换算为实际值，再钳制和排序。

`showThresholdMarkers` 控制阈值环，默认开启；`showThresholdLabels` 控制边界标签，默认关闭。标签只在高度足够时呈现，核心数值始终优先。

### 5. 以固定坐标系实现响应式和可访问性

SVG 使用固定坐标系、`width="100%"`、`height="100%"` 和 `preserveAspectRatio="xMidYMid meet"`，因此不需要 `ResizeObserver`，也不会引入布局抖动。组件根据传入 panel 高度进入 compact 模式，隐藏字段名和阈值标签等次要信息。

根 SVG 使用 `role="img"`，可访问名称包含字段名、格式化值和范围。图形路径设为装饰性内容，不增加键盘焦点，也不引入 outline、ring、shadow 或高对比度 focus 边框。

### 6. 兼容现有 dashboard 数据

新增 option 都有默认值，现有 gauge panel 不需要迁移。registry 仍注册为 `gauge`，`optionSchemaVersion` 不变；旧 panel 会在默认值合并后自动获得新的 SVG 表现。

## Risks / Trade-offs

- **与上游实现逐渐分叉：** 本地代码不会自动获得 Grafana 后续修复。通过固定 v13.1.0 来源、单元测试几何边界和显式修改说明降低风险。
- **标签在小尺寸下信息减少：** compact 模式会隐藏次要标签，但保留可访问名称中的完整信息，并优先保证数值可读。
- **复杂 threshold 配置可能拥挤：** 本次仅显示边界标签，不做碰撞布局；高度不足时整体隐藏标签。后续可在独立模块内演进，而不影响数据层。
- **SVG 文本与浏览器字体度量存在差异：** 使用保守的固定坐标字号和截断策略，测试关注结构和模式切换，不依赖像素级字体测量。

## Migration Plan

1. 新增 gauge 模块和 focused tests。
2. registry 改为导入新 `GaugeVisualization`，删除旧 CSS gauge 实现。
3. 添加第三方来源与 Apache-2.0 notice。
4. 运行 focused tests、TypeScript typecheck 和 touched-file lint。

不涉及持久化数据迁移或后端发布顺序。若需要回滚，只需恢复旧 visualization 实现；已保存 dashboard 不受影响。

## Open Questions

无。本切片明确排除多 gauge、渐变、动画和编辑器定制，这些能力应在本切片验证通过后分别评估。
