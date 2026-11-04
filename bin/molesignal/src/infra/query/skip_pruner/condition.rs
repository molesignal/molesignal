// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 条件树里落在一列上的原子条件，读成文件统计能判断的形式。

use std::collections::HashMap;

use sqlparser::ast::{
    BinaryOperator, DataType as SqlType, Expr, FunctionArg, FunctionArgExpr, FunctionArguments,
    UnaryOperator, Value as SqlValue,
};

use crate::{
    domain::stream::{FieldType, Schema, StreamIndexType, cidr_range},
    infra::query::parser::WhereTree,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Comparison {
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl Comparison {
    fn mirrored(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::LtEq => Self::GtEq,
            Self::Gt => Self::Lt,
            Self::GtEq => Self::LtEq,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Literal {
    fn from_expr(expression: &Expr) -> Option<Self> {
        match expression {
            Expr::Value(value) => match &value.value {
                SqlValue::SingleQuotedString(value) => Some(Self::String(value.clone())),
                SqlValue::Boolean(value) => Some(Self::Bool(*value)),
                SqlValue::Number(value, _) => value
                    .parse::<i64>()
                    .map(Self::Int)
                    .or_else(|_| value.parse::<f64>().map(Self::Float))
                    .ok(),
                _ => None,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Minus,
                expr,
            } => match Self::from_expr(expr)? {
                Self::Int(value) => value.checked_neg().map(Self::Int),
                Self::Float(value) => Some(Self::Float(-value)),
                Self::Bool(_) | Self::String(_) => None,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Plus,
                expr,
            }
            | Expr::Nested(expr) => Self::from_expr(expr),
            _ => None,
        }
    }

    pub(super) fn supports(&self, data_type: FieldType) -> bool {
        matches!(
            (self, data_type),
            (Self::Bool(_), FieldType::Bool)
                | (Self::Int(_), FieldType::Int64 | FieldType::Timestamp)
                | (Self::Int(_) | Self::Float(_), FieldType::Float64)
                | (Self::String(_), FieldType::Utf8 | FieldType::Json)
        )
    }
}

/// 一列的值要满足的条件。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Test {
    Compare(Comparison, Literal),
    /// `=`、`IN`：等于其中之一。
    AnyOf(Vec<Literal>),
    /// `!=`、`NOT IN`：不等于其中任何一个。
    NoneOf(Vec<Literal>),
    /// `LIKE 'abc%…'`：以这段字面量开头（区分大小写）。
    Prefix(String),
    /// `cidr_match(列, '网段')`：地址落在这个数值范围里，含两端。
    Network(u128, u128),
    /// 都满足：`BETWEEN` 的两端。
    All(Vec<Test>),
    /// `IS NULL`：没有值。
    Null,
    /// `IS NOT NULL`：有值。
    NotNull,
}

/// 落在一列上的条件。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Condition {
    pub(super) field: String,
    pub(super) data_type: FieldType,
    /// 字段建了 `ip` 索引：统计里有它的地址范围。
    pub(super) address: bool,
    pub(super) test: Test,
}

/// 能用统计判断的字段：没加密的（加密字段的统计是密文的）。
struct Fields<'a>(HashMap<&'a str, (FieldType, StreamIndexType)>);

/// 条件树里能用文件统计判断的原子条件，与它们的编号。
pub(super) fn extract(tree: &WhereTree, schema: &Schema) -> Vec<(usize, Condition)> {
    let fields = Fields(
        schema
            .fields
            .iter()
            .filter(|field| !field.encrypted)
            .map(|field| {
                (
                    field.name.as_str(),
                    (field.data_type, field.effective_index_type()),
                )
            })
            .collect(),
    );

    if fields.0.is_empty() {
        return Vec::new();
    }

    tree.atoms()
        .filter_map(|(atom, expression)| Some((atom, condition(expression, &fields)?)))
        .collect()
}

/// 一个原子条件：单个比较、`IN`、`BETWEEN`、前缀 `LIKE`、`cidr_match`、`IS NULL` 或
/// `IS NOT NULL`。
fn condition(expression: &Expr, fields: &Fields<'_>) -> Option<Condition> {
    match expression {
        Expr::Nested(inner) => condition(inner, fields),
        Expr::IsNull(expr) | Expr::IsNotNull(expr) => {
            let (field, data_type, address) = column(expr, fields)?;

            let test = match expression {
                Expr::IsNull(_) => Test::Null,
                _ => Test::NotNull,
            };

            Some(Condition {
                field,
                data_type,
                address,
                test,
            })
        }
        Expr::BinaryOp { left, op, right } => comparison(left, op, right, fields)
            .or_else(|| comparison_mirrored(right, op, left, fields)),
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let (field, data_type, address) = column(expr, fields)?;

            let literals = list
                .iter()
                .map(|item| Literal::from_expr(item).filter(|literal| literal.supports(data_type)))
                .collect::<Option<Vec<_>>>()?;

            let test = match negated {
                false => Test::AnyOf(literals),
                true => Test::NoneOf(literals),
            };

            Some(Condition {
                field,
                data_type,
                address,
                test,
            })
        }
        Expr::Between {
            expr,
            negated: false,
            low,
            high,
        } => {
            let (field, data_type, address) = column(expr, fields)?;
            let low = Literal::from_expr(low).filter(|literal| literal.supports(data_type))?;
            let high = Literal::from_expr(high).filter(|literal| literal.supports(data_type))?;

            Some(Condition {
                field,
                data_type,
                address,
                test: Test::All(vec![
                    Test::Compare(Comparison::GtEq, low),
                    Test::Compare(Comparison::LtEq, high),
                ]),
            })
        }
        Expr::Like {
            negated: false,
            any: false,
            expr,
            pattern,
            escape_char: None,
        } => {
            let (field, data_type, address) = column(expr, fields)?;

            let Some(Literal::String(pattern)) = Literal::from_expr(pattern) else {
                return None;
            };

            let prefix = like_prefix(&pattern)?;

            (data_type == FieldType::Utf8).then_some(Condition {
                field,
                data_type,
                address,
                test: Test::Prefix(prefix),
            })
        }
        Expr::Function(function)
            if function.name.to_string().eq_ignore_ascii_case("cidr_match") =>
        {
            let arguments = function_arguments(&function.args)?;

            let [value, network] = arguments.as_slice() else {
                return None;
            };

            let (field, data_type, address) = column(value, fields)?;

            let Some(Literal::String(network)) = Literal::from_expr(network) else {
                return None;
            };

            let (low, high) = cidr_range(&network)?;

            Some(Condition {
                field,
                data_type,
                address,
                test: Test::Network(low, high),
            })
        }
        _ => None,
    }
}

fn comparison(
    column_expr: &Expr,
    op: &BinaryOperator,
    value: &Expr,
    fields: &Fields<'_>,
) -> Option<Condition> {
    let (field, data_type, address) = column(column_expr, fields)?;
    let literal = Literal::from_expr(value).filter(|literal| literal.supports(data_type))?;

    let test = match op {
        BinaryOperator::Eq => Test::AnyOf(vec![literal]),
        BinaryOperator::NotEq => Test::NoneOf(vec![literal]),
        BinaryOperator::Lt => Test::Compare(Comparison::Lt, literal),
        BinaryOperator::LtEq => Test::Compare(Comparison::LtEq, literal),
        BinaryOperator::Gt => Test::Compare(Comparison::Gt, literal),
        BinaryOperator::GtEq => Test::Compare(Comparison::GtEq, literal),
        _ => return None,
    };

    Some(Condition {
        field,
        data_type,
        address,
        test,
    })
}

/// `字面量 op 列`：比较方向反过来。
fn comparison_mirrored(
    column_expr: &Expr,
    op: &BinaryOperator,
    value: &Expr,
    fields: &Fields<'_>,
) -> Option<Condition> {
    let mut condition = comparison(column_expr, op, value, fields)?;

    if let Test::Compare(comparison, _) = &mut condition.test {
        *comparison = comparison.mirrored();
    }

    Some(condition)
}

/// 无表限定的列，与它的类型、是否 `ip` 索引。只接受无表限定的列，避免多表查询中拿另一张表的
/// 谓词裁主表。日志筛选器对时间字段生成 `CAST(field AS BIGINT)`、对文本生成
/// `CAST(field AS VARCHAR)`：不改变取值次序的一层转换透明剥掉（见 [`keeps_order`]），别的转换
/// 不用来裁剪，统计记的是转换之前的值。
fn column(expression: &Expr, fields: &Fields<'_>) -> Option<(String, FieldType, bool)> {
    let (name, cast) = unqualified_column(expression, None)?;
    let (data_type, index_type) = *fields.0.get(name)?;

    if cast.is_some_and(|target| !keeps_order(target, data_type)) {
        return None;
    }

    Some((
        name.to_owned(),
        data_type,
        index_type == StreamIndexType::Ip,
    ))
}

/// 剥掉括号与至多一层 `CAST` 的无表限定列，与那层 `CAST` 转成的类型。
fn unqualified_column<'a>(
    expression: &'a Expr,
    cast: Option<&'a SqlType>,
) -> Option<(&'a str, Option<&'a SqlType>)> {
    match expression {
        Expr::Identifier(identifier) => Some((identifier.value.as_str(), cast)),
        Expr::Nested(expr) => unqualified_column(expr, cast),
        Expr::Cast {
            expr, data_type, ..
        } if cast.is_none() => unqualified_column(expr, Some(data_type)),
        _ => None,
    }
}

/// 转换后取值的次序与相等关系不变：整数与时间转成 64 位整数（时间按微秒，与统计一致），文本
/// 转成文本。浮点转成整数会取整，次序与统计对不上。
fn keeps_order(target: &SqlType, field: FieldType) -> bool {
    match target {
        SqlType::BigInt(_) | SqlType::Int64 => {
            matches!(field, FieldType::Int64 | FieldType::Timestamp)
        }
        SqlType::Varchar(_)
        | SqlType::CharacterVarying(_)
        | SqlType::CharVarying(_)
        | SqlType::Nvarchar(_)
        | SqlType::Text
        | SqlType::String(_) => matches!(field, FieldType::Utf8 | FieldType::Json),
        _ => false,
    }
}

/// `LIKE` 模式开头的字面量：第一个通配符之前的文字，`\` 转义的按原字符计。模式以通配符开头时
/// 没有前缀。
fn like_prefix(pattern: &str) -> Option<String> {
    let mut prefix = String::new();
    let mut chars = pattern.chars();

    while let Some(character) = chars.next() {
        match character {
            '%' | '_' => break,
            '\\' => prefix.push(chars.next().unwrap_or('\\')),
            other => prefix.push(other),
        }
    }

    (!prefix.is_empty()).then_some(prefix)
}

fn function_arguments(arguments: &FunctionArguments) -> Option<Vec<&Expr>> {
    let FunctionArguments::List(list) = arguments else {
        return None;
    };

    list.args
        .iter()
        .map(|argument| match argument {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(expression)) => Some(expression),
            _ => None,
        })
        .collect()
}
