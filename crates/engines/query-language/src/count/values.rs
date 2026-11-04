// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 识别字段分析里“最常见的值”的语句：
//!
//! ```sql
//! SELECT NULLIF(<col>, '') AS …, COUNT(*) AS … FROM <table>
//! WHERE [CAST(_timestamp AS BIGINT) >= <from> AND CAST(_timestamp AS BIGINT) < <to> AND]
//!       [<col> = '<value>' AND …] NULLIF(<col>, '') IS NOT NULL
//! GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT <n>
//! ```
//!
//! 结果只取决于每个值在满足条件的行里出现几次：整值索引的词典与倒排表就能数出来。

use sqlparser::ast::{GroupByExpr, LimitClause, OrderByKind, SetExpr, Statement};

use super::{
    conditions::{Conditions, TimeWindow, collect_conditions, nonempty},
    counts_rows, integer, item_expr, plain, reads_only, single_query,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopValues {
    /// 被计数的列。
    pub column: String,
    /// WHERE 限定的时间窗 `[from, to)`，微秒；没有时间条件的一边为 `None`。
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// 等值过滤 `(列名, 字面量)`。
    pub filters: Vec<(String, String)>,
    /// 子串过滤 `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub contains: Vec<(String, String)>,
    /// 取前几个值。
    pub limit: usize,
    /// 去掉 ORDER BY 与 LIMIT 的同一语句：数出每个值，不排序、不截断。
    pub grouped: String,
}

/// `sql` 是按 `table` 一列的值计数、取出现最多的前 n 个值（同样多时按值升序）的语句时，返回
/// 这列、过滤条件与 n。其它任何形态都返回 `None`。
pub fn top_values(sql: &str, table: &str) -> Option<TopValues> {
    let mut query = single_query(sql)?;

    let limit = match query.limit_clause.as_ref()? {
        LimitClause::LimitOffset {
            limit: Some(limit),
            offset: None,
            limit_by,
        } if limit_by.is_empty() => usize::try_from(integer(limit)?).ok()?,
        _ => return None,
    };

    let order = query.order_by.as_ref()?;

    let by_count_then_value = order.interpolate.is_none()
        && matches!(&order.kind, OrderByKind::Expressions(keys)
            if matches!(keys.as_slice(), [count, value]
                if count.with_fill.is_none()
                    && value.with_fill.is_none()
                    && integer(&count.expr) == Some(2)
                    && count.options.asc == Some(false)
                    && integer(&value.expr) == Some(1)
                    && value.options.asc != Some(false)));

    if !by_count_then_value {
        return None;
    }

    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };

    let grouped_by_value = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers)
            if modifiers.is_empty() && matches!(exprs.as_slice(), [expr] if integer(expr) == Some(1))
    );

    if !plain(select) || !grouped_by_value || !reads_only(&select.from, table) {
        return None;
    }

    let [value, rows] = select.projection.as_slice() else {
        return None;
    };

    let column = nonempty(item_expr(value)?)?;

    if !counts_rows(item_expr(rows)?) {
        return None;
    }

    let mut conditions = Conditions {
        window: Some(TimeWindow::default()),
        present: Some(Vec::new()),
        ..Conditions::default()
    };

    collect_conditions(select.selection.as_ref()?, &mut conditions)?;

    // 只数有值的行，正是语句自己选出的那一列。
    let present = conditions.present.unwrap_or_default();

    if !present.iter().any(|present| present == &column) || present.iter().any(|p| p != &column) {
        return None;
    }

    let window = conditions.window.unwrap_or_default();

    query.order_by = None;
    query.limit_clause = None;

    Some(TopValues {
        column,
        from: window.from,
        to: window.to,
        filters: conditions.filters,
        contains: conditions.contains,
        limit,
        grouped: Statement::Query(Box::new(query)).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与字段分析写出的语句同形。
    fn statement(conditions: &str, order: &str) -> String {
        format!(
            "SELECT NULLIF(\"service\", '') AS __fs_value, COUNT(*) AS __fs_count FROM \"app\" \
             WHERE CAST(_timestamp AS BIGINT) >= 1000 AND CAST(_timestamp AS BIGINT) < 9000{conditions} \
             AND NULLIF(\"service\", '') IS NOT NULL GROUP BY 1 {order} LIMIT 10"
        )
    }

    #[test]
    fn reads_the_column_the_window_and_the_filters() {
        let values = top_values(
            &statement(" AND \"environment\" = 'prod'", "ORDER BY 2 DESC, 1 ASC"),
            "app",
        )
        .expect("a top values statement");

        assert_eq!(values.column, "service");
        assert_eq!((values.from, values.to), (Some(1000), Some(9000)));
        assert_eq!(
            values.filters,
            vec![("environment".to_owned(), "prod".to_owned())]
        );
        assert_eq!(values.limit, 10);
        assert!(!values.grouped.contains("ORDER BY"));
        assert!(!values.grouped.contains("LIMIT"));
        assert!(values.grouped.contains("GROUP BY 1"));
    }

    #[test]
    fn rejects_other_orders_and_conditions() {
        for sql in [
            statement("", "ORDER BY 2 DESC"),
            statement("", "ORDER BY 2 ASC, 1 ASC"),
            statement("", "ORDER BY 1 ASC, 2 DESC"),
            statement(" AND \"service\" LIKE 'a%'", "ORDER BY 2 DESC, 1 ASC"),
            statement(
                " AND (\"host\" = 'a' OR \"host\" = 'b')",
                "ORDER BY 2 DESC, 1 ASC",
            ),
            statement(
                " AND NULLIF(\"host\", '') IS NOT NULL",
                "ORDER BY 2 DESC, 1 ASC",
            ),
            statement("", "ORDER BY 2 DESC, 1 ASC").replace("LIMIT 10", "LIMIT 10 OFFSET 5"),
            statement("", "ORDER BY 2 DESC, 1 ASC").replace("COUNT(*)", "COUNT(\"service\")"),
            statement("", "ORDER BY 2 DESC, 1 ASC")
                .replace("NULLIF(\"service\", '') AS", "\"service\" AS"),
            statement("", "ORDER BY 2 DESC, 1 ASC").replace("\"app\"", "\"other\""),
        ] {
            assert_eq!(top_values(&sql, "app"), None, "{sql}");
        }
    }
}
