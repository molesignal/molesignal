// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! JSON 字段的键值索引：它在归档里的字段名与词的写法，以及 `json_text(列, 'key')` 上能用它回答的
//! 条件。剪枝与按索引计数共用这里的写法，与写入索引时的写法一致。

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Value,
};

use crate::column_name;

/// 键值索引在归档里的字段名的后缀，见 [`key_value_field`]。
pub const KEY_VALUE_SUFFIX: &str = "\0kv";

/// JSON 字段的键值索引在归档里的字段名。与数据流里的字段名不同：字段改过索引类型时，改之前写下的
/// 归档里没有它，答不出，不会把别的索引方式的词当成键值。
pub fn key_value_field(column: &str) -> String {
    format!("{column}{KEY_VALUE_SUFFIX}")
}

/// 键值索引里「`key` 的值是 `value`」这个词，`value` 是 `json_text` 给出的文本。
pub fn key_value_term(key: &str, value: &str) -> String {
    format!("={key}\0{value}")
}

/// 键值索引里「有 `key`，值不是 null」这个词。
pub fn key_term(key: &str) -> String {
    format!("?{key}")
}

/// `json_text(列, 'key')` 上的原子条件落在的列，与键值索引里满足它的行必有其一的词：`= '值'`、
/// `IN` 是这个键取这些值，`IS NOT NULL`（也包在 `NULLIF(…, '')` 里）是有这个键。一行在一个键上
/// 只有一个值，所以等值条件的词命中的文档数就是满足它的行数。
pub fn key_value_terms(expr: &Expr) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            let ((column, key), value) = json_member(left)
                .zip(literal(right))
                .or_else(|| json_member(right).zip(literal(left)))?;

            Some((column, vec![key_value_term(&key, value)]))
        }
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            let (column, key) = json_member(expr)?;

            let terms = list
                .iter()
                .map(|value| literal(value).map(|value| key_value_term(&key, value)))
                .collect::<Option<Vec<_>>>()?;

            (!terms.is_empty()).then_some((column, terms))
        }
        Expr::IsNotNull(expr) => {
            let (column, key) = json_member(expr).or_else(|| json_member(nullif_operand(expr)?))?;

            Some((column, vec![key_term(&key)]))
        }
        _ => None,
    }
}

/// `json_text(列, 'key')`：列（按 DataFusion 的解析，见 [`column_name`]）与键。
pub fn json_member(expr: &Expr) -> Option<(String, String)> {
    let Expr::Function(function) = expr else {
        return None;
    };

    if !function.name.to_string().eq_ignore_ascii_case("json_text") {
        return None;
    }

    let [column, key] = arguments(&function.args)?;

    Some((column_name(column)?, literal(key)?.to_owned()))
}

/// `NULLIF(x, '')` 的 `x`：它不为空时 `x` 也不为空。
fn nullif_operand(expr: &Expr) -> Option<&Expr> {
    let Expr::Function(function) = expr else {
        return None;
    };

    if !function.name.to_string().eq_ignore_ascii_case("nullif") {
        return None;
    }

    let [operand, _] = arguments(&function.args)?;

    Some(operand)
}

fn arguments(arguments: &FunctionArguments) -> Option<[&Expr; 2]> {
    let FunctionArguments::List(list) = arguments else {
        return None;
    };

    let [first, second] = list.args.as_slice() else {
        return None;
    };

    Some([unnamed(first)?, unnamed(second)?])
}

fn unnamed(argument: &FunctionArg) -> Option<&Expr> {
    match argument {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(expression)) => Some(expression),
        _ => None,
    }
}

fn literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(text) => Some(text),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::{dialect::GenericDialect, parser::Parser};

    use super::*;

    fn terms(condition: &str) -> Option<(String, Vec<String>)> {
        let expr = Parser::new(&GenericDialect)
            .try_with_sql(condition)
            .unwrap()
            .parse_expr()
            .unwrap();

        key_value_terms(&expr)
    }

    #[test]
    fn json_text_conditions_name_their_key_value_terms() {
        assert_eq!(
            terms("json_text(Context, 'plan') = 'pro'"),
            Some(("context".into(), vec!["=plan\0pro".into()]))
        );
        assert_eq!(
            terms("'pro' = json_text(\"Context\", 'plan')"),
            Some(("Context".into(), vec!["=plan\0pro".into()]))
        );
        assert_eq!(
            terms("json_text(context, 'plan') IN ('a', 'b')"),
            Some(("context".into(), vec!["=plan\0a".into(), "=plan\0b".into()]))
        );
        assert_eq!(
            terms("NULLIF(json_text(context, 'plan'), '') IS NOT NULL"),
            Some(("context".into(), vec!["?plan".into()]))
        );
        assert_eq!(terms("json_text(context, 'plan') != 'pro'"), None);
        assert_eq!(
            terms("json_text(context, 'plan') IN ('a', upper('b'))"),
            None
        );
        assert_eq!(terms("context = 'pro'"), None);
    }
}
