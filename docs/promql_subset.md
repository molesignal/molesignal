# PromQL 支持范围

MoleSignal 的 PromQL 引擎（`src/infra/query/promql/`）基于 `promql-parser`
解析 + 自研 evaluator over Arrow / DataFusion，支持 instant 与 range 两类求值。
本文列出当前支持的函数、运算与已知差异，便于从 Prometheus / VictoriaMetrics
迁移时校准期望。

> instant query 在 `time_range.end` 处求值；range query 在 `[start, end]` 按 step
> 步进，每步对其前置窗口求值，结果合成 matrix。

## 支持的函数

### rate 家族（输入 `metric[range]`）

`rate` · `irate` · `increase`（counter reset 自动检测）

### range-vector 派生（输入 `metric[range]`）

`delta` · `idelta` · `deriv` · `predict_linear(v[range], t)` · `resets` · `changes` ·
`holt_winters(v[range], sf, tf)`（别名 `double_exponential_smoothing`）

### `*_over_time`（输入 `metric[range]`）

`avg` · `min` · `max` · `sum` · `count` · `quantile` · `stddev` · `stdvar` · `last` ·
`present` · `mad`（各 `_over_time`）

### 聚合算子（支持 `by` / `without`）

`sum` · `avg` · `min` · `max` · `count` · `stddev` · `stdvar` · `quantile(φ, v)` ·
`group` · `count_values` · `topk` · `bottomk` · `limitk` · `limit_ratio`

### 直方图（classic bucket）

`histogram_quantile(φ, v)` · `histogram_fraction(lower, upper, v)`

> 输入要求是按 `le` 标签分桶的 cumulative bucket，典型形态
> `sum by(le)(rate(metric_bucket[range]))`。

### 标签

`label_replace(v, dst, replacement, src, regex)` · `label_join(v, dst, sep, src...)`

### 排序

`sort` · `sort_desc` · `sort_by_label` · `sort_by_label_desc`

### 时间 / 日期

`time` · `timestamp` · `minute` · `hour` · `day_of_week` · `day_of_month` ·
`day_of_year` · `days_in_month` · `month` · `year`（无参 `datetime()` 等价
`vector(time())`）

### 类型 / 缺失

`vector(s)` · `scalar(v)` · `absent(v)` · `absent_over_time(v[range])`

### 数学

- 基础：`abs` `ceil` `floor` `round(v, [nearest])` `exp` `ln` `log2` `log10` `sqrt` `sgn` `pi`
- 钳制：`clamp(v, min, max)` `clamp_min(v, min)` `clamp_max(v, max)`
- 三角 / 双曲：`sin cos tan asin acos atan sinh cosh tanh asinh acosh atanh`
- 角度：`deg` `rad`

## 运算符

- **算术**：`+ - * / % ^`
- **比较**：`== != > >= < <=`，支持 `bool` modifier
- **集合**：`and` · `or` · `unless`
- **向量匹配**：`on(...)` / `ignoring(...)` 搭配 `group_left(...)` / `group_right(...)`（多对一 / 一对多）
- **一元**：负号 `-v`

## 修饰 / 结构

- 选择器 `@ <timestamp>` 与 `offset <duration>`
- 子查询 `expr[range:step]`（`step` 缺省 60s，分辨率上限 11000 步）

## 标签匹配

支持 `=` · `!=` · `=~` · `!~`，正则走 `regex` crate（RE2 语法），与 Prometheus 的 PCRE
行为在边缘语法上略有差异（本质都是 RE2 子集）。同时支持 or 组 `{job="a" or job="b"}`。

`__name__` 固定为 stream 名（metrics stream），不可重命名。

## 已知差异 / 未实现

| 能力 | 状态 | 说明 |
|---|---|---|
| native-histogram 函数（`histogram_count` / `histogram_sum` / `histogram_avg` / `histogram_stddev` / `histogram_stdvar`） | ❌ | classic bucket 模型下 N/A |
| 向量匹配缺失侧填充（`default`） | ❌ | 未匹配项不填充 |
| recording / alerting rules | 走独立模型 | 用 MoleSignal 自己的告警规则，不读 Prometheus rule 文件（二者格式不互通） |

未支持的函数 / 语法返回 `Error::Invalid("promql function not yet supported: <name>")`；
HTTP `POST /api/v1/query`（`language: "promql"`）映射为 400。

## 内部数据布局

metrics stream 的 parquet schema：

```
_timestamp : TimestampMicros  NOT NULL
value      : Float64          整数 value 自动 cast 为 Float64
<label>    : Utf8             每个 label 一列（非 _timestamp/value 的 Utf8 列即 label）
```

求值路径：按 `parquet_file_meta` 时间窗裁剪候选 parquet → 只解码与窗口相交的 row group →
在内存按 matcher 过滤、按 label 列组合分组成 series。单 selector 一次物化的样本数
有上限，超出即报错提示收窄窗口或追加 label matcher。
