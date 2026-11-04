// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 计数语句的 WHERE：只认索引能回答的条件的合取。

use sqlparser::ast::{BinaryOperator, Expr, FunctionArg, FunctionArgExpr, Value};

use super::{column_name, integer, plain_call, timestamp_micros};
use crate::{key_value_field, key_value_terms};

/// WHERE 里的时间下界与上界；同一边出现多次时取最窄的。
#[derive(Default)]
pub(super) struct TimeWindow {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
}

/// WHERE 收集到的条件。等值与子串条件总是允许；`window` 与 `present` 为 `Some` 时才允许时间
/// 上下界与“有非空值”的条件。
#[derive(Default)]
pub(super) struct Conditions {
    /// `列 = '字面量'`：`(列名, 字面量)`。
    pub(super) filters: Vec<(String, String)>,
    /// `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub(super) contains: Vec<(String, String)>,
    pub(super) window: Option<TimeWindow>,
    /// `NULLIF(col, '') IS NOT NULL` 的列。
    pub(super) present: Option<Vec<String>>,
}

/// WHERE 必须整体是允许的条件的合取：`列 = '字面量'`、`列 ILIKE '%子串%'`、
/// `CAST(_timestamp AS BIGINT) >= n`、`< n`、`NULLIF(列, '') IS NOT NULL`，以及 JSON 字段上的
/// `json_text(列, 'key') = '字面量'` 与 `IS NOT NULL`（作为键值索引里的一个词，见
/// [`key_value_terms`]）。出现其它条件时返回 `None`。
pub(super) fn collect_conditions(expr: &Expr, conditions: &mut Conditions) -> Option<()> {
    if let Some((column, mut terms)) = key_value_terms(expr) {
        let term = terms.pop().filter(|_| terms.is_empty())?;
        conditions.filters.push((key_value_field(&column), term));
        return Some(());
    }

    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            collect_conditions(left, conditions)?;
            collect_conditions(right, conditions)
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            conditions.filters.push(equality(left, right)?);
            Some(())
        }
        Expr::ILike {
            negated: false,
            any: false,
            expr,
            pattern,
            escape_char: None,
        } => {
            conditions.contains.push(contained(expr, pattern)?);
            Some(())
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::GtEq,
            right,
        } if timestamp_micros(left) => {
            let window = conditions.window.as_mut()?;
            let from = integer(right)?;
            window.from = Some(window.from.map_or(from, |known| known.max(from)));
            Some(())
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Lt,
            right,
        } if timestamp_micros(left) => {
            let window = conditions.window.as_mut()?;
            let to = integer(right)?;
            window.to = Some(window.to.map_or(to, |known| known.min(to)));
            Some(())
        }
        Expr::IsNotNull(value) => {
            let column = nonempty(value)?;
            conditions.present.as_mut()?.push(column);
            Some(())
        }
        Expr::Nested(inner) => collect_conditions(inner, conditions),
        _ => None,
    }
}

/// `NULLIF(列, '')`：没有值与空串都是 NULL 的那一列。
pub(super) fn nonempty(expr: &Expr) -> Option<String> {
    let [
        FunctionArg::Unnamed(FunctionArgExpr::Expr(column)),
        FunctionArg::Unnamed(FunctionArgExpr::Expr(empty)),
    ] = plain_call(expr, "nullif")?
    else {
        return None;
    };

    let blank = matches!(
        empty,
        Expr::Value(value) if value.value == Value::SingleQuotedString(String::new())
    );

    blank.then(|| column_name(column)).flatten()
}

/// `列 = '字面量'`，两侧可以互换。
fn equality(left: &Expr, right: &Expr) -> Option<(String, String)> {
    column_literal(left, right).or_else(|| column_literal(right, left))
}

fn column_literal(column: &Expr, literal: &Expr) -> Option<(String, String)> {
    let Expr::Value(value) = literal else {
        return None;
    };

    let Value::SingleQuotedString(text) = &value.value else {
        return None;
    };

    Some((column_name(column)?, text.clone()))
}

/// `列 ILIKE '%子串%'`：值里任何位置出现这个子串（不分大小写）。子串非空、不含 LIKE 的通配符
/// 与转义符；`MATCH(列, '词')` 改写成的正是这样的条件。
fn contained(column: &Expr, pattern: &Expr) -> Option<(String, String)> {
    let Expr::Value(value) = pattern else {
        return None;
    };

    let Value::SingleQuotedString(text) = &value.value else {
        return None;
    };

    let needle = text.strip_prefix('%')?.strip_suffix('%')?;

    if needle.is_empty() || needle.contains(['%', '_', '\\']) {
        return None;
    }

    Some((column_name(column)?, needle.to_ascii_lowercase()))
}
