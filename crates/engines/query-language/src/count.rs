// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 识别只数行数的语句：`SELECT count(*) FROM <table> [WHERE col = 'v' AND …]`、explorer
//! 直方图按时间分柱数行数的语句（见 [`BarCounts`]），以及字段分析的最常见值与汇总。
//!
//! 这些语句的结果只取决于满足条件的行数，直方图还取决于每行的时间：引擎可以用 Catalog 记录的
//! 行数或索引回答，不必读数据。条件是等值（`col = 'v'`，由整值索引回答）与子串
//! （`col ILIKE '%词%'`，由分词索引回答）的合取。这里只做语法判断，字段有没有这样的索引由
//! 调用方按 stream schema 决定。列名按 DataFusion 的规则解析：未加引号的标识符按 ASCII
//! 转小写。

use sqlparser::{
    ast::{
        BinaryOperator, CaseWhen, CastKind, DataType, Expr, FunctionArg, FunctionArgExpr,
        FunctionArguments, GroupByExpr, ObjectNamePart, Query, Select, SelectItem, SetExpr,
        Statement, TableFactor, TableWithJoins, UnaryOperator, Value,
    },
    dialect::GenericDialect,
    parser::Parser,
};

mod conditions;
mod summary;
mod values;

use conditions::{Conditions, TimeWindow, collect_conditions};
pub use summary::{FieldSummary, field_summary};
pub use values::{TopValues, top_values};

/// 行时间列。
const TIMESTAMP_COLUMN: &str = "_timestamp";

/// explorer 直方图语句里决定结果的全部内容。语句的写法是：
///
/// ```sql
/// SELECT (CAST(_timestamp AS BIGINT) - <origin>) / <width> AS …, COUNT(*) AS …
///        [, SUM(CASE WHEN UPPER(CAST(<level> AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END) AS …]
/// FROM <table>
/// WHERE CAST(_timestamp AS BIGINT) >= <from> AND CAST(_timestamp AS BIGINT) < <to>
///       [AND <col> = '<value>' …] [AND <col> ILIKE '%<term>%' …]
/// GROUP BY 1
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarCounts {
    /// 一行落在第 `(_timestamp - origin) / width` 根柱子：微秒，整数除法。
    pub origin: i64,
    pub width: i64,
    /// WHERE 限定的时间窗 `[from, to)`，微秒。
    pub from: i64,
    pub to: i64,
    /// 等值过滤 `(列名, 字面量)`。
    pub filters: Vec<(String, String)>,
    /// 子串过滤 `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub contains: Vec<(String, String)>,
    /// 错误行另外计数时，判断错误所读的 level 列。
    pub errors: Option<String>,
}

/// `SELECT count(*)` 的条件；不带 WHERE 时都为空，数的是整张表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlainCount {
    /// 等值过滤 `(列名, 字面量)`。
    pub filters: Vec<(String, String)>,
    /// 子串过滤 `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub contains: Vec<(String, String)>,
}

/// `sql` 只统计 `table` 的行数时，返回它的条件。
///
/// 其它任何形态都返回 `None`：多个投影项、`count(col)`、`DISTINCT`、`GROUP BY`、JOIN、
/// 子查询、`LIMIT` / `ORDER BY`，以及 WHERE 里任何不是 `列 = '字面量'` 与
/// `列 ILIKE '%子串%'` 合取的条件（`OR`、`NOT`、比较、`LIKE`、带通配符的 `ILIKE`、函数、
/// 时间条件）。
pub fn plain_count(sql: &str, table: &str) -> Option<PlainCount> {
    let select = plain_select(sql)?;

    let ungrouped = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers) if exprs.is_empty() && modifiers.is_empty()
    );

    let [item] = select.projection.as_slice() else {
        return None;
    };

    if !ungrouped || !counts_rows(item_expr(item)?) || !reads_only(&select.from, table) {
        return None;
    }

    let mut conditions = Conditions::default();

    if let Some(selection) = &select.selection {
        collect_conditions(selection, &mut conditions)?;
    }

    Some(PlainCount {
        filters: conditions.filters,
        contains: conditions.contains,
    })
}

/// `sql` 是按时间分柱数 `table` 行数的 explorer 直方图语句时，返回它的柱子与过滤条件。
///
/// 只认 [`BarCounts`] 所示的一种写法：投影的顺序与形状、WHERE 恰好是时间上下界、
/// `列 = '字面量'` 与 `列 ILIKE '%子串%'` 的合取、`GROUP BY 1`。柱宽须为正，时间窗两端减去 `origin` 不得溢出，
/// 窗内每一行算柱号便都不溢出。level 由几个列 `COALESCE` 而来的写法也返回 `None`：
/// 索引答不出一行先在哪一列有值。
pub fn bar_counts(sql: &str, table: &str) -> Option<BarCounts> {
    let select = plain_select(sql)?;

    let grouped_by_bar = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers)
            if modifiers.is_empty() && matches!(exprs.as_slice(), [expr] if integer(expr) == Some(1))
    );

    if !grouped_by_bar || !reads_only(&select.from, table) {
        return None;
    }

    let (bar, rows, errors) = match select.projection.as_slice() {
        [bar, rows] => (bar, rows, None),
        [bar, rows, errors] => (bar, rows, Some(errors)),
        _ => return None,
    };

    let (origin, width) = bar_index(item_expr(bar)?)?;

    if !counts_rows(item_expr(rows)?) {
        return None;
    }

    let errors = match errors {
        Some(item) => Some(error_level(item_expr(item)?)?),
        None => None,
    };

    let mut conditions = Conditions {
        window: Some(TimeWindow::default()),
        ..Conditions::default()
    };

    collect_conditions(select.selection.as_ref()?, &mut conditions)?;

    let window = conditions.window?;
    let (from, to) = (window.from?, window.to?);
    let fits = width > 0 && from.checked_sub(origin).is_some() && to.checked_sub(origin).is_some();

    fits.then_some(BarCounts {
        origin,
        width,
        from,
        to,
        filters: conditions.filters,
        contains: conditions.contains,
        errors,
    })
}

/// 单条、不带 CTE / ORDER BY / LIMIT / DISTINCT / HAVING / 窗口等修饰的 `SELECT`。
/// GROUP BY 由调用方判断。
fn plain_select(sql: &str) -> Option<Select> {
    let query = single_query(sql)?;

    if query.order_by.is_some() || query.limit_clause.is_some() {
        return None;
    }

    let SetExpr::Select(select) = *query.body else {
        return None;
    };

    plain(&select).then_some(*select)
}

/// `sql` 是单条查询，没有 CTE 与 `FETCH` / `FOR` / `SETTINGS` 之类的修饰；ORDER BY 与 LIMIT
/// 由调用方判断。
fn single_query(sql: &str) -> Option<Query> {
    let statements = Parser::parse_sql(&GenericDialect, sql).ok()?;

    let Ok([Statement::Query(query)]) = <[Statement; 1]>::try_from(statements) else {
        return None;
    };

    let plain_query = query.with.is_none()
        && query.fetch.is_none()
        && query.locks.is_empty()
        && query.for_clause.is_none()
        && query.settings.is_none()
        && query.format_clause.is_none()
        && query.pipe_operators.is_empty();

    plain_query.then_some(*query)
}

/// SELECT 没有 DISTINCT、HAVING、窗口、`INTO` 之类的修饰。GROUP BY 由调用方判断。
fn plain(select: &Select) -> bool {
    select.distinct.is_none()
        && select.select_modifiers.is_none()
        && select.optimizer_hints.is_empty()
        && select.top.is_none()
        && select.exclude.is_none()
        && select.into.is_none()
        && select.lateral_views.is_empty()
        && select.prewhere.is_none()
        && select.connect_by.is_empty()
        && select.cluster_by.is_empty()
        && select.distribute_by.is_empty()
        && select.sort_by.is_empty()
        && select.having.is_none()
        && select.named_window.is_empty()
        && select.qualify.is_none()
        && select.value_table_mode.is_none()
}

fn item_expr(item: &SelectItem) -> Option<&Expr> {
    match item {
        SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => Some(expr),
        _ => None,
    }
}

/// `count(*)` 或 `count(1)`。
fn counts_rows(expr: &Expr) -> bool {
    match plain_call(expr, "count") {
        Some([FunctionArg::Unnamed(FunctionArgExpr::Wildcard)]) => true,
        Some([FunctionArg::Unnamed(FunctionArgExpr::Expr(one))]) => integer(one) == Some(1),
        _ => false,
    }
}

/// 名为 `name` 的普通函数调用的参数：没有 `DISTINCT`、`FILTER`、窗口或其它子句。
fn plain_call<'a>(expr: &'a Expr, name: &str) -> Option<&'a [FunctionArg]> {
    let Expr::Function(function) = expr else {
        return None;
    };

    let [ObjectNamePart::Identifier(called)] = function.name.0.as_slice() else {
        return None;
    };

    let plain = called.value.eq_ignore_ascii_case(name)
        && !function.uses_odbc_syntax
        && matches!(function.parameters, FunctionArguments::None)
        && function.filter.is_none()
        && function.null_treatment.is_none()
        && function.over.is_none()
        && function.within_group.is_empty();

    let FunctionArguments::List(list) = &function.args else {
        return None;
    };

    (plain && list.duplicate_treatment.is_none() && list.clauses.is_empty())
        .then_some(list.args.as_slice())
}

/// `(CAST(_timestamp AS BIGINT) - origin) / width`，返回 `(origin, width)`。
fn bar_index(expr: &Expr) -> Option<(i64, i64)> {
    let Expr::BinaryOp {
        left,
        op: BinaryOperator::Divide,
        right: width,
    } = expr
    else {
        return None;
    };

    let Expr::Nested(offset) = left.as_ref() else {
        return None;
    };

    let Expr::BinaryOp {
        left: micros,
        op: BinaryOperator::Minus,
        right: origin,
    } = offset.as_ref()
    else {
        return None;
    };

    if !timestamp_micros(micros) {
        return None;
    }

    Some((integer(origin)?, integer(width)?))
}

/// `SUM(CASE WHEN UPPER(CAST(<level> AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END)`，返回
/// level 列名。
fn error_level(expr: &Expr) -> Option<String> {
    let [FunctionArg::Unnamed(FunctionArgExpr::Expr(case))] = plain_call(expr, "sum")? else {
        return None;
    };

    let Expr::Case {
        operand: None,
        conditions,
        else_result: Some(otherwise),
        ..
    } = case
    else {
        return None;
    };

    let [CaseWhen { condition, result }] = conditions.as_slice() else {
        return None;
    };

    let Expr::BinaryOp {
        left: upper,
        op: BinaryOperator::Eq,
        right: error,
    } = condition
    else {
        return None;
    };

    let says_error = matches!(
        error.as_ref(),
        Expr::Value(value) if value.value == Value::SingleQuotedString("ERROR".to_owned())
    );

    if !says_error || integer(result) != Some(1) || integer(otherwise) != Some(0) {
        return None;
    }

    let [FunctionArg::Unnamed(FunctionArgExpr::Expr(text))] = plain_call(upper, "upper")? else {
        return None;
    };

    let Expr::Cast {
        kind: CastKind::Cast,
        expr: level,
        data_type: DataType::Varchar(None),
        array: false,
        format: None,
    } = text
    else {
        return None;
    };

    column_name(level)
}

/// `CAST(_timestamp AS BIGINT)`：行时间的微秒数。
fn timestamp_micros(expr: &Expr) -> bool {
    let Expr::Cast {
        kind: CastKind::Cast,
        expr: column,
        data_type: DataType::BigInt(None),
        array: false,
        format: None,
    } = expr
    else {
        return false;
    };

    matches!(column.as_ref(), Expr::Identifier(_))
        && column_name(column).is_some_and(|name| name == TIMESTAMP_COLUMN)
}

/// 整数字面量，可以带负号或套括号。
fn integer(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::Number(digits, false) => digits.parse().ok(),
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => integer(expr)?.checked_neg(),
        Expr::Nested(inner) => integer(inner),
        _ => None,
    }
}

/// FROM 只有一张未加修饰的 `table`。
fn reads_only(from: &[TableWithJoins], table: &str) -> bool {
    let [only] = from else {
        return false;
    };

    let TableFactor::Table {
        name,
        args: None,
        with_hints,
        version: None,
        with_ordinality: false,
        partitions,
        json_path: None,
        sample: None,
        index_hints,
        ..
    } = &only.relation
    else {
        return false;
    };

    let [ObjectNamePart::Identifier(ident)] = name.0.as_slice() else {
        return false;
    };

    only.joins.is_empty()
        && with_hints.is_empty()
        && partitions.is_empty()
        && index_hints.is_empty()
        && ident.value == table
}

/// 列引用在 DataFusion 里解析成的列名：带引号的原样，不带引号的转成小写。限定名取最后一段。
pub fn column_name(expr: &Expr) -> Option<String> {
    let ident = match expr {
        Expr::Identifier(ident) => ident,
        Expr::CompoundIdentifier(parts) => parts.last()?,
        _ => return None,
    };

    Some(match ident.quote_style {
        Some(_) => ident.value.clone(),
        None => ident.value.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters(sql: &str) -> Option<Vec<(String, String)>> {
        plain_count(sql, "requests").map(|count| count.filters)
    }

    fn pair(column: &str, value: &str) -> (String, String) {
        (column.to_owned(), value.to_owned())
    }

    /// 与 explorer 直方图写出的语句同形。
    fn histogram(origin: &str, level: &str, filters: &str) -> String {
        format!(
            "SELECT (CAST(_timestamp AS BIGINT) - {origin}) / 60000000 AS __histogram_bucket, \
             COUNT(*) AS __histogram_total{level} \
             FROM \"requests\" \
             WHERE CAST(_timestamp AS BIGINT) >= 1000 AND CAST(_timestamp AS BIGINT) < 9000{filters} \
             GROUP BY 1"
        )
    }

    const ERRORS: &str = ", SUM(CASE WHEN UPPER(CAST(\"level\" AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END) AS __histogram_errors";

    #[test]
    fn accepts_counts_with_equality_conjunctions() {
        assert_eq!(filters("SELECT count(*) FROM requests"), Some(vec![]));
        assert_eq!(
            filters("select COUNT(1) as n from \"requests\""),
            Some(vec![])
        );
        assert_eq!(
            filters("SELECT count(*) AS cnt FROM requests WHERE path = '/api/bar'"),
            Some(vec![pair("path", "/api/bar")])
        );
        assert_eq!(
            filters(
                "SELECT count(*) FROM requests r WHERE ('GET' = r.method) AND (path = 'a' AND status_text = 'ok')"
            ),
            Some(vec![
                pair("method", "GET"),
                pair("path", "a"),
                pair("status_text", "ok")
            ])
        );
    }

    /// `json_text(列, 'key')` 的等值与 `IS NOT NULL` 是键值索引里的一个词；`IN` 有几个词，不计。
    #[test]
    fn json_member_conditions_count_as_key_value_terms() {
        assert_eq!(
            filters(
                "SELECT count(*) FROM requests WHERE json_text(ctx, 'plan') = 'pro' \
                 AND json_text(ctx, 'beta') IS NOT NULL"
            ),
            Some(vec![
                pair("ctx\0kv", "=plan\0pro"),
                pair("ctx\0kv", "?beta")
            ])
        );
        assert_eq!(
            filters("SELECT count(*) FROM requests WHERE json_text(ctx, 'plan') IN ('a', 'b')"),
            None
        );
    }

    #[test]
    fn names_columns_the_way_datafusion_resolves_them() {
        assert_eq!(
            filters("SELECT count(*) FROM requests WHERE Path = 'a' AND \"Method\" = 'GET'"),
            Some(vec![pair("path", "a"), pair("Method", "GET")])
        );
    }

    #[test]
    fn reads_substring_conditions_lower_cased() {
        assert_eq!(
            plain_count(
                "SELECT count(*) FROM requests WHERE message ILIKE '%TimeOut%' AND path = 'a'",
                "requests"
            ),
            Some(PlainCount {
                filters: vec![pair("path", "a")],
                contains: vec![pair("message", "timeout")],
            })
        );
    }

    #[test]
    fn rejects_everything_a_row_count_cannot_answer() {
        for sql in [
            "SELECT count(path) FROM requests",
            "SELECT count(DISTINCT path) FROM requests",
            "SELECT count(*), max(status) FROM requests",
            "SELECT count(*) FROM requests WHERE path = 'a' OR path = 'b'",
            "SELECT count(*) FROM requests WHERE path = 'a' AND (host = 'b' OR host = 'c')",
            "SELECT count(*) FROM requests WHERE NOT path = 'a'",
            "SELECT count(*) FROM requests WHERE path <> 'a'",
            "SELECT count(*) FROM requests WHERE status = 500",
            "SELECT count(*) FROM requests WHERE path LIKE '/api/%'",
            "SELECT count(*) FROM requests WHERE path LIKE '%bar%'",
            "SELECT count(*) FROM requests WHERE path ILIKE 'bar%'",
            "SELECT count(*) FROM requests WHERE path ILIKE '%b_r%'",
            "SELECT count(*) FROM requests WHERE path ILIKE '%b\\%r%'",
            "SELECT count(*) FROM requests WHERE path ILIKE '%%'",
            "SELECT count(*) FROM requests WHERE path NOT ILIKE '%bar%'",
            "SELECT count(*) FROM requests WHERE path ILIKE '%bar%' ESCAPE '!'",
            "SELECT count(*) FROM requests WHERE _timestamp >= 1",
            "SELECT count(*) FROM requests WHERE CAST(_timestamp AS BIGINT) >= 1",
            "SELECT count(*) FROM requests WHERE lower(path) = 'a'",
            "SELECT count(*) FROM requests GROUP BY path",
            "SELECT count(*) FROM requests HAVING count(*) > 1",
            "SELECT count(*) FROM requests LIMIT 1",
            "SELECT count(*) FROM requests ORDER BY 1",
            "SELECT DISTINCT count(*) FROM requests",
            "SELECT count(*) FROM other",
            "SELECT count(*) FROM requests JOIN other ON requests.a = other.a",
            "SELECT count(*) FROM (SELECT * FROM requests) t",
            "WITH t AS (SELECT * FROM requests) SELECT count(*) FROM t",
            "SELECT count(*) FILTER (WHERE path = 'a') FROM requests",
            "SELECT count(*) OVER () FROM requests",
            "SELECT count(*) FROM requests; SELECT 1",
            "not sql",
        ] {
            assert_eq!(plain_count(sql, "requests"), None, "{sql}");
        }
    }

    #[test]
    fn reads_the_bars_of_an_explorer_histogram() {
        assert_eq!(
            bar_counts(&histogram("600", "", ""), "requests"),
            Some(BarCounts {
                origin: 600,
                width: 60_000_000,
                from: 1000,
                to: 9000,
                filters: vec![],
                contains: vec![],
                errors: None,
            })
        );

        assert_eq!(
            bar_counts(
                &histogram(
                    "-500",
                    ERRORS,
                    " AND \"environment\" = 'prod' AND (host = 'a') AND message ILIKE '%panic%'"
                ),
                "requests"
            ),
            Some(BarCounts {
                origin: -500,
                width: 60_000_000,
                from: 1000,
                to: 9000,
                filters: vec![pair("environment", "prod"), pair("host", "a")],
                contains: vec![pair("message", "panic")],
                errors: Some("level".to_owned()),
            })
        );
    }

    #[test]
    fn rejects_histograms_an_index_cannot_count() {
        let coalesced = ", SUM(CASE WHEN UPPER(COALESCE(CAST(\"level\" AS VARCHAR), CAST(\"body_level\" AS VARCHAR))) = 'ERROR' THEN 1 ELSE 0 END) AS e";

        for sql in [
            // Errors read from several level columns, or counted another way.
            histogram("0", coalesced, ""),
            histogram("0", &ERRORS.replace("'ERROR'", "'WARN'"), ""),
            histogram("0", &ERRORS.replace("THEN 1", "THEN 2"), ""),
            histogram("0", &ERRORS.replace("UPPER", "LOWER"), ""),
            // Conditions other than equalities and substrings on top of the time window.
            histogram("0", "", " AND message ILIKE 'x%'"),
            histogram("0", "", " AND message LIKE '%x%'"),
            histogram("0", "", " AND status = 500"),
            histogram("0", "", " OR host = 'a'"),
            // An OR in brackets among the conditions: an index counts only what every row meets.
            histogram("0", "", " AND (host = 'a' OR host = 'b')"),
            histogram(
                "0",
                "",
                " AND host = 'a' AND (message ILIKE '%x%' OR level = 'error')",
            ),
            // A bar origin that is not a whole number, or a time window that overflows it.
            histogram("1.5", "", ""),
            histogram("-9223372036854775807", "", ""),
            histogram("0", "", "").replace(" GROUP BY 1", " GROUP BY 1 ORDER BY 1"),
            histogram("0", "", "").replace(" GROUP BY 1", " GROUP BY 1 LIMIT 3"),
            histogram("0", "", "").replace(" GROUP BY 1", " GROUP BY 2"),
            histogram("0", "", "").replace("/ 60000000", "/ 0"),
            histogram("0", "", "").replace("/ 60000000", "/ -60000000"),
            histogram("0", "", "").replace("\"requests\"", "\"other\""),
            histogram("0", "", "").replace("COUNT(*)", "COUNT(host)"),
            histogram("0", "", "").replace("AS BIGINT) - 0", "AS INT) - 0"),
            histogram("0", "", "").replace("AND CAST(_timestamp AS BIGINT) < 9000", ""),
            histogram("0", "", "").replace("CAST(_timestamp AS BIGINT) >= 1000 AND ", ""),
        ] {
            assert_eq!(bar_counts(&sql, "requests"), None, "{sql}");
        }
    }

    #[test]
    fn keeps_the_narrowest_time_window() {
        let sql = histogram(
            "0",
            "",
            " AND CAST(_timestamp AS BIGINT) >= 2000 AND CAST(_timestamp AS BIGINT) < 10000",
        );
        let bars = bar_counts(&sql, "requests").expect("a histogram");

        assert_eq!((bars.from, bars.to), (2000, 9000));
    }
}
