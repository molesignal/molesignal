// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个文件的统计能否证明某个条件没有一行满足：空值数、min/max（zone map）、值集合与地址范围。
//! 缺了哪一样就不拿它判断；任何一样证明不可能，文件就不读。

use std::cmp::Ordering;

use serde_json::Value;

use super::condition::{Comparison, Condition, Literal, Test};
use crate::domain::{
    storage::QueryFile,
    stream::{FieldType, address_number, parse_address_key},
};

/// `file` 里可能有满足 `condition` 的行：每一样统计都不排除它。
pub(super) fn may_match(condition: &Condition, file: &QueryFile) -> bool {
    within_nulls(condition, file)
        && each_part(&condition.test, &|test| {
            within_bounds(test, condition, file)
        })
        && within_values(condition, file)
        && each_part(&condition.test, &|test| {
            within_addresses(test, condition, file)
        })
}

/// 按 `test` 的结构合起各部分的回答：`All` 都要成立。
fn each_part(test: &Test, part: &dyn Fn(&Test) -> bool) -> bool {
    match test {
        Test::All(tests) => tests.iter().all(|test| each_part(test, part)),
        _ => part(test),
    }
}

impl Comparison {
    /// 一个值与字面量的先后满足这个比较。
    fn holds(self, order: Ordering) -> bool {
        match self {
            Self::Lt => order == Ordering::Less,
            Self::LtEq => order != Ordering::Greater,
            Self::Gt => order == Ordering::Greater,
            Self::GtEq => order != Ordering::Less,
        }
    }
}

/// 空值数不排除条件：`IS NULL` 要有空的行；别的条件都要字段有值，字段在文件里全是空的就没有一行
/// 满足。
fn within_nulls(condition: &Condition, file: &QueryFile) -> bool {
    let Some(nulls) = file
        .null_counts
        .get(&condition.field)
        .and_then(Value::as_u64)
    else {
        return true;
    };

    match condition.test {
        Test::Null => nulls > 0,
        _ => nulls < file.rows,
    }
}

/// min/max 不排除 `test`。截短的字符串边界只会更宽，判断照样成立。
fn within_bounds(test: &Test, condition: &Condition, file: &QueryFile) -> bool {
    let (Some(minimum), Some(maximum)) = (
        file.min_values.get(&condition.field),
        file.max_values.get(&condition.field),
    ) else {
        return true;
    };

    let order = |bound: &Value, literal: &Literal| compare(bound, literal, condition.data_type);

    match test {
        Test::Compare(comparison, literal) => {
            let (Some(low), Some(high)) = (order(minimum, literal), order(maximum, literal)) else {
                return true;
            };

            match comparison {
                Comparison::Lt | Comparison::LtEq => comparison.holds(low),
                Comparison::Gt | Comparison::GtEq => comparison.holds(high),
            }
        }
        Test::AnyOf(literals) => literals.iter().any(|literal| {
            let (Some(low), Some(high)) = (order(minimum, literal), order(maximum, literal)) else {
                return true;
            };

            low != Ordering::Greater && high != Ordering::Less
        }),
        // 文件里只有一个值、它又被排除：没有一行满足。
        Test::NoneOf(literals) => {
            minimum != maximum
                || literals
                    .iter()
                    .all(|literal| order(minimum, literal) != Some(Ordering::Equal))
        }
        // 以 `prefix` 开头的字符串是区间 [prefix, prefix 进位)：与 [min, max] 相交。
        Test::Prefix(prefix) => {
            let (Some(minimum), Some(maximum)) = (minimum.as_str(), maximum.as_str()) else {
                return true;
            };

            maximum >= prefix.as_str()
                && (minimum <= prefix.as_str() || minimum.starts_with(prefix.as_str()))
        }
        Test::Network(..) | Test::All(_) | Test::Null | Test::NotNull => true,
    }
}

/// 值集合不排除条件：集合是段里出现过的全部值，有一个值满足整个条件才可能有这样的行。
fn within_values(condition: &Condition, file: &QueryFile) -> bool {
    // 值集合只记有值的行，说不出有没有空的。
    if condition.test == Test::Null {
        return true;
    }

    let Some(Value::Array(values)) = file.value_sets.get(&condition.field) else {
        return true;
    };

    values
        .iter()
        .any(|value| satisfies(&condition.test, value, condition.data_type))
}

/// `value` 可能满足 `test`；类型对不上、判断不了的算可能。
fn satisfies(test: &Test, value: &Value, data_type: FieldType) -> bool {
    let order = |literal: &Literal| compare(value, literal, data_type);

    match test {
        Test::Compare(comparison, literal) => {
            order(literal).is_none_or(|order| comparison.holds(order))
        }
        Test::AnyOf(literals) => literals
            .iter()
            .any(|literal| order(literal).is_none_or(|order| order == Ordering::Equal)),
        Test::NoneOf(literals) => literals
            .iter()
            .all(|literal| order(literal) != Some(Ordering::Equal)),
        Test::Prefix(prefix) => value
            .as_str()
            .is_none_or(|text| text.starts_with(prefix.as_str())),
        Test::Network(..) | Test::Null | Test::NotNull => true,
        Test::All(tests) => tests.iter().all(|test| satisfies(test, value, data_type)),
    }
}

/// `ip` 字段的地址范围不排除 `test`。范围只覆盖能解析成地址的值：等于一个地址字面量的值也能
/// 解析，落在范围里；不是地址的字面量判断不了。
fn within_addresses(test: &Test, condition: &Condition, file: &QueryFile) -> bool {
    if !condition.address {
        return true;
    }

    let Some(Value::Array(range)) = file.address_ranges.get(&condition.field) else {
        return true;
    };

    let key = |index: usize| {
        range
            .get(index)
            .and_then(Value::as_str)
            .and_then(parse_address_key)
    };

    let (Some(low), Some(high)) = (key(0), key(1)) else {
        return true;
    };

    match test {
        Test::Network(from, to) => *from <= high && low <= *to,
        Test::AnyOf(literals) => literals.iter().any(|literal| match literal {
            Literal::String(text) => {
                address_number(text).is_none_or(|address| (low..=high).contains(&address))
            }
            _ => true,
        }),
        _ => true,
    }
}

/// 统计里的一个值与字面量的先后；类型对不上时为 `None`。
fn compare(bound: &Value, literal: &Literal, data_type: FieldType) -> Option<Ordering> {
    match (data_type, literal) {
        (FieldType::Bool, Literal::Bool(value)) => bound.as_bool()?.partial_cmp(value),
        (FieldType::Int64 | FieldType::Timestamp, Literal::Int(value)) => {
            bound.as_i64()?.partial_cmp(value)
        }
        (FieldType::Float64, Literal::Int(value)) => bound.as_f64()?.partial_cmp(&(*value as f64)),
        (FieldType::Float64, Literal::Float(value)) if value.is_finite() => {
            bound.as_f64()?.partial_cmp(value)
        }
        (FieldType::Utf8 | FieldType::Json, Literal::String(value)) => {
            Some(bound.as_str()?.cmp(value.as_str()))
        }
        _ => None,
    }
}
