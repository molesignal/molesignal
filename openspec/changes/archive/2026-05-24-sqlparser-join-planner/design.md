## Context

Multi-stream JOIN planner（spec M1）已经用正则解析 `FROM` / `JOIN` 来决定要注册哪些 MemTable。够用但脆弱：CTE / 子查询 / quoted identifier 都会误抓或漏抓。DataFusion 53 已经传递依赖 `sqlparser`，可以零成本切到真 AST walk。

## Goals / Non-Goals

**Goals:**
- 正确解析 CTE / 子查询 / quoted identifier / schema-qualified table
- DataFusionEngine 切到新解析器无 API 变化
- 给未来 query rewrite（org_id WHERE 注入）留下函数入口

**Non-Goals:**
- 不实装真正的 LogicalPlan rewriter（DataFusion API 不稳，深度耦合 → 留独立 PR）
- 不实装 `_org_id` 列添加到所有 stream 的 schema 演化
- 不动 multi-tenancy 保护（现状 `ensure_stream_in_org` + per-org MemTable scope 足够）
- 不引 sqlx-style 查询编译时校验（用户 SQL 是 runtime 的）

## Decisions

### D1：用 `sqlparser` 而不是 DataFusion 自身的 logical_plan

DataFusion 的 logical_plan 已经把"哪些表被引用"信息埋在 plan tree 里，但要拿到必须先成功 `SessionContext::sql(...)`，而 sql 调用要求所有表都已 register —— 鸡生蛋问题。`sqlparser` 是 syntactic walk，不需要 table 元信息。

### D2：返回 `Vec<TableRef { name, alias?, schema? }>`

虽然现阶段只用 name，但 alias / schema 信息对未来 rewrite + diagnostics 有用，保留无成本。

### D3：CTE 跳过逻辑

`sqlparser` 把 CTE 表示为 `With { cte_tables: [Cte { alias, query }] } + body: Query`。算法：
1. 收集所有 CTE alias 到 set
2. Walk 整个 AST，所有 `TableFactor::Table { name }` 若 name 在 set 内 → skip
3. CTE body 自身仍要 walk（拿里面的 base tables）

### D4：rewrite 函数现在 passthrough

放进 spec 是为了 lock 公共 API surface；现在不实装避免 `_org_id` schema 大改触发的二级变更。当 stream schema 加 `_org_id` 列时，单独 PR 把 passthrough 换成实装。

### D5：error 不 panic

`sqlparser::Parser::parse_sql` 在非法 SQL 上返 `Err`；当前包装成 `Error::invalid` 让 DataFusionEngine 在 execute 路径上报 400 instead of 5xx。

## Risks / Trade-offs

**[R1] sqlparser 与 DataFusion 用版本不一致**：DataFusion 53 内部用 sqlparser 0.51；我们 direct dep 用 0.55。
→ Mitigation：实际只是 syntactic walk，不传 AST 给 DataFusion；两份独立编译；测试覆盖 dialect 差异。

**[R2] 用户 SQL 跟 `sqlparser` 不支持的 dialect 写法**：sqlparser 默认 generic dialect 兼容性最好，但 `PostgresDialect` 解析更精确。
→ Mitigation：默认用 `GenericDialect`；用户写到 `RETURNING` / `ILIKE` 等 PG 专属语法时，解析报错 → 已经走到 400 路径，行为合理。

**[R3] 多 dialect 失败 fallback**：失败时 fallback regex 解析？
→ 拒：错误 silent 更危险（如 CTE 又被误抓）。直接报错让用户知道 SQL 不合法或不支持。

**[R4] sqlparser AST 结构 minor 升级 breaking**：未来 0.55 → 0.56 可能调字段。
→ Mitigation：pin minor 版本到 workspace；upgrade 单独 PR 验证。
