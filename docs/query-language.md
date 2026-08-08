# 文本检索函数：MATCH 与 MATCH_TEXT

日志 / 文本字段的全文检索能力由两个 SQL 函数提供：`MATCH`（通用子串）与
`MATCH_TEXT`（索引绑定的全文检索）。本文定义二者的语义契约、查询语法与索引前提，
作为实现（`src/infra/query/tantivy_pruner/`、`src/infra/search/datafusion_engine/`）的
对照基准。

> 函数规范形式为**全大写** `MATCH` / `MATCH_TEXT`（文档、示例、错误信息、前端补全均大写）；
> SQL 解析与执行大小写不敏感，小写调用（`match_text(...)`）同样生效。

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
（`indexed && !exact`，即 full_text 索引类型）。未配置索引的字段调用会报错，
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

- 字段必须已配置 full_text 索引（`indexed && !exact`）；未配置时查询失败，错误信息
  指向配置全文索引。
- full_text 索引类型仅限 **string（utf8）** 字段：创建数据流与更新设置时对
  `index_type == full_text` 且字段非 utf8 的请求返回 400；前端 schema 编辑界面按字段
  类型过滤索引选项（非 utf8 不提供 full_text 选项）。
- 存量已配置 full_text 的 json 字段**不产生行为回退**：写侧索引构建仍支持 string 与
  json 字段，索引前提校验仅约束新建与更新的配置提交。

### 执行与裁剪模型

`MATCH_TEXT` 由 DataFusion 以 **ILIKE 表达式树**执行（结果正确性的来源）。tantivy
仅做文件级裁剪，且仅限「顶层合取项中、纯 AND 树、无通配符单 token」的叶子——这类
term 小写化后与 TEXT 索引的 token 形态一致，`count_term` 可证明其不命中。OR / NOT /
短语 / 通配符分支一律只执行不裁剪；任何裁剪不确定的情形都退化为全量执行，**不会
丢数据**。
