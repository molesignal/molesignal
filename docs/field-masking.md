# 字段遮掩

字段遮掩在后端查询返回边界执行。MoleSignal 仍保存原始值，并使用原始值完成过滤、排序、聚合、告警计算与内部派生；客户端、Flight SQL、异步搜索结果和共享查询拿到的是遮掩后的最终结果。

Metrics 指标流不参与字段遮掩，包括 `_molesignal` 指标流。PromQL、Prometheus exemplar 和通过 SQL 查询的 metrics 结果均保持原有指标值与标签；指标字段仍可配置索引和提取。

## 规则优先级

全局规则位于“设置 → 数据安全 → 字段遮掩”，按列表顺序匹配，第一条同时命中字段名、可选数据流名称和可选数据流类型的启用规则生效。字段名和数据流名称支持精确值以及 `*`、`?` 通配符；未指定类型表示所有非 Metrics 类型。

数据流字段配置优先于全局规则：

- `inherit`：使用首条命中的全局规则，没有命中时不遮掩；
- `custom`：对该流的字段使用独立算法；
- `none`：明确不遮掩，并跳过当前及未来的全局规则。

普通数据流配置需要 `streams.configure`，全局规则变更需要 `org.settings.manage`。系统 `_molesignal` 数据流由 `sys.telemetry.manage` 管理；traces/profiles 允许修改字段索引、提取和遮掩，metrics 只允许索引与提取。名称、类型、加密属性、保留策略和删除操作仍不可变。

## 算法

- 全遮掩：整个值替换为固定内容；
- 区间遮掩：替换 `[start, end)` 字符区间；
- 内部遮掩：保留指定数量的首尾字符，替换中间部分；
- 外部遮掩：只保留 `[start, end)` 字符区间，替换两侧；
- 哈希：使用后端根密钥派生组织级密钥，并生成确定性的 HMAC-SHA-256 十六进制摘要。

字符位置按 Unicode 字符而非 UTF-8 字节计算。`null` 保持为 `null`。SQL 派生字段和别名会继承其引用字段的遮掩策略；多流查询发生冲突时采用更保守的遮掩结果。

## 接口

- `GET/POST /api/v1/field_masking/rules`
- `PUT/DELETE /api/v1/field_masking/rules/{id}`
- `PUT /api/v1/field_masking/rules/reorder`
- `GET /api/v1/field_masking/effective/{stream_id}`

Fields 模式只为未遮掩字段生成值补全；字段名、运算符和内置函数补全不受影响。
