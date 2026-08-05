## Why

当前 multi-stream JOIN planner（M1）用正则抓 SQL 中的 `FROM` / `JOIN <name>` 标识符。能跑通绝大多数日常查询，但是：
- 子查询中的 `FROM (SELECT ...)` 会被错误地把内层别名抓出来
- `CTE WITH x AS (SELECT ... FROM logs)` 完全识别不到（regex 不能跨 token 上下文）
- 表别名 / quoted identifier / schema-qualified（`schema.table`）边界处理简陋
- 没法做 LogicalPlan 级别的 multi-tenancy guard（当前只能在 stream level 校验，跨 stream JOIN 后 SELECT * 仍可能泄漏）

用真正的 SQL AST parser（`sqlparser` crate）做一遍 walk，可以彻底解决，也为后续 query rewrite（org_id filter 注入、column projection pruning）打基础。

## What Changes

- 引入 `sqlparser = "0.55"` workspace dep（DataFusion 已经传递依赖，加 direct dep 无额外二进制开销）。
- 新增 `crates/infra/src/query/parser.rs`：`extract_referenced_tables(stmt: &str) -> Vec<TableRef>`（walk `sqlparser::ast`，递归收集 `TableFactor::Table`，跳过 CTE 内别名引用），替代现有的 `planner::parse_from_tables`。
- 新增 `extract_org_filter_targets(stmt) -> Vec<TableRef>`：返每个 base table 引用的位置（line, col）+ 是否已有 WHERE clause，便于后续 rewrite pass。
- `DataFusionEngine::execute` 切到新解析器；`planner::parse_from_tables` 标 deprecated 转发。
- 新增 `crates/infra/src/query/rewrite.rs`：`enforce_org_isolation(stmt, org_id)` 把 SELECT 改写为 `SELECT ... FROM (SELECT * FROM <table> WHERE _org_id = $org) AS <table>`（如果该 stream 含 `_org_id` 列；当前 schema 全租户每行没有 org_id 字段，本 change 默认跳过该改写，留 ingester 写 `_org_id` 字段的 follow-up；rewrite 函数本身就位即可）。
- 新增 unit tests 12 个：CTE / 子查询 / 多 JOIN / 别名 / schema-qualified / quoted identifier / 大小写敏感等。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `query`: planner 解析器从 regex 升级到 sqlparser-based AST walk；rewrite 框架就位。

## Impact

- **依赖**：`sqlparser = "0.55"` direct dep（与 DataFusion 53 用同 minor）；编译时间增加 < 5s。
- **替换点**：`crates/infra/src/query/planner.rs::parse_from_tables` deprecated 转发到新 parser。
- **不破坏现有调用方**：API 签名不变（输入 SQL → 输出表名列表），仅算法升级。
- **`api/.../routes/query.rs::parse_from_tables`**：是 inspect_query 用的轻量解析，也切到新 parser，保持一致行为。
- **测试**：12 个新 unit test；现有 4 个旧 test 全数通过（algorithm 等价 +更精确）。
- **风险**：rewrite 不引入，仅 framework；现有 multi-tenancy 保护机制（`ensure_stream_in_org` + per-org MemTable scope）保持不变。
