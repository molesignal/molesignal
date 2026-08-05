## 1. 依赖

- [x] 1.1 `Cargo.toml` 工作区 deps 加 `sqlparser = "0.55"`
- [x] 1.2 `crates/infra/Cargo.toml` 加 `sqlparser.workspace = true`

## 2. AST parser

- [x] 2.1 `crates/infra/src/query/parser.rs`：`pub struct TableRef { name, alias, schema }` + `pub fn extract_referenced_tables(stmt: &str) -> Result<Vec<TableRef>>`
- [x] 2.2 算法：`Parser::parse_sql(&GenericDialect, stmt)` → 拿 `Vec<Statement>` → 每个 Statement walk → 收 CTE alias set → walk body 过滤 alias 命中 → dedup by `name`
- [x] 2.3 walk helper：`fn visit_table_factor(factor, ctes, out)` 处理 `TableFactor::{Table, Derived, NestedJoin, …}`
- [x] 2.4 export 到 `crates/infra/src/query/mod.rs::pub mod parser;`

## 3. 12 个 unit test

- [x] 3.1 `simple_select_one_table`
- [x] 3.2 `multiple_join_three_tables`
- [x] 3.3 `cte_inner_extracted_outer_skipped`
- [x] 3.4 `subquery_from_walked`
- [x] 3.5 `lateral_subquery_walked`
- [x] 3.6 `quoted_identifier_unquoted`
- [x] 3.7 `schema_qualified_reduces_to_table_name`
- [x] 3.8 `self_join_deduplicated`
- [x] 3.9 `union_all_collects_both_sides`
- [x] 3.10 `invalid_sql_returns_err`
- [x] 3.11 `case_insensitive_keywords`
- [x] 3.12 `aliased_table_returns_alias`

## 4. Rewrite framework (passthrough)

- [x] 4.1 `crates/infra/src/query/rewrite.rs`：`pub fn enforce_org_isolation(stmt: &str, org_id: &Id) -> Result<String>`
- [x] 4.2 内部直接返 `Ok(stmt.into())`，doc 注释说明未来契约
- [x] 4.3 unit test：passthrough preserves SQL（输入 `"SELECT * FROM logs"` → 输出 `"SELECT * FROM logs"`）

## 5. DataFusionEngine 切换

- [x] 5.1 `crates/infra/src/search/datafusion_engine.rs::execute`：把 `parse_from_tables(...)` 替换为 `parser::extract_referenced_tables(...).map(|r| r.name).collect()`
- [x] 5.2 `planner::parse_from_tables` 标 `#[deprecated]` + 内部转发到新 parser 的简化版本
- [x] 5.3 `crates/api/src/http/routes/query.rs::parse_from_tables`（inspect_query 用）也切到新 parser

## 6. 编译矩阵

- [x] 6.1 `cargo check --workspace` clean
- [x] 6.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 6.3 `cargo test --workspace --lib` 全绿（含 12 个新 test + 现有 4 个 planner test 应继续 pass）
- [x] 6.4 `cargo test -p molesignal-infra --lib query::parser:: query::rewrite::` 全绿
