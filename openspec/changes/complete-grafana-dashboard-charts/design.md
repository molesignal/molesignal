## Context

Dashboard engine 已经拥有统一的 `DataFrame -> transformations -> FieldConfig -> visualization` 链路，`time_series` 使用成熟的 uPlot 实现，`gauge` 已拆为独立 SVG 模块。其余五种图表仍内联在 800 行以上的 `visualizations.tsx` 中，并且只使用简单 flex/grid 比例表达数据，无法正确处理负值、真实时间跨度、多系列、空值或小尺寸 panel。

Grafana v13.1.0 的 `packages/grafana-ui` 位于官方 `LICENSING.md` 声明的 Apache-2.0 例外范围内，其中 `BigValue`、`BarGauge`、`Sparkline` 和通用 uPlot wrapper 可作为布局与数学参考。Grafana 的 Bar Chart、Heatmap、State Timeline 完整 panel 位于 AGPL 核心目录，本设计明确不读取或复制其实现，只按用户可观察的产品语义进行 MoleSignal 原创实现。

视觉继续遵循 MoleSignal “confident quiet” token 系统和 Dieter Rams / Functionalist 方向：数值、尺度和状态优先；颜色只编码系列或状态；无渐变、阴影、装饰动画及 focus 外框。

## Goals / Non-Goals

**Goals:**

- 一次性完成 `stat`、`bar_gauge`、`bar_chart`、`heatmap`、`state_timeline` 的生产级实现。
- 将五类图表从超限聚合文件拆入职责明确、每个生产文件不超过 500 行的专属目录。
- 统一 finite-number reduction、范围归一化、阈值、颜色、时间单位和响应式尺寸行为。
- 保持现有 `DataFrame`、`FieldConfig`、visualization type 和 dashboard schema 兼容。
- 为每类图表提供可访问摘要，同时不让大量装饰图元污染 accessibility tree。
- 固定 Grafana Apache 来源版本、文件和本地修改，并排除 AGPL panel 源码。

**Non-Goals:**

- 不替换已经成熟的 `time_series`，不重复改造刚完成的 radial gauge 视觉。
- 不改造 `table`、`logs`、`text` 内容面板。
- 不复制 Grafana panel editor 的源码或像素外观，不实现 cross-panel hover、canvas tooltip 或 drag selection 框选。
- 不引入 `@grafana/*`、Emotion、tinycolor、D3 chart runtime 或新的 npm 依赖。
- 不追求 Grafana 像素级复刻，也不采用 Grafana theme 或品牌外观。

## Decisions

### 1. 建立共享 model primitives，但让每个 visualization 保持独立

`visualizations/shared` 负责纯数据能力：reduction、稳定范围、阈值区间、时间归一化、稳定颜色和 container measurement。每个 visualization 目录包含自己的 model 与 presentational component，registry 只导入公开入口。

替代方案是继续在 `visualizations.tsx` 中共享私有函数，但该文件已超限且会持续形成循环依赖；另一个替代方案是建立通用 “万能图表组件”，但五类图表的数据语义差异过大，会制造布尔参数矩阵。因此选择小型共享 primitives + 独立垂直模块。

### 2. Apache 组件只改造布局和数学，不复制依赖图

Stat 借鉴 BigValue 的 wide/stacked breakpoint、文字层级和可选 sparkline；Bar Gauge 借鉴范围比例、横纵布局和 title/value 空间分配；Sparkline 借鉴数值在背景图层中的定位。所有代码改写为 React、SVG、CSS token 和 MoleSignal contracts。

第三方 notice 增加准确 tag、commit、source paths、Apache-2.0 许可和修改说明。Bar Chart、Heatmap、State Timeline 标记为原创实现；不访问或复制 `public/app/plugins/panel/*`。

### 3. 使用 DOM/SVG 混合渲染而不是新增 canvas runtime

Stat、Bar Gauge、Heatmap、State Timeline 使用语义清晰的 DOM/SVG；Bar Chart 使用 SVG，因为它需要统一坐标系、零基线和轴。可见数据量在 model 层受控：Bar Chart 最多呈现 120 个 category，Heatmap 将超过 120 列的数据聚合，Timeline 合并连续状态。

相比 canvas，这种选择更容易测试、支持 native tooltip 和双主题；相比直接用 uPlot，自定义类别、矩阵和状态区段不需要插件生命周期。大规模数据通过限制/聚合而不是无限 DOM 节点解决。

### 4. Stat 采用 responsive BigValue 语义

Stat 对每个 numeric field 执行配置的 reduction，使用 `formatFieldValue` 保留 unit、decimals、mapping 和 threshold color。单个 tile 根据真实容器宽高选择 wide 或 stacked 布局；多值使用 auto-fit grid。`textMode` 保持 `value`、`value_and_name`、`name` 兼容；新增 `graphMode`、`colorMode`、`showPercentChange` 都有默认值。

Sparkline 仅在至少有两个有限点且空间足够时显示。Percent change 使用首末有限值，首值为零时不显示，避免无限值。

### 5. Bar Gauge 每个字段拥有独立、稳定范围

每个 reduced field 优先使用自己的显式 min/max；缺省范围沿用 gauge 的稳定归一化规则。活动长度钳制到 range，但显示文本保留真实值。横向和纵向布局均呈现名称、值、未填充轨道和可选 threshold markers，并使用原生 `meter` 语义。

不移植 Grafana gradient/LCD 模式；`displayMode` 只提供 functionalist 的 `basic` 和 `thresholds`，前者使用活动值颜色，后者强调阈值区间。

### 6. Bar Chart 先构造 category/series model，再计算 SVG geometry

有 string、enum、boolean 或 time field 时，该字段作为 category，所有 numeric fields 成为 series；多个 frame 按 category label 合并。无 category 时，每个 numeric field 的 reduced value 成为一个 category。domain 总是包含零，从而正确表达正负值；相等 domain 会扩展。

横纵方向共享同一 model，但分别计算坐标。稳定 series color 来自 MoleSignal chart token；FieldConfig fixed color 优先。`groupWidth` 被钳制，`showValues` 支持 auto/always/never。过多 category 保留最近 120 个并允许滚动。

### 7. Heatmap 表达“series × sample”矩阵

每个 numeric field 形成一行，字段值形成列。超过 120 列时按连续窗口计算有限值均值，避免直接截断丢失整体分布；空窗口保持 null。所有行共享全局 finite min/max，单值范围使用中等强度而不是除零。

颜色方案映射到现有 semantic/chart tokens，并通过 opacity 表达强度，不使用 CSS gradient。行名、首末时间/索引和 native cell title 保留可读上下文。

### 8. State Timeline 使用真实 duration，而不是等宽数组项

Timeline 优先使用 frame 的 time field，将微秒/毫秒/秒统一为 epoch seconds；没有可用时间时回退到 index。每个 state 的结束时间来自下一点，最后一点使用中位采样间隔。`mergeEqual` 按格式化状态和值颜色合并连续区段。

区段位置按全局 domain 百分比计算；mapping/threshold color 优先，缺省使用稳定 chart color。`showValues` 根据区段可见宽度决定文字，图例去重并限制为八项。

### 9. Backward compatibility 通过 registry defaults 完成

既有 option key 和语义继续生效：`calculation`、`textMode`、`orientation`、`groupWidth`、`colorScheme`、`mergeEqual`、`showValues`。新 option 只通过 defaultOptions 补充，不提升 schemaVersion，也不需要持久化迁移。`avg` 继续作为 `mean` 的兼容别名。

### 10. 既有 Dashboard 表面统一从 registry 解析图表状态和 options

Dashboard 查看页、编辑器 live preview、全屏 panel 和公开分享页已经收敛到 `DashboardRenderer -> VisualizationRenderer`，因此集成不新增平行入口。`VisualizationRenderer` 统一将持久化 options 与当前 plugin defaults 合并，使旧 Dashboard 的稀疏配置立即获得新增默认值；编辑器使用同一份有效 options，确保所有可配置项可见。

查询缓存身份只包含会改变数据请求或返回行的数据属性。`legend` 属于 presentation-only 配置，不进入 query key；缓存中的原始 DataFrame 在渲染前按当前 Legend 模板重新命名。这样输入别名时图例即时变化，但不会进入 loading、重发请求或重建 uPlot 实例。

Prometheus 查询的 Legend 编辑器采用 Grafana 的 `Auto / Verbose / Custom` 三态语义。`Auto` 持久化为 `__auto` 并只显示各返回系列之间不相同的 labels；缺失或空值保持旧 Dashboard 的 `Verbose` 行为，显示全部 label 名称和值；`Custom` 持久化用户模板并即时应用 `{{label_name}}` 替换。选择 Custom 时先填入可直接覆盖的 `{{label_name}}`，清空模板回到 Auto。三种模式都只改变缓存 DataFrame 的 display name，不触发数据查询。

Time series 的 Legend 计算项采用 Grafana `Values` 控件语义，而不是逗号分隔文本。编辑器以可勾选的多选菜单展示 `Last`、`Min`、`Max`、`Mean` 和 `Total`，触发器内用标签呈现已选项；`Total` 继续持久化为既有 `sum` 值，保证 Dashboard JSON 兼容。每次选择直接更新 `legendStats` 数组，生产预览通过现有 options 链路同步刷新，不触发查询。

切换 visualization type 时只迁移目标 plugin 明确支持的同名 option，避免 `time_series`、Stat、Bar Chart 等类型互相遗留无效字段。查询尚未返回 frame 时统一显示非交互 loading 状态，查询失败时统一显示 alert 语义；已有 frame 的刷新继续保留图表内容。真实 `DashboardRenderer` 集成测试通过 panel query executor 注入 DataFrame，覆盖 registry 选择、transform/field config 后的数据和最终图表语义。

### 11. Dashboard 编辑采用真实画布原位模式

`/dashboards/:id/edit` 默认继续渲染生产 `DashboardRenderer`，并在根级 grid item 外增加轻量编辑控件。编辑控件只负责选择、拖动、调整尺寸和打开具体 panel；查询、变量、transform、field config、图表 registry 与 view 页面保持同一条运行链路。拖动中的 grid position 通过临时 Dashboard definition 驱动真实画布，pointer interaction 完成后才写入 editor history。

具体 panel 仍通过 `?panel=<id>` 进入查询与 visualization 配置界面，返回时清除该参数并回到真实 Dashboard 画布。编辑器不再维护 `layout | panel` tab 状态，也不再渲染静态 bar 占位图。结构元素 inspector 可以作为原位画布旁的上下文面板保留，但不是独立页面。

把 pointer interaction 和 grid item chrome 拆入 `dashboard-engine/editor/` 专属目录，避免继续扩大已有超限 `DashboardEditor.tsx` 和 `DashboardRenderer.tsx` 的职责。选中态使用背景/图标色，不使用 ring、shadow 或高对比度 focus 外框。

Panel 不显示独立的拖动按钮。编辑模式通过 panel title bar 这一整条稳定表面启动移动，标题栏内的菜单、链接和其他交互控件必须排除在拖动热区之外；没有跨越 grid step 的单击不会提交 layout history。resize 继续使用右下角控件，避免把移动和尺寸调整混在同一手势中。

### 12. Query 变量使用结构化选择器而不是 JSON 文本区

Dashboard settings 的变量编辑器不再直接暴露 `query` record。Query type 选择框提供 `Label values`、`Classic query` 和 `SQL`：Label values 使用 Metric 与 Label 字段生成既有 `label_values(metric, label)` expression；Classic query 保留导入 Dashboard 的表达式编辑能力；SQL 提供 query、stream name 和 stream type。

持久化继续使用既有 `query.expression`、`query.kind`、`query.streamName` 和 `query.streamType`，并只增加可选的 `queryType`、`metric`、`label` 编辑提示。旧 Dashboard 根据 `kind` 和 expression 自动推断选择项，不需要 schema migration；运行时 variable resolver 继续只读取原有字段。

### 13. Dashboard 配置只提供领域结构化控件

Dashboard settings 只保留 `General`、`Variables`、`Annotations` 和 `Links` 四个用户页面，不再把完整 Dashboard JSON model 暴露为可编辑 tab。JSON 序列化、校验和下载能力仍作为内部模型与导出能力保留，但配置工作流不要求用户理解持久化结构。

Annotation query 使用事件列表编辑时间、结束时间、标签和标识，并在更新 `query.items` 时保留 query record 中未知的 provider 字段。Panel data link variables 使用 key/value 行；transformation options 按 transformation type 显示字段、枚举、数字、列表或重命名映射；field thresholds、value mappings 和 override properties 使用各自的类型化编辑器。Visualization 的内置 options 继续按 primitive、枚举和集合类型编辑，导入但无法识别的嵌套 option 只提示已保留，不再降级为 JSON 文本区。旧 Dashboard 中未被当前控件识别的 record key 在修改已知字段时原样保留，避免 schema migration 或导入数据丢失。

这些控件都沿用现有 token、紧凑层级和无 focus 外框规则。固定枚举采用选择框，集合采用可增删行或多选，数值采用 number input；不以字符串化 JSON 作为兜底输入。

## Risks / Trade-offs

- **SVG/DOM 在极端数据量下节点过多** → Bar Chart 限制 category，Heatmap 聚合列，Timeline 合并连续状态并限制图例。
- **多个 frame 的 category label 冲突** → 相同显示 label 按同一 category 合并；series id 保持 field identity，tooltip 仍显示 series 名。
- **窄 panel 标签拥挤** → container measurement 驱动 compact 模式，次要标签隐藏，完整信息保留在 accessible name/native title。
- **状态值包含复杂对象** → 使用稳定序列化 key 与 `formatFieldValue` 文本；无法序列化时回退 `String(value)`。
- **本地实现与 Grafana 后续版本分叉** → 固定 v13.1.0/commit，纯函数测试覆盖边界；升级必须显式重新审计许可和行为。
- **颜色 token 用于 SVG/DOM 时无法直接混色** → 使用现有 token + opacity，不在运行时解析或生成渐变。
- **拖动时真实 panel 重新布局导致查询抖动** → 查询 key 不包含 grid position，拖动只替换临时 layout definition，pointer up 后再提交一次 history。

## Migration Plan

1. 新增 shared primitives 和测试，并让 radial gauge 复用通用范围/阈值实现。
2. 按 Stat、Bar Gauge、Bar Chart、Heatmap、State Timeline 顺序新增独立模块和 focused tests。
3. registry 切换为模块导入，删除聚合文件中的旧实现和不再使用的 helper。
4. 更新第三方 notice，执行 focused tests、TypeScript typecheck、touched-file lint、依赖与文件长度审计。
5. 在既有 DashboardRenderer 和编辑器链路中补齐有效 options、类型切换、运行状态与端到端组件集成覆盖。
6. 用真实 `DashboardRenderer` 替换独立 Layout 预览，并在 renderer 根级 grid 上接入原位选择、拖动和 resize 控件。
7. 移除 Dashboard JSON model 设置页，并将所有现存 JSON 配置文本区替换为保持持久化兼容的结构化编辑器。

不涉及后端、数据迁移或发布顺序。回滚只需恢复 registry 的旧组件引用；dashboard JSON 不变。

## Open Questions

无。交互式 tooltip、stacked bar、heatmap bucket schema 和跨 panel hover 留作独立后续能力，不阻塞本次完整基础实现。
