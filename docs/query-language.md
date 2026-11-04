# 文本检索函数：MATCH 与 MATCH_TEXT

日志 / 文本字段的全文检索能力由两个 SQL 函数提供：`MATCH`（通用子串）与
`MATCH_TEXT`（索引绑定的全文检索）。本文定义二者的语义契约、查询语法与索引前提，
作为实现（`src/infra/query/tantivy_pruner/`、`src/infra/search/datafusion_engine/`）的
对照基准。

> 函数规范形式为**全大写** `MATCH` / `MATCH_TEXT`（文档、示例、错误信息、前端补全均大写）；
> SQL 解析与执行大小写不敏感，小写调用（`match_text(...)`）同样生效。

## 字段索引类型

数据流字段的 `index_type` 直接决定写入与查询路径，不再由 `indexed/exact` 两个兼容字段
模糊推断：

| 类型 | 写入结构 | 查询用途 |
| --- | --- | --- |
| `none` | 不创建字段索引 | 完整交给 Parquet/DataFusion 执行 |
| `exact` | 文本字段使用未分词 Tantivy `STRING`；Parquet Bloom；Catalog 里的 min/max；封存分区的整值过滤器 | `=` / `IN` 先用分区过滤器、再用 Tantivy 计数裁剪文件，命中少时只读命中行所在的页 |
| `full_text` | 分词 Tantivy（`molesignal_text` 分词器：字母数字连续段 + 中日韩二元组，只记文档） | `MATCH` / `MATCH_TEXT` / `LIKE` / `ILIKE` 字面量的安全文件级候选裁剪 |
| `bloom` | Parquet Bloom；文本字段另有段级整值过滤器 Artifact（封存时并进分区过滤器）；min/max | `=` / `IN` 在打开文件前跳过一定不含这些值的文件 |
| `skip` | Catalog 里的 min/max；段内不同值不超过 32 个时记下值集合 | 顶层合取里的比较、`IN` / `NOT IN`、`BETWEEN`、前缀 `LIKE` 的文件级裁剪 |
| `ngram` | 段级三元组过滤器 Artifact（字节三元组，逐字符小写化） | `LIKE` / `ILIKE` / `MATCH` 字面量有三元组不在文件里时跳过 |
| `ip` | Catalog 里段内地址的范围（IPv4 映射进 IPv6） | `CIDR_MATCH(field, 'net')` 与对地址的 `=` / `IN` 的文件级裁剪 |

文件统计（min/max、值集合、地址范围）的裁剪见 `src/infra/query/skip_pruner/`：只用顶层合取
链上的条件，同一列上用 `OR` 连起来的条件整体算一个；跨列的 `OR`、`NOT`、复杂表达式、类型不
匹配、缺失边界与加密字段一律保守保留文件。全文字段不记 min/max（两条整行日志只会压在
Catalog 上），过长的字符串边界截成 64 字节以内仍保持是上下界。

数据流设置里的 `partitions`（按字段分片，见 `domain::stream::FieldPartitions`）把日志与链路
的新数据按一两个文本字段的值分进分片：每个字段按哈希（2–64 片）或首字符（37 片）分，两个字段
的分片号按各自的分片数进位拼成一个，乘积不超过 256。段在列统计里记下写入时的分片字段；查询对
分片字段的 `=` / `IN`，以及首字符分片字段上以文字开头的 `LIKE` / `ILIKE`，只读可能命中的分片
的文件，合并只合分片字段相同的段。

索引只影响候选裁剪，最终结果仍由 DataFusion 的行级表达式决定。修改字段配置只影响随后
写出的 Parquet 文件；Tantivy 归档的格式变了（当前格式 4）时由后台按新格式重建。

### 默认索引

字段第一次写入数据流时按下表取得索引类型，之后可在数据流的字段配置里修改。已有数据流里
已经登记的字段保持原有配置，不随默认值变化。

| 数据流类型 | 默认 `exact` | 默认 `full_text` |
| --- | --- | --- |
| 日志 | `_event_id`、`session_id`、`fingerprint`、`country`、`browser`、`application`、`environment` | `message` |
| Trace | `trace_id`、`span_id`、`parent_span_id`、`service.name` | — |
| 指标 | 指标名字段 | — |

`_timestamp` 不是数据流 schema 里的字段，没有 `index_type`：每个数据流都按它分区并按查询的
时间范围裁剪文件。日志与 Trace 的字段列表因此把它和上表的默认字段一起列在「索引字段」下。

## MATCH(field, term)：通用子串匹配

`MATCH` 是**无门槛**的通用子串函数：字段值包含 `term` 作为连续子串即命中，
大小写不敏感，任何字段可用（有 TEXT 索引时用于文件裁剪加速，无索引时纯 ILIKE 执行）。

| 语义 | 说明 |
| --- | --- |
| 匹配规则 | 连续子串包含（`ILIKE '%term%'`） |
| 大小写 | 不敏感（ILIKE） |
| `%` / `_` | 一律按**字面量**处理（不解释为 LIKE 通配符） |
| `*` | 普通字符（不是通配符） |
| 空 term | 恒不匹配任何行 |
| 索引前提 | 无（任何字段可用） |

示例：

```sql
SELECT * FROM app_logs WHERE MATCH(message, 'panic');   -- 子串命中，大小写不敏感
SELECT * FROM app_logs WHERE MATCH(message, '100%');    -- 字面 `100%`，不会命中 `1000`
```

## MATCH_TEXT(field, query)：全文检索

`MATCH_TEXT` 对单个字段执行全文检索，前提是字段已配置全文索引
（`index_type == full_text`）。未配置索引的字段调用会报错，
错误信息指明该字段未配置全文索引。单 token 查询的匹配语义与 `MATCH` 一致。

### 查询语法

| 语法 | 语义 |
| --- | --- |
| `a b` | 空格分隔的多个词 = **token 级 AND**：每个词都必须以子串形式出现，位置无关 |
| `"a b"` | 双引号 = **短语**：引号内文本作为连续子串 |
| `error*` / `*error` / `*error*` | `*` = **通配符**：前缀 / 后缀 / 包含；短语内同样生效（`"api v*"`） |
| `a OR b` | 或 |
| `-a` | 排除（NOT） |
| `\*` / `\"` / `\\` | 转义：表示字面量 `*` / `"` / `\`（`\%`、`\_` 同理） |
| `''` / `'   '` | 空 query 或全部为空 token：恒不匹配任何行 |
| 大小写 | 不敏感（ILIKE） |
| `%` / `_` | 按**字面量**处理（不是 LIKE 通配符） |

示例：

```sql
-- token 级 AND：两个词可分散出现
SELECT * FROM app_logs WHERE MATCH_TEXT(message, 'panic disk');

-- 短语：连续子串
SELECT * FROM app_logs WHERE MATCH_TEXT(message, '"disk full"');

-- 混合 token 与短语
SELECT * FROM app_logs WHERE MATCH_TEXT(message, 'panic "disk full"');

-- 前缀通配符：以 error 开头
SELECT * FROM app_logs WHERE MATCH_TEXT(message, 'error*');

-- OR / NOT
SELECT * FROM app_logs WHERE MATCH_TEXT(message, 'panic OR timeout');
SELECT * FROM app_logs WHERE MATCH_TEXT(message, 'panic -debug');
```

### 索引前提与错误行为

- 字段必须已配置 `full_text` 索引；未配置时查询失败，错误信息
  指向配置全文索引。
- full_text 索引类型仅限 **string（utf8）** 字段：创建数据流与更新设置时对
  `index_type == full_text` 且字段非 utf8 的请求返回 400；前端 schema 编辑界面按字段
  类型过滤索引选项（非 utf8 不提供 full_text 选项）。
- 存量已配置 full_text 的 json 字段**不产生行为回退**：写侧索引构建仍支持 string 与
  json 字段，索引前提校验仅约束新建与更新的配置提交。

### 执行与裁剪模型

`MATCH` 与 `MATCH_TEXT` 先改写成 **ILIKE 表达式树**，由 DataFusion 执行（结果正确性的
来源）。裁剪从改写后的语句里读：顶层合取链上的 `LIKE` / `ILIKE`（改写出的，与语句本来就有
的，如日志筛选器的「包含」）每段字面量是一个子串谓词，同一列上 `OR` 连起来的几个模式合成一个
谓词的几个候选；`NOT` 分支只执行不裁剪。

Tantivy 判断子串时把字面量拆成索引答得了的段：ASCII 字母数字段必须落在某个词里（词典扫描与
长词三元组），中日韩文字段的二元组必须都在词典里；标点与其它文字不查。有一段一定没有，文件
就一定没有这个子串。格式 4 之前的归档按默认分词建，中日韩段不查。任何裁剪不确定的情形都
退化为全量执行，**不会丢数据**。
