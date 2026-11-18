// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 识别按一列的值数行的语句：
//!
//! ```sql
//! SELECT <列> [AS …], COUNT(*) [AS …] FROM <table>
//! [WHERE <行时间的上下界> [AND <列> = '<字面量>' …]]
//! GROUP BY <列> [ORDER BY <值或行数> [ASC | DESC] [NULLS FIRST | LAST], …] [LIMIT <n>]
//! ```
//!
//! 两列可以互换位置；GROUP BY 写列名或位置，ORDER BY 写输出的列名、位置或 `COUNT(*)`。结果只
//! 取决于每个值（包括没有值）出现在几行：没有等值条件时，整个落在时间窗里的文件，整值索引的词典
//! 就能数出来；有等值条件时，按这些字段记下的每分钟行数能数出来。
//! 字段分析写出的 `NULLIF(列, '')` 形态见 [`super::top_values`]。

use sqlparser::ast::{
    Expr, GroupByExpr, Ident, LimitClause, OrderByExpr, OrderByKind, SelectItem, SetExpr, Statement,
};

use super::{
    column_name,
    conditions::{Conditions, TimeWindow, collect_conditions},
    counts_rows, integer, item_expr, plain, reads_only, single_query,
};

/// ORDER BY 的一项比较的是组的值，还是组的行数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKey {
    Value,
    Rows,
}

/// ORDER BY 的一项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupOrder {
    pub key: GroupKey,
    pub descending: bool,
    /// 没有值的组排在最前。语句没写时与 DataFusion 一样：降序在前，升序在后。
    pub nulls_first: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedCounts {
    /// 按它的值分组的列。
    pub column: String,
    /// 值是结果的第一列（`SELECT 列, COUNT(*)`）；否则行数是第一列。
    pub value_first: bool,
    /// WHERE 限定的时间窗 `[from, to)`，微秒；没有时间条件的一边为 `None`。
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// Equality conditions `(column, literal)`.
    pub filters: Vec<(String, String)>,
    /// 依次比较的 ORDER BY 各项；没有 ORDER BY 时为空。
    pub order: Vec<GroupOrder>,
    pub limit: Option<usize>,
    /// 去掉 ORDER BY 与 LIMIT 的同一语句：数出每个值，不排序、不截断。
    pub grouped: String,
}

/// 结果里的一列：被分组的列，或行数。
struct Output {
    /// 结果里这一列的名字：别名，没有别名时是列名；行数没有别名时为 `None`。
    name: Option<String>,
    position: i64,
}

/// `sql` 是按 `table` 一列的值数行、只带行时间条件与等值条件的语句时，返回这列、时间窗、等值
/// 条件、排序与行数上限。其它任何形态都返回 `None`。
pub fn grouped_counts(sql: &str, table: &str) -> Option<GroupedCounts> {
    let mut query = single_query(sql)?;

    let limit = match query.limit_clause.as_ref() {
        None => None,
        Some(LimitClause::LimitOffset {
            limit: Some(limit),
            offset: None,
            limit_by,
        }) if limit_by.is_empty() => Some(usize::try_from(integer(limit)?).ok()?),
        Some(_) => return None,
    };

    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };

    if !plain(select) || !reads_only(&select.from, table) {
        return None;
    }

    let [first, second] = select.projection.as_slice() else {
        return None;
    };

    let (column, value, rows) = match (grouped_column(first), grouped_column(second)) {
        (Some((column, name)), None) if counts_rows(item_expr(second)?) => (
            column,
            Output {
                name: Some(name),
                position: 1,
            },
            Output {
                name: alias(second),
                position: 2,
            },
        ),
        (None, Some((column, name))) if counts_rows(item_expr(first)?) => (
            column,
            Output {
                name: Some(name),
                position: 2,
            },
            Output {
                name: alias(first),
                position: 1,
            },
        ),
        _ => return None,
    };

    // 两列同名时 ORDER BY 说不清比较的是哪一列。
    if value.name == rows.name {
        return None;
    }

    let grouped_by_column = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers)
            if modifiers.is_empty()
                && matches!(exprs.as_slice(), [expr]
                    if integer(expr) == Some(value.position)
                        || (is_column(expr) && column_name(expr).as_ref() == Some(&column)))
    );

    if !grouped_by_column {
        return None;
    }

    let mut conditions = Conditions {
        window: Some(TimeWindow::default()),
        ..Conditions::default()
    };

    if let Some(selection) = &select.selection {
        collect_conditions(selection, &mut conditions)?;
    }

    if !conditions.contains.is_empty() {
        return None;
    }

    let order = match query.order_by.as_ref() {
        None => Vec::new(),
        Some(order) if order.interpolate.is_none() => match &order.kind {
            OrderByKind::Expressions(keys) => keys
                .iter()
                .map(|key| order_item(key, &column, &value, &rows))
                .collect::<Option<Vec<_>>>()?,
            OrderByKind::All(_) => return None,
        },
        Some(_) => return None,
    };

    let window = conditions.window.unwrap_or_default();

    query.order_by = None;
    query.limit_clause = None;

    Some(GroupedCounts {
        column,
        value_first: value.position == 1,
        from: window.from,
        to: window.to,
        filters: conditions.filters,
        order,
        limit,
        grouped: Statement::Query(Box::new(query)).to_string(),
    })
}

/// 一个直接读某列的输出项：那一列，与它在结果里的名字。
fn grouped_column(item: &SelectItem) -> Option<(String, String)> {
    let expr = item_expr(item)?;

    if !is_column(expr) {
        return None;
    }

    let column = column_name(expr)?;
    let name = alias(item).unwrap_or_else(|| column.clone());

    Some((column, name))
}

/// 输出项的别名，按 DataFusion 的规则解析。
pub(super) fn alias(item: &SelectItem) -> Option<String> {
    match item {
        SelectItem::ExprWithAlias { alias, .. } => Some(identifier(alias)),
        _ => None,
    }
}

fn identifier(ident: &Ident) -> String {
    column_name(&Expr::Identifier(ident.clone())).unwrap_or_default()
}

pub(super) fn is_column(expr: &Expr) -> bool {
    matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
}

/// ORDER BY 的一项比较的是哪一列：输出的位置、输出的名字、`COUNT(*)`，或被分组的列本身。
fn order_item(
    key: &OrderByExpr,
    column: &str,
    value: &Output,
    rows: &Output,
) -> Option<GroupOrder> {
    if key.with_fill.is_some() {
        return None;
    }

    let name = is_column(&key.expr)
        .then(|| column_name(&key.expr))
        .flatten();

    let compared = if let Some(position) = integer(&key.expr) {
        match position {
            _ if position == value.position => GroupKey::Value,
            _ if position == rows.position => GroupKey::Rows,
            _ => return None,
        }
    } else if counts_rows(&key.expr) || (name.is_some() && name == rows.name) {
        GroupKey::Rows
    } else if name.is_some() && (name == value.name || name.as_deref() == Some(column)) {
        GroupKey::Value
    } else {
        return None;
    };

    let descending = key.options.asc == Some(false);

    Some(GroupOrder {
        key: compared,
        descending,
        nulls_first: key.options.nulls_first.unwrap_or(descending),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: &str = "_timestamp >= 1000 AND _timestamp < 9000";

    fn ascending(key: GroupKey) -> GroupOrder {
        GroupOrder {
            key,
            descending: false,
            nulls_first: false,
        }
    }

    fn descending(key: GroupKey) -> GroupOrder {
        GroupOrder {
            key,
            descending: true,
            nulls_first: true,
        }
    }

    #[test]
    fn reads_the_column_the_window_and_the_order() {
        let groups = grouped_counts(
            &format!(
                "SELECT service, COUNT(*) AS n FROM bench_logs WHERE {WINDOW} \
                 GROUP BY service ORDER BY service ASC"
            ),
            "bench_logs",
        )
        .expect("a grouped count");

        assert_eq!(groups.column, "service");
        assert!(groups.value_first);
        assert_eq!((groups.from, groups.to), (Some(1000), Some(9000)));
        assert_eq!(groups.order, vec![ascending(GroupKey::Value)]);
        assert_eq!(groups.limit, None);
        assert!(!groups.grouped.contains("ORDER BY"));
        assert!(groups.grouped.contains("GROUP BY service"));

        let top = grouped_counts(
            &format!(
                "SELECT service, COUNT(*) AS n FROM bench_logs WHERE {WINDOW} \
                 GROUP BY service ORDER BY n DESC, service ASC LIMIT 5"
            ),
            "bench_logs",
        )
        .expect("a grouped count");

        assert_eq!(
            top.order,
            vec![descending(GroupKey::Rows), ascending(GroupKey::Value)]
        );
        assert_eq!(top.limit, Some(5));
        assert!(!top.grouped.contains("LIMIT"));
        assert!(top.filters.is_empty());
    }

    #[test]
    fn reads_equality_conditions() {
        let groups = grouped_counts(
            &format!(
                "SELECT service, COUNT(*) AS n FROM bench_logs WHERE {WINDOW} \
                 AND level = 'ERROR' GROUP BY service ORDER BY n DESC, service ASC LIMIT 5"
            ),
            "bench_logs",
        )
        .expect("a grouped count");

        assert_eq!(
            groups.filters,
            vec![("level".to_owned(), "ERROR".to_owned())]
        );
        assert_eq!(groups.limit, Some(5));
        assert!(groups.grouped.contains("level = 'ERROR'"));
    }

    #[test]
    fn reads_positions_counts_and_swapped_columns() {
        let groups = grouped_counts(
            "SELECT COUNT(*), \"Service\" AS s FROM app GROUP BY 2 \
             ORDER BY 1 DESC NULLS LAST, s, count(*) LIMIT 3",
            "app",
        )
        .expect("a grouped count");

        assert_eq!(groups.column, "Service");
        assert!(!groups.value_first);
        assert_eq!((groups.from, groups.to), (None, None));

        assert_eq!(
            groups.order,
            vec![
                GroupOrder {
                    key: GroupKey::Rows,
                    descending: true,
                    nulls_first: false,
                },
                ascending(GroupKey::Value),
                ascending(GroupKey::Rows),
            ]
        );

        let unordered = grouped_counts("SELECT service, COUNT(1) FROM app GROUP BY 1", "app")
            .expect("a grouped count");

        assert!(unordered.order.is_empty());
    }

    #[test]
    fn rejects_other_statements() {
        let statement =
            |select: &str, rest: &str| format!("SELECT {select} FROM app WHERE {WINDOW}{rest}");

        for sql in [
            statement(
                "service, COUNT(*) AS n",
                " AND service ILIKE '%a%' GROUP BY service",
            ),
            statement(
                "service, COUNT(*) AS n",
                " AND duration >= 100 GROUP BY service",
            ),
            statement("service, COUNT(service) AS n", " GROUP BY service"),
            statement("NULLIF(service, '') AS v, COUNT(*)", " GROUP BY 1"),
            statement("service, level, COUNT(*)", " GROUP BY service, level"),
            statement("service, COUNT(*) AS n", " GROUP BY level"),
            statement("service, COUNT(*) AS n", " GROUP BY 2"),
            statement(
                "service, COUNT(*) AS n",
                " GROUP BY service HAVING COUNT(*) > 1",
            ),
            statement("service, COUNT(*) AS n", " GROUP BY service ORDER BY level"),
            statement("service, COUNT(*) AS n", " GROUP BY service ORDER BY 3"),
            statement(
                "service, COUNT(*) AS n",
                " GROUP BY service LIMIT 5 OFFSET 1",
            ),
            statement("service, COUNT(*) AS service", " GROUP BY 1"),
            statement("DISTINCT service, COUNT(*) AS n", " GROUP BY service"),
            "SELECT service, COUNT(*) FROM app WHERE timestamp >= 1000 GROUP BY service".to_owned(),
            "SELECT service, COUNT(*) FROM other GROUP BY service".to_owned(),
        ] {
            assert_eq!(grouped_counts(&sql, "app"), None, "{sql}");
        }
    }
}
