// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 识别字段分析里一个文本字段的汇总语句：
//!
//! ```sql
//! SELECT COUNT(*) AS <total>, COUNT(NULLIF(<col>, '')) AS <present>,
//!        approx_distinct(NULLIF(<col>, '')) AS <distinct>
//! FROM <table>
//! [WHERE [CAST(_timestamp AS BIGINT) >= <from> AND CAST(_timestamp AS BIGINT) < <to>]
//!        [AND <col> = '<value>' …] [AND <col> ILIKE '%<term>%' …]]
//! ```
//!
//! 结果只取决于每个值出现在几行：整值索引的词典就能数出来，不同值的个数也就不必估计。

use sqlparser::ast::{FunctionArg, FunctionArgExpr, GroupByExpr, SelectItem};

use super::{
    conditions::{Conditions, TimeWindow, collect_conditions, nonempty},
    counts_rows, item_expr, plain_call, plain_select, reads_only,
};

/// 数出每个值的语句里，值与行数这两列的名字。
const SUMMARY_VALUE: &str = "__summary_value";
const SUMMARY_ROWS: &str = "__summary_rows";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSummary {
    /// 被汇总的列。
    pub column: String,
    /// WHERE 限定的时间窗 `[from, to)`，微秒；没有时间条件的一边为 `None`。
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// 等值过滤 `(列名, 字面量)`。
    pub filters: Vec<(String, String)>,
    /// 子串过滤 `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub contains: Vec<(String, String)>,
    /// 结果三列的名字：行数、有值的行数、不同值的个数。
    pub names: [String; 3],
    /// 按同样的条件数出每个值的语句：第一列是值（没有值的行是 NULL），第二列是行数。
    pub grouped: String,
}

/// `sql` 是汇总 `table` 一个文本字段的语句时，返回这列、条件、结果列名与数出每个值的语句。
/// 其它任何形态都返回 `None`，结果列没有别名的也是：引擎得照原样的名字交回结果。
pub fn field_summary(sql: &str, table: &str) -> Option<FieldSummary> {
    let select = plain_select(sql)?;

    let ungrouped = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers) if exprs.is_empty() && modifiers.is_empty()
    );

    if !ungrouped || !reads_only(&select.from, table) {
        return None;
    }

    let [total, present, distinct] = select.projection.as_slice() else {
        return None;
    };

    if !counts_rows(item_expr(total)?) {
        return None;
    }

    let value = counted_value(item_expr(present)?, "count")?;
    let column = nonempty(value)?;

    if nonempty(counted_value(item_expr(distinct)?, "approx_distinct")?)? != column {
        return None;
    }

    let names = [alias(total)?, alias(present)?, alias(distinct)?];

    let mut conditions = Conditions {
        window: Some(TimeWindow::default()),
        ..Conditions::default()
    };

    if let Some(selection) = &select.selection {
        collect_conditions(selection, &mut conditions)?;
    }

    let window = conditions.window.unwrap_or_default();

    let condition = select
        .selection
        .as_ref()
        .map(|selection| format!(" WHERE {selection}"))
        .unwrap_or_default();

    let grouped = format!(
        "SELECT {value} AS {SUMMARY_VALUE}, COUNT(*) AS {SUMMARY_ROWS} FROM {from}{condition} \
         GROUP BY 1",
        from = select.from[0],
    );

    Some(FieldSummary {
        column,
        from: window.from,
        to: window.to,
        filters: conditions.filters,
        contains: conditions.contains,
        names,
        grouped,
    })
}

/// `name(<value>)` 的唯一参数。
fn counted_value<'a>(
    expr: &'a sqlparser::ast::Expr,
    name: &str,
) -> Option<&'a sqlparser::ast::Expr> {
    match plain_call(expr, name)? {
        [FunctionArg::Unnamed(FunctionArgExpr::Expr(value))] => Some(value),
        _ => None,
    }
}

/// 结果列的别名，按 DataFusion 的规则：带引号的原样，不带引号的转成小写。
fn alias(item: &SelectItem) -> Option<String> {
    let SelectItem::ExprWithAlias { alias, .. } = item else {
        return None;
    };

    Some(match alias.quote_style {
        Some(_) => alias.value.clone(),
        None => alias.value.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与字段分析写出的语句同形。
    fn statement(conditions: &str) -> String {
        format!(
            "SELECT COUNT(*) AS __fs_total, COUNT(NULLIF(\"service\", '')) AS __fs_present, \
             approx_distinct(NULLIF(\"service\", '')) AS __fs_distinct FROM \"app\" \
             WHERE CAST(_timestamp AS BIGINT) >= 1000 AND CAST(_timestamp AS BIGINT) < 9000{conditions}"
        )
    }

    #[test]
    fn reads_the_column_the_window_the_names_and_the_conditions() {
        let summary = field_summary(&statement(" AND message ILIKE '%Panic%'"), "app")
            .expect("a summary statement");

        assert_eq!(summary.column, "service");
        assert_eq!((summary.from, summary.to), (Some(1000), Some(9000)));
        assert!(summary.filters.is_empty());
        assert_eq!(
            summary.contains,
            vec![("message".to_owned(), "panic".to_owned())]
        );
        assert_eq!(
            summary.names,
            ["__fs_total", "__fs_present", "__fs_distinct"].map(str::to_owned)
        );
        assert_eq!(
            summary.grouped,
            "SELECT NULLIF(\"service\", '') AS __summary_value, COUNT(*) AS __summary_rows \
             FROM \"app\" WHERE CAST(_timestamp AS BIGINT) >= 1000 AND CAST(_timestamp AS \
             BIGINT) < 9000 AND message ILIKE '%Panic%' GROUP BY 1"
        );
    }

    #[test]
    fn a_summary_without_conditions_counts_the_whole_table() {
        let summary = field_summary(
            "SELECT COUNT(*) AS t, COUNT(NULLIF(service, '')) AS p, \
             approx_distinct(NULLIF(service, '')) AS d FROM app",
            "app",
        )
        .expect("a summary statement");

        assert_eq!((summary.from, summary.to), (None, None));
        assert_eq!(
            summary.grouped,
            "SELECT NULLIF(service, '') AS __summary_value, COUNT(*) AS __summary_rows FROM \
             app GROUP BY 1"
        );
    }

    #[test]
    fn rejects_other_summaries() {
        for sql in [
            statement(" AND status = 500"),
            statement(" OR host = 'a'"),
            statement(" AND (host = 'a' OR host = 'b')"),
            statement("").replace(" AS __fs_total", ""),
            statement("").replace("COUNT(*)", "COUNT(host)"),
            statement("").replace(
                "approx_distinct(NULLIF(\"service\", ''))",
                "approx_distinct(NULLIF(\"host\", ''))",
            ),
            statement("").replace("approx_distinct", "count"),
            statement("").replace(
                "NULLIF(\"service\", '')) AS __fs_present",
                "\"service\") AS __fs_present",
            ),
            statement("").replace("\"app\"", "\"other\""),
            format!("{} GROUP BY 1", statement("")),
            format!("{} LIMIT 1", statement("")),
            statement("").replace(
                " FROM \"app\"",
                ", MIN(\"service\") AS __fs_min FROM \"app\"",
            ),
        ] {
            assert_eq!(field_summary(&sql, "app"), None, "{sql}");
        }
    }
}
