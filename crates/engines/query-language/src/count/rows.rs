// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The conditions of a statement that reads the rows of one table one by one, when they are only
//! equalities and bounds of the row time: counts kept by field and minute then tell where its
//! newest or oldest rows lie.

use sqlparser::ast::{GroupByExpr, SetExpr};

use super::{
    conditions::{Conditions, TimeWindow, collect_conditions},
    plain, reads_only, single_query,
};

/// The whole WHERE of a statement: equality conditions and the row time window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowFilters {
    /// Equality conditions `(column, literal)`.
    pub filters: Vec<(String, String)>,
    /// The row time window `[from, to)` in microseconds; `None` on a side without a bound.
    pub from: Option<i64>,
    pub to: Option<i64>,
}

/// The conditions of `sql` when it reads `table` alone, ungrouped, and its WHERE, if any, is a
/// conjunction of `column = 'literal'` and bounds of the row time. What it selects, how it orders
/// and how many rows it takes are not looked at.
pub fn row_filters(sql: &str, table: &str) -> Option<RowFilters> {
    let query = single_query(sql)?;

    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };

    let ungrouped = matches!(
        &select.group_by,
        GroupByExpr::Expressions(exprs, modifiers) if exprs.is_empty() && modifiers.is_empty()
    );

    if !ungrouped || !plain(select) || !reads_only(&select.from, table) {
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

    let window = conditions.window.unwrap_or_default();

    Some(RowFilters {
        filters: conditions.filters,
        from: window.from,
        to: window.to,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(column: &str, value: &str) -> (String, String) {
        (column.to_owned(), value.to_owned())
    }

    #[test]
    fn reads_equalities_and_the_time_window_of_rows_in_any_order() {
        assert_eq!(
            row_filters(
                "SELECT _timestamp, message FROM logs WHERE level = 'ERROR' \
                 AND _timestamp >= 1000 AND _timestamp < 9000 \
                 ORDER BY _timestamp DESC, event_id ASC LIMIT 100",
                "logs"
            ),
            Some(RowFilters {
                filters: vec![pair("level", "ERROR")],
                from: Some(1000),
                to: Some(9000),
            })
        );

        assert_eq!(
            row_filters("SELECT * FROM logs ORDER BY _timestamp LIMIT 5", "logs"),
            Some(RowFilters::default())
        );
    }

    #[test]
    fn rejects_other_conditions_and_shapes() {
        for sql in [
            "SELECT * FROM logs WHERE message ILIKE '%x%' ORDER BY _timestamp DESC LIMIT 5",
            "SELECT * FROM logs WHERE duration >= 100 ORDER BY _timestamp DESC LIMIT 5",
            "SELECT * FROM logs WHERE level = 'a' OR level = 'b' LIMIT 5",
            "SELECT level, count(*) FROM logs GROUP BY level",
            "SELECT * FROM other ORDER BY _timestamp DESC LIMIT 5",
            "SELECT DISTINCT level FROM logs",
        ] {
            assert_eq!(row_filters(sql, "logs"), None, "{sql}");
        }
    }
}
