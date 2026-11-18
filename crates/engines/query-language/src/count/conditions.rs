// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 计数语句的 WHERE：只认索引能回答的条件的合取。

use std::ops::Bound;

use sqlparser::ast::{BinaryOperator, Expr, FunctionArg, FunctionArgExpr, UnaryOperator, Value};

use super::{TIMESTAMP_COLUMN, column_name, compared_timestamp, integer, plain_call};
use crate::{key_value_field, key_value_terms};

/// A number a condition compares a column with: an integer literal or a decimal one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Integer(i64),
    Decimal(f64),
}

/// The numbers of a column that `>=`, `>`, `<=` or `<` keeps: one end set by the literal, the
/// other open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    pub lower: Bound<Number>,
    pub upper: Bound<Number>,
}

impl NumberRange {
    /// The range of `column op number`; `None` for any other operator.
    fn of(op: &BinaryOperator, number: Number) -> Option<Self> {
        let (lower, upper) = match op {
            BinaryOperator::GtEq => (Bound::Included(number), Bound::Unbounded),
            BinaryOperator::Gt => (Bound::Excluded(number), Bound::Unbounded),
            BinaryOperator::LtEq => (Bound::Unbounded, Bound::Included(number)),
            BinaryOperator::Lt => (Bound::Unbounded, Bound::Excluded(number)),
            _ => return None,
        };

        Some(Self { lower, upper })
    }
}

/// WHERE 里的时间下界与上界；同一边出现多次时取最窄的。
#[derive(Default)]
pub(super) struct TimeWindow {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
}

impl TimeWindow {
    /// 加一个下界 `>= from`。
    fn narrow_from(&mut self, from: i64) {
        self.from = Some(self.from.map_or(from, |known| known.max(from)));
    }

    /// 加一个上界 `< to`。
    fn narrow_to(&mut self, to: i64) {
        self.to = Some(self.to.map_or(to, |known| known.min(to)));
    }
}

/// WHERE 收集到的条件。等值与子串条件总是允许；`window` 与 `present` 为 `Some` 时才允许时间
/// 上下界与“有非空值”的条件，`ranges` 为 `Some` 时才允许别的列与数字的比较。
#[derive(Default)]
pub(super) struct Conditions {
    /// `列 = '字面量'`：`(列名, 字面量)`。
    pub(super) filters: Vec<(String, String)>,
    /// `列 ILIKE '%子串%'`：`(列名, 按 ASCII 转小写的子串)`。
    pub(super) contains: Vec<(String, String)>,
    pub(super) window: Option<TimeWindow>,
    /// `NULLIF(col, '') IS NOT NULL` 的列。
    pub(super) present: Option<Vec<String>>,
    /// `列 >= 数`, `列 < 数`, …: the numbers each condition keeps of a column other than the row
    /// time. Only a caller that counts them asks for them.
    pub(super) ranges: Option<Vec<(String, NumberRange)>>,
}

/// WHERE 必须整体是允许的条件的合取：`列 = '字面量'`、`列 ILIKE '%子串%'`、行时间与整数微秒
/// 的 `>=`、`>`、`<`、`<=`（`_timestamp` 或 `CAST(_timestamp AS BIGINT)`）、
/// `NULLIF(列, '') IS NOT NULL`，以及 JSON 字段上的 `json_text(列, 'key') = '字面量'` 与
/// `IS NOT NULL`（作为键值索引里的一个词，见 [`key_value_terms`]）。出现其它条件时返回 `None`。
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
        Expr::BinaryOp { left, op, right } if compared_timestamp(left) => {
            let window = conditions.window.as_mut()?;
            let bound = integer(right)?;

            // 时间是整数微秒：`> n` 就是 `>= n + 1`，`<= n` 就是 `< n + 1`。
            match op {
                BinaryOperator::GtEq => window.narrow_from(bound),
                BinaryOperator::Gt => window.narrow_from(bound.checked_add(1)?),
                BinaryOperator::Lt => window.narrow_to(bound),
                BinaryOperator::LtEq => window.narrow_to(bound.checked_add(1)?),
                _ => return None,
            }

            Some(())
        }
        Expr::BinaryOp { left, op, right } => {
            let ranges = conditions.ranges.as_mut()?;
            ranges.push(compared_number(left, op, right)?);
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

/// `列 > 数`, `列 >= 数`, `列 < 数` or `列 <= 数`, either way round, on a column other than the row
/// time: the row time's bounds narrow the window, and only as written there.
fn compared_number(
    left: &Expr,
    op: &BinaryOperator,
    right: &Expr,
) -> Option<(String, NumberRange)> {
    let (column, op, number) = match (column_name(left), number(right)) {
        (Some(column), Some(number)) => (column, op.clone(), number),
        _ => {
            // `数 < 列` is `列 > 数`.
            let flipped = match op {
                BinaryOperator::Lt => BinaryOperator::Gt,
                BinaryOperator::LtEq => BinaryOperator::GtEq,
                BinaryOperator::Gt => BinaryOperator::Lt,
                BinaryOperator::GtEq => BinaryOperator::LtEq,
                _ => return None,
            };

            (column_name(right)?, flipped, number(left)?)
        }
    };

    if column == TIMESTAMP_COLUMN {
        return None;
    }

    Some((column, NumberRange::of(&op, number)?))
}

/// A number literal, integer or decimal, maybe negated or in brackets.
fn number(expr: &Expr) -> Option<Number> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::Number(digits, false) => match digits.parse::<i64>() {
                Ok(integer) => Some(Number::Integer(integer)),
                Err(_) => digits
                    .parse::<f64>()
                    .ok()
                    .filter(|decimal| decimal.is_finite())
                    .map(Number::Decimal),
            },
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => match number(expr)? {
            Number::Integer(integer) => integer.checked_neg().map(Number::Integer),
            Number::Decimal(decimal) => Some(Number::Decimal(-decimal)),
        },
        Expr::Nested(inner) => number(inner),
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
