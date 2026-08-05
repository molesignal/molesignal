// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 基于 `sqlparser` AST 的查询表名提取（change `sqlparser-join-planner`）。
//!
//! 用途：multi-stream JOIN planner 在执行前需要知道 SQL 引用了哪些 stream，
//! 据此决定要 register 到 `SessionContext` 的 Parquet `TableProvider`。这里走真 AST walk，
//! 替代旧的正则 `planner::parse_from_tables`（CTE / 子查询 / quoted identifier 都会
//! 误判）。
//!
//! 解析失败 → `Err(Error::invalid(...))`，由 HTTP 层映射成 400。
//!
//! 设计要点（design.md / D3）：
//! 1. 遍历每个 `Statement`；如果是 `Query`，先收集 `With.cte_tables` 的所有 alias。
//! 2. 用 `BTreeSet<String>` 维持 base table name 集合 + `Vec<TableRef>` 保插入顺序，
//!    `name` 命中 CTE alias 时跳过（CTE 外层引用不算 base table）。
//! 3. 进入 `TableFactor::Derived` 子查询时递归 walk，子查询带自己的 CTE 嵌套作用域。
//! 4. `ObjectName(Vec<Ident>)` 多段标识符 → 取最后一段作为表名（schema-qualified 收敛）。
//! 5. quoted identifier → `Ident.value` 已是去引号后的裸名，直接用。

use std::{collections::BTreeSet, ops::ControlFlow};

use sqlparser::{
    ast::{
        BinaryOperator, Cte, Expr, ObjectName, Query, Select, SetExpr, Statement, TableFactor,
        TableWithJoins, Value, visit_relations_mut,
    },
    dialect::GenericDialect,
    parser::Parser,
};

use crate::{
    domain::stream::StreamType,
    shared::{Error, Result},
};

/// 在 SQL 中被引用的一个 base table。
///
/// 字段保留 `alias` 与 `schema` 为未来 rewrite / diagnostics 留接口；
/// 当前 caller 仅用 `name`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    /// 解析后的裸表名（去 schema、去 quoted）。
    pub name: String,
    /// SQL 中给出的别名（`logs AS l` → `Some("l")`）。
    pub alias: Option<String>,
    /// schema-qualified 时的 schema 部分（`analytics.logs` → `Some("analytics")`）。
    pub schema: Option<String>,
}

/// 采样查询提示解析：识别 SQL 开头的优化器风格注释 `/*+ sample(N) */`（大小写不敏感），
/// 返回 `(样本扫描行数上限, 去掉提示后的 SQL)`。无提示 / 提示非法 → `(None, 原 SQL)`。
///
/// 采样语义：引擎对主表只读到约 N 行即停（parquet_file_meta 粒度的访问计划），SQL 在样本上执行，
/// 用速度换近似——适合大表探索。N 必须为正整数。
pub fn parse_sample_hint(sql: &str) -> (Option<u64>, String) {
    let trimmed = sql.trim_start();
    if !trimmed.starts_with("/*+") {
        return (None, sql.to_string());
    }
    let Some(end) = trimmed.find("*/") else {
        return (None, sql.to_string());
    };
    let hint = &trimmed[3..end];
    let rest = trimmed[end + 2..].trim_start().to_string();
    let lower = hint.to_lowercase();
    // 在 hint 内找 `sample` 后的第一组括号内整数。
    let parsed = lower.find("sample").and_then(|i| {
        let after = &hint[i + "sample".len()..];
        let lp = after.find('(')?;
        let rp = after[lp..].find(')')? + lp;
        after[lp + 1..rp].trim().parse::<u64>().ok()
    });
    match parsed {
        Some(n) if n > 0 => (Some(n), rest),
        // 提示存在但不是合法 sample → 不改写，按普通查询交给 DataFusion（注释本身合法 SQL）。
        _ => (None, sql.to_string()),
    }
}

/// 从 SQL 中抽取所有 base table 引用。
///
/// 行为细节见模块文档；返回结果按首次出现顺序排列，**按 `name` 去重**。
pub fn extract_referenced_tables(stmt: &str) -> Result<Vec<TableRef>> {
    let statements = Parser::parse_sql(&GenericDialect, stmt)
        .map_err(|e| Error::invalid(format!("sqlparser: {e}")))?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<TableRef> = Vec::new();
    for s in &statements {
        // 仅 Query 类才有 FROM；其余 (INSERT/UPDATE 等) 当前不参与 multi-stream planner。
        if let Statement::Query(q) = s {
            visit_query(q, &mut Vec::new(), &mut seen, &mut out);
        }
    }
    Ok(out)
}

/// 从 SQL 提取 `col = 'literal'` 形态的等值谓词，返回 `(列名, 字面量)` 列表。
///
/// 用途：查询侧对 `exact`-indexed 字段（`trace_id` 等）的等值查询走 tantivy 候选裁剪
/// （见 [`crate::infra::query::tantivy_pruner`]）。调用方需自行按 stream schema 过滤出
/// 真正 exact-indexed 的列，本函数只做纯语法提取。
///
/// - 走 AST（非正则）：避免把字符串字面量内部、`LIKE`、`!=` 里的 `=` 误当等值。
/// - 覆盖 `t.col`（取末段列名）与镜像 `'x' = col`。
/// - 仅裸单引号字符串字面量（`Value::SingleQuotedString`）——这正是 `trace_id = '...'`
///   的形态，也是唯一能安全喂给未分词 STRING 索引 `count_term` 的常量。
/// - 解析失败返回空（调用方无谓词即不裁剪，保守且安全）。
pub fn extract_equality_predicates(sql: &str) -> Vec<(String, String)> {
    let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for stmt in &statements {
        // 只看顶层 SELECT 的 WHERE：子查询/CTE 的谓词作用于别的表，不能用来裁主表候选。
        if let Statement::Query(q) = stmt
            && let SetExpr::Select(select) = q.body.as_ref()
            && let Some(selection) = &select.selection
        {
            collect_conjunct_equalities(selection, &mut out);
        }
    }
    out
}

/// 只在**合取**（顶层 `AND` 链）里收集等值谓词。合取项对结果的每一行都必然成立，才可
/// 安全用于**文件级**候选裁剪；一旦下潜到 `OR`（析取）分支就停止——那里的等值对某文件
/// 不成立不代表该文件无匹配行（可经 OR 的另一支满足），拿它裁剪会漏数据。
fn collect_conjunct_equalities(expr: &Expr, out: &mut Vec<(String, String)>) {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            collect_conjunct_equalities(left, out);
            collect_conjunct_equalities(right, out);
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            if let Some(pair) = eq_col_literal(left, right).or_else(|| eq_col_literal(right, left))
            {
                out.push(pair);
            }
        }
        // 括号分组 `(a AND b)` 透传继续拆合取。
        Expr::Nested(inner) => collect_conjunct_equalities(inner, out),
        // OR / NOT / 比较 / LIKE / 函数调用等：不可用于裁剪，整支忽略。
        _ => {}
    }
}

/// 若 `col_expr` 是列引用、`lit_expr` 是单引号字符串字面量，返回 `(列名, 字面量)`。
fn eq_col_literal(col_expr: &Expr, lit_expr: &Expr) -> Option<(String, String)> {
    let col = match col_expr {
        Expr::Identifier(id) => id.value.clone(),
        Expr::CompoundIdentifier(parts) => parts.last()?.value.clone(),
        _ => return None,
    };
    let Expr::Value(vws) = lit_expr else {
        return None;
    };
    match &vws.value {
        Value::SingleQuotedString(s) => Some((col, s.clone())),
        _ => None,
    }
}

/// Flight SQL（spec flight-sql）：经 [`prepare_flight_sql_select`] 校验 + 改写后的语句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightSqlSelect {
    /// 剥掉 `<stream_type>.` 限定符后的 SQL —— 引擎在 `SessionContext` 里按
    /// 裸 stream 名注册 `TableProvider`，限定名直接传会解析失败。
    pub sql: String,
    /// 首个 base table 推导的 stream hint；无表查询（`SELECT 1`）为 `None`。
    pub stream: Option<(String, StreamType)>,
}

/// Flight SQL 入口的语句准备（spec flight-sql）：
///
/// 1. 必须恰好一条语句且为 `Statement::Query`（SELECT / WITH / UNION），
///    DML / DDL / EXPLAIN / 多语句 → `Error::InvalidArgument`。
/// 2. schema 限定符即 stream_type（`logs.nginx` → Logs + `nginx`）；未限定默认
///    Logs（与 HTTP `GET /query/stream` 的 `stream_type` 缺省一致）。出现
///    `logs/metrics/traces/extend` 之外的限定符 → 报错（多半是拼写错误）。
/// 3. 把所有 `<stream_type>.<table>` 改写为裸表名后重新序列化。
pub fn prepare_flight_sql_select(stmt: &str) -> Result<FlightSqlSelect> {
    let mut statements = Parser::parse_sql(&GenericDialect, stmt)
        .map_err(|e| Error::invalid(format!("sqlparser: {e}")))?;
    if statements.len() != 1 {
        return Err(Error::invalid("expected exactly one SQL statement"));
    }
    let mut statement = statements.pop().expect("len checked above");
    if !matches!(statement, Statement::Query(_)) {
        return Err(Error::invalid(
            "only SELECT statements are supported over Flight SQL",
        ));
    }

    // CTE-aware 抽 base table；所有限定符必须是合法 stream_type（防 typo 落到
    // 引擎层变成难懂的 table-not-found）。
    let refs = extract_referenced_tables(stmt)?;
    for r in &refs {
        if let Some(q) = r.schema.as_deref()
            && stream_type_from_qualifier(q).is_none()
        {
            return Err(Error::invalid(format!(
                "unknown stream type qualifier '{q}' (expected logs/metrics/traces/extend)"
            )));
        }
    }
    let stream = refs.first().map(|r| {
        let st = r
            .schema
            .as_deref()
            .and_then(stream_type_from_qualifier)
            .unwrap_or(StreamType::Logs);
        (r.name.clone(), st)
    });

    // 限定名 → 裸表名（CTE 别名是单段不受影响）：
    // - `<stream_type>.<table>`（文档推荐写法）
    // - `<catalog>.<stream_type>.<table>`（DBeaver 等客户端浏览表数据时按
    //   get_tables 返回的 catalog/schema 生成全限定名）
    let _: ControlFlow<()> = visit_relations_mut(&mut statement, |name: &mut ObjectName| {
        let is_stream_qualifier = |part: &sqlparser::ast::ObjectNamePart| {
            part.as_ident()
                .is_some_and(|i| stream_type_from_qualifier(&i.value).is_some())
        };
        match name.0.len() {
            2 if is_stream_qualifier(&name.0[0]) => {
                name.0.remove(0);
            }
            3 if is_stream_qualifier(&name.0[1]) => {
                name.0.drain(0..2);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });

    Ok(FlightSqlSelect {
        sql: statement.to_string(),
        stream,
    })
}

/// stream_type schema 限定符 → [`StreamType`]；大小写不敏感，未知 → `None`。
fn stream_type_from_qualifier(q: &str) -> Option<StreamType> {
    match q.to_ascii_lowercase().as_str() {
        "logs" => Some(StreamType::Logs),
        "metrics" => Some(StreamType::Metrics),
        "traces" => Some(StreamType::Traces),
        "profiles" => Some(StreamType::Profiles),
        "extend" => Some(StreamType::Extend),
        _ => None,
    }
}

fn visit_query(
    q: &Query,
    cte_scopes: &mut Vec<BTreeSet<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TableRef>,
) {
    // 进入当前 Query 作用域：把它的 CTE alias 推到 stack；body 走完再 pop。
    let mut local: BTreeSet<String> = BTreeSet::new();
    if let Some(with) = &q.with {
        for cte in &with.cte_tables {
            local.insert(cte_alias(cte));
        }
        // CTE 内部仍要 walk（找内层 base table），但 CTE 名本身不算引用。
        // 注意：CTE body 应该看得到「同级 CTE 的别名」吗？标准 SQL 中前置 CTE 可以被
        // 同 WITH 里后面的 CTE 引用。这里把同级 alias 也合到当前作用域里再 walk body。
        cte_scopes.push(local.clone());
        for cte in &with.cte_tables {
            visit_query(&cte.query, cte_scopes, seen, out);
        }
        cte_scopes.pop();
    }
    cte_scopes.push(local);
    visit_set_expr(&q.body, cte_scopes, seen, out);
    cte_scopes.pop();
}

fn visit_set_expr(
    body: &SetExpr,
    cte_scopes: &mut Vec<BTreeSet<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TableRef>,
) {
    match body {
        SetExpr::Select(select) => visit_select(select, cte_scopes, seen, out),
        SetExpr::Query(q) => visit_query(q, cte_scopes, seen, out),
        SetExpr::SetOperation { left, right, .. } => {
            visit_set_expr(left, cte_scopes, seen, out);
            visit_set_expr(right, cte_scopes, seen, out);
        }
        // VALUES / INSERT / UPDATE / TABLE / 其它非典型分支：不引入 base table 信息。
        _ => {}
    }
}

fn visit_select(
    select: &Select,
    cte_scopes: &mut Vec<BTreeSet<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TableRef>,
) {
    for twj in &select.from {
        visit_table_with_joins(twj, cte_scopes, seen, out);
    }
}

fn visit_table_with_joins(
    twj: &TableWithJoins,
    cte_scopes: &mut Vec<BTreeSet<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TableRef>,
) {
    visit_table_factor(&twj.relation, cte_scopes, seen, out);
    for join in &twj.joins {
        visit_table_factor(&join.relation, cte_scopes, seen, out);
    }
}

fn visit_table_factor(
    factor: &TableFactor,
    cte_scopes: &mut Vec<BTreeSet<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TableRef>,
) {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let (schema, bare) = split_object_name(name);
            // CTE alias 命中 → 跳过（仅在 schema 为空时；`schema.cte_name` 几乎不可能合法）。
            if schema.is_none() && in_any_scope(cte_scopes, &bare) {
                return;
            }
            if seen.insert(bare.clone()) {
                out.push(TableRef {
                    name: bare,
                    alias: alias.as_ref().map(|a| a.name.value.clone()),
                    schema,
                });
            }
        }
        TableFactor::Derived { subquery, .. } => {
            visit_query(subquery, cte_scopes, seen, out);
        }
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => {
            visit_table_with_joins(table_with_joins, cte_scopes, seen, out);
        }
        // TableFunction / Pivot / Unnest / JsonTable / OpenJsonTable / 等：不引入 base table。
        // 未来扩展可在此分支补；当前 stream planner 用不到。
        _ => {}
    }
}

fn cte_alias(cte: &Cte) -> String {
    cte.alias.name.value.clone()
}

fn split_object_name(name: &ObjectName) -> (Option<String>, String) {
    let parts: Vec<String> = name.0.iter().map(object_name_part_value).collect();
    match parts.len() {
        0 => (None, String::new()),
        1 => (None, parts.into_iter().next().unwrap()),
        _ => {
            let last = parts.last().cloned().unwrap_or_default();
            // 倒数第二段作为 schema；3 段以上的 db.schema.table 也收敛到 schema=middle、name=last。
            let mid = parts.iter().rev().nth(1).cloned();
            (mid, last)
        }
    }
}

/// sqlparser 0.61 把 `ObjectName.0` 改成 `Vec<ObjectNamePart>` 枚举，
/// `Ident` 是其中一个变体；这里只关心 ident 段，函数式调用段跳过。
fn object_name_part_value(part: &sqlparser::ast::ObjectNamePart) -> String {
    use sqlparser::ast::ObjectNamePart;
    match part {
        ObjectNamePart::Identifier(id) => id.value.clone(),
        // Function-call style (e.g. `unnest(...)` in some dialects) — 取 debug 名兜底。
        other => format!("{other}"),
    }
}

fn in_any_scope(scopes: &[BTreeSet<String>], name: &str) -> bool {
    scopes.iter().any(|s| s.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(refs: &[TableRef]) -> Vec<&str> {
        refs.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn equality_predicates_basic_and_mirror_and_compound() {
        // 基本形态（trace 端点的实际 SQL）。
        let p = extract_equality_predicates(
            "SELECT * FROM traces WHERE trace_id = '0af7651916cd43dd' ORDER BY _timestamp",
        );
        assert_eq!(
            p,
            vec![("trace_id".to_string(), "0af7651916cd43dd".to_string())]
        );

        // 镜像：字面量在左。
        let p = extract_equality_predicates("SELECT * FROM t WHERE 'x' = span_id");
        assert_eq!(p, vec![("span_id".to_string(), "x".to_string())]);

        // 限定列 t.col → 取末段列名。
        let p = extract_equality_predicates("SELECT * FROM traces t WHERE t.service_name = 'api'");
        assert_eq!(p, vec![("service_name".to_string(), "api".to_string())]);

        // 多个 AND 谓词都提取。
        let p =
            extract_equality_predicates("SELECT * FROM t WHERE trace_id = 'a' AND span_id = 'b'");
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn equality_predicates_ignores_non_string_and_non_eq() {
        // 数值字面量不提取（STRING 索引只接字符串等值）。
        assert!(extract_equality_predicates("SELECT * FROM t WHERE code = 500").is_empty());
        // 非等值算子不提取。
        assert!(extract_equality_predicates("SELECT * FROM t WHERE n > 3").is_empty());
        assert!(extract_equality_predicates("SELECT * FROM t WHERE msg LIKE '%x%'").is_empty());
        // 列 = 列 不提取（右侧非字面量）。
        assert!(extract_equality_predicates("SELECT * FROM t WHERE a = b").is_empty());
        // 解析失败 → 空。
        assert!(extract_equality_predicates("NOT SQL @@@").is_empty());
    }

    /// OR 分支里的等值**不得**用于裁剪：文件可经 OR 另一支满足，裁了会漏数据。
    #[test]
    fn equality_predicates_skips_disjunction() {
        // 顶层 OR → 一个都不提取（保守，不裁）。
        assert!(
            extract_equality_predicates("SELECT * FROM t WHERE trace_id = 'a' OR span_id = 'b'")
                .is_empty()
        );
        // AND 顶层但一支是 OR：只提取纯合取那一支，OR 支忽略。
        let p = extract_equality_predicates(
            "SELECT * FROM t WHERE trace_id = 'a' AND (span_id = 'b' OR span_id = 'c')",
        );
        assert_eq!(p, vec![("trace_id".to_string(), "a".to_string())]);
        // 括号包裹的合取仍拆开。
        let p =
            extract_equality_predicates("SELECT * FROM t WHERE (trace_id = 'a' AND span_id = 'b')");
        assert_eq!(p.len(), 2);
        // 子查询里的等值不提取（作用于别的表）。
        let p = extract_equality_predicates(
            "SELECT * FROM t WHERE id IN (SELECT id FROM u WHERE k = 'v')",
        );
        assert!(p.is_empty());
    }

    #[test]
    fn sample_hint_parsed_and_stripped() {
        let (n, sql) = parse_sample_hint("/*+ sample(10000) */ SELECT * FROM logs");
        assert_eq!(n, Some(10_000));
        assert_eq!(sql, "SELECT * FROM logs");
        // 大小写不敏感 + 多余空白。
        let (n2, _) = parse_sample_hint("  /*+  SAMPLE( 50 ) */SELECT 1");
        assert_eq!(n2, Some(50));
    }

    #[test]
    fn sample_hint_absent_or_invalid_is_passthrough() {
        assert_eq!(
            parse_sample_hint("SELECT * FROM logs"),
            (None, "SELECT * FROM logs".to_string())
        );
        // 非 sample 的合法注释 → 不改写。
        let (n, sql) = parse_sample_hint("/*+ other_hint */ SELECT 1");
        assert_eq!(n, None);
        assert_eq!(sql, "/*+ other_hint */ SELECT 1");
        // sample(0) 视为无效。
        assert_eq!(parse_sample_hint("/*+ sample(0) */ SELECT 1").0, None);
    }

    #[test]
    fn simple_select_one_table() {
        let r = extract_referenced_tables("SELECT * FROM logs WHERE x = 1").unwrap();
        assert_eq!(names(&r), vec!["logs"]);
    }

    #[test]
    fn multiple_join_three_tables() {
        let r = extract_referenced_tables(
            "SELECT * FROM logs l JOIN metrics m ON l.ts = m.ts \
             JOIN traces t ON l.trace_id = t.trace_id",
        )
        .unwrap();
        let mut got: Vec<&str> = names(&r);
        got.sort();
        assert_eq!(got, vec!["logs", "metrics", "traces"]);
    }

    #[test]
    fn cte_inner_extracted_outer_skipped() {
        let r = extract_referenced_tables(
            "WITH high_lat AS (SELECT * FROM traces WHERE duration_us > 1000) \
             SELECT * FROM high_lat JOIN services s ON high_lat.svc = s.name",
        )
        .unwrap();
        let mut got: Vec<&str> = names(&r);
        got.sort();
        assert_eq!(got, vec!["services", "traces"]);
        assert!(!got.contains(&"high_lat"));
    }

    #[test]
    fn subquery_from_walked() {
        let r = extract_referenced_tables(
            "SELECT * FROM (SELECT id FROM logs) sub JOIN traces ON sub.id = traces.id",
        )
        .unwrap();
        let mut got: Vec<&str> = names(&r);
        got.sort();
        assert_eq!(got, vec!["logs", "traces"]);
    }

    #[test]
    fn lateral_subquery_walked() {
        // `LATERAL (SELECT ...)` 也走 Derived 分支；这里同时验证 INNER JOIN ... ON true。
        let r = extract_referenced_tables(
            "SELECT * FROM logs l JOIN LATERAL \
             (SELECT * FROM metrics WHERE ts = l.ts) sub ON true",
        )
        .unwrap();
        let mut got: Vec<&str> = names(&r);
        got.sort();
        assert_eq!(got, vec!["logs", "metrics"]);
    }

    #[test]
    fn quoted_identifier_unquoted() {
        let r = extract_referenced_tables(r#"SELECT * FROM "weird-stream""#).unwrap();
        assert_eq!(names(&r), vec!["weird-stream"]);
    }

    #[test]
    fn schema_qualified_reduces_to_table_name() {
        let r = extract_referenced_tables("SELECT * FROM analytics.logs").unwrap();
        assert_eq!(names(&r), vec!["logs"]);
        assert_eq!(r[0].schema.as_deref(), Some("analytics"));
    }

    #[test]
    fn self_join_deduplicated() {
        let r = extract_referenced_tables("SELECT * FROM logs a JOIN logs b ON a.id = b.parent_id")
            .unwrap();
        assert_eq!(names(&r), vec!["logs"]);
    }

    #[test]
    fn union_all_collects_both_sides() {
        let r = extract_referenced_tables("SELECT id FROM logs UNION ALL SELECT id FROM traces")
            .unwrap();
        let mut got: Vec<&str> = names(&r);
        got.sort();
        assert_eq!(got, vec!["logs", "traces"]);
    }

    #[test]
    fn invalid_sql_returns_err() {
        let err = extract_referenced_tables("SELEC * FROM").unwrap_err();
        assert!(err.to_string().contains("sqlparser"), "{err}");
    }

    #[test]
    fn case_insensitive_keywords() {
        let r = extract_referenced_tables("select * from LOGS l").unwrap();
        // 表名标识符按 sqlparser 默认（GenericDialect）保留原大小写。
        assert_eq!(names(&r), vec!["LOGS"]);
    }

    #[test]
    fn aliased_table_returns_alias() {
        let r = extract_referenced_tables("SELECT * FROM logs AS l").unwrap();
        assert_eq!(names(&r), vec!["logs"]);
        assert_eq!(r[0].alias.as_deref(), Some("l"));
    }

    // === prepare_flight_sql_select（spec flight-sql） ===

    #[test]
    fn flight_sql_strips_all_four_stream_type_qualifiers() {
        for (qualifier, expected) in [
            ("logs", StreamType::Logs),
            ("metrics", StreamType::Metrics),
            ("traces", StreamType::Traces),
            ("extend", StreamType::Extend),
        ] {
            let out =
                prepare_flight_sql_select(&format!("SELECT * FROM {qualifier}.nginx LIMIT 5"))
                    .unwrap();
            assert_eq!(out.sql, "SELECT * FROM nginx LIMIT 5", "{qualifier}");
            let (name, st) = out.stream.expect("stream hint");
            assert_eq!(name, "nginx");
            assert_eq!(st, expected);
        }
    }

    #[test]
    fn flight_sql_unqualified_defaults_to_logs() {
        let out = prepare_flight_sql_select("SELECT count(*) FROM nginx").unwrap();
        assert_eq!(out.sql, "SELECT count(*) FROM nginx");
        assert_eq!(out.stream, Some(("nginx".to_string(), StreamType::Logs)));
    }

    #[test]
    fn flight_sql_no_table_yields_no_stream_hint() {
        let out = prepare_flight_sql_select("SELECT 1").unwrap();
        assert_eq!(out.stream, None);
    }

    #[test]
    fn flight_sql_unknown_qualifier_rejected() {
        let err = prepare_flight_sql_select("SELECT * FROM analytics.nginx").unwrap_err();
        assert!(
            err.to_string().contains("unknown stream type qualifier"),
            "{err}"
        );
    }

    #[test]
    fn flight_sql_quoted_qualifier_stripped() {
        let out = prepare_flight_sql_select(r#"SELECT * FROM "metrics"."cpu-usage""#).unwrap();
        assert_eq!(out.sql, r#"SELECT * FROM "cpu-usage""#);
        assert_eq!(
            out.stream,
            Some(("cpu-usage".to_string(), StreamType::Metrics))
        );
    }

    #[test]
    fn flight_sql_catalog_qualified_stripped() {
        // DBeaver 浏览表数据按 catalog.schema.table 生成全限定名
        let out = prepare_flight_sql_select("SELECT * FROM molesignal.logs.app LIMIT 3").unwrap();
        assert_eq!(out.sql, "SELECT * FROM app LIMIT 3");
        assert_eq!(out.stream, Some(("app".to_string(), StreamType::Logs)));

        // 带引号变体（引号在重序列化后保留，仍是合法 SQL）
        let out = prepare_flight_sql_select(r#"SELECT * FROM "molesignal"."metrics"."cpu_usage""#)
            .unwrap();
        assert_eq!(out.sql, r#"SELECT * FROM "cpu_usage""#);
        assert_eq!(
            out.stream,
            Some(("cpu_usage".to_string(), StreamType::Metrics))
        );
    }

    #[test]
    fn flight_sql_cte_with_qualified_inner_table() {
        let out = prepare_flight_sql_select(
            "WITH errs AS (SELECT * FROM logs.nginx WHERE level = 'error') \
             SELECT count(*) FROM errs",
        )
        .unwrap();
        assert!(out.sql.contains("FROM nginx"), "{}", out.sql);
        assert_eq!(out.stream, Some(("nginx".to_string(), StreamType::Logs)));
    }

    #[test]
    fn flight_sql_rejects_dml_ddl_and_multi_statement() {
        for sql in [
            "INSERT INTO nginx VALUES (1)",
            "DROP TABLE nginx",
            "UPDATE nginx SET x = 1",
            "DELETE FROM nginx",
            "CREATE TABLE t (x INT)",
            "SELECT 1; SELECT 2",
        ] {
            let err = prepare_flight_sql_select(sql).unwrap_err();
            assert!(
                matches!(err, Error::InvalidArgument(_)),
                "{sql} should be rejected, got: {err}"
            );
        }
    }
}
