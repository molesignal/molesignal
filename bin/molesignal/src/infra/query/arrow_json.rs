// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询结果的 Arrow → JSON 转换：SQL 引擎与联邦查询共用同一份类型映射。
//!
//! | Arrow 类型 | JSON 值 |
//! |---|---|
//! | `Null` | `null` |
//! | `Boolean` | 布尔 |
//! | `Int8`…`Int64`、`UInt8`…`UInt64` | 数字 |
//! | `Float16` / `Float32` / `Float64` | 数字；`NaN` 与无穷大没有 JSON 表示，输出 `null` |
//! | `Decimal*` | 精度不超过 15 位输出数字，更高精度输出字符串，保证不丢位 |
//! | `Utf8` / `LargeUtf8` / `Utf8View` | 字符串 |
//! | `Binary` 系列 | 小写十六进制字符串 |
//! | `Timestamp`（任意单位与时区） | 距 Unix 纪元的微秒数（整数） |
//! | `Date32` / `Date64` | `YYYY-MM-DD` 字符串 |
//! | `Time32` / `Time64` | `HH:MM:SS[.fraction]` 字符串 |
//! | `List` 系列 / `FixedSizeList` | 数组 |
//! | `Struct` / `Map` | 对象 |
//! | `Dictionary` | 字典里的值 |
//!
//! 其余类型（`Duration`、`Interval`、`Union` 等）输出 Arrow 的文本格式。

use arrow::{
    array::{Array, ArrayRef, AsArray, RecordBatch, StructArray, downcast_dictionary_array},
    datatypes::{
        ArrowPrimitiveType, DataType, Date32Type, Date64Type, Float16Type, Float32Type,
        Float64Type, Int8Type, Int16Type, Int32Type, Int64Type, Time32MillisecondType,
        Time32SecondType, Time64MicrosecondType, Time64NanosecondType, TimeUnit,
        TimestampMicrosecondType, TimestampMillisecondType, TimestampNanosecondType,
        TimestampSecondType, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
    },
    util::display::{ArrayFormatter, FormatOptions},
};
use serde_json::{Number, Value};

#[cfg(test)]
mod tests;

/// 十进制 15 位以内的数在 `f64` 里无损往返。
const EXACT_DECIMAL_DIGITS: u8 = 15;

/// 把结果批转成列名与按列位置排列的行；列名取自第一个批次。
pub fn batches_to_json(batches: &[RecordBatch]) -> (Vec<String>, Vec<Vec<Value>>) {
    let mut columns = Vec::new();
    let mut rows = Vec::new();

    for batch in batches {
        if columns.is_empty() {
            columns = batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().clone())
                .collect();
        }

        for row_index in 0..batch.num_rows() {
            rows.push(
                batch
                    .columns()
                    .iter()
                    .map(|column| cell_to_json(column.as_ref(), row_index))
                    .collect(),
            );
        }
    }

    (columns, rows)
}

/// 把一个单元格转成 JSON 值，类型映射见模块文档。
pub fn cell_to_json(array: &dyn Array, index: usize) -> Value {
    if array.is_null(index) {
        return Value::Null;
    }

    match array.data_type() {
        DataType::Null => Value::Null,
        DataType::Boolean => Value::Bool(array.as_boolean().value(index)),
        DataType::Int8 => number::<Int8Type>(array, index),
        DataType::Int16 => number::<Int16Type>(array, index),
        DataType::Int32 => number::<Int32Type>(array, index),
        DataType::Int64 => number::<Int64Type>(array, index),
        DataType::UInt8 => number::<UInt8Type>(array, index),
        DataType::UInt16 => number::<UInt16Type>(array, index),
        DataType::UInt32 => number::<UInt32Type>(array, index),
        DataType::UInt64 => number::<UInt64Type>(array, index),
        DataType::Float16 => {
            single_precision(f32::from(array.as_primitive::<Float16Type>().value(index)))
        }
        DataType::Float32 => single_precision(array.as_primitive::<Float32Type>().value(index)),
        DataType::Float64 => Value::from(array.as_primitive::<Float64Type>().value(index)),
        DataType::Decimal32(precision, _)
        | DataType::Decimal64(precision, _)
        | DataType::Decimal128(precision, _)
        | DataType::Decimal256(precision, _) => decimal(array, index, *precision),
        DataType::Utf8 => Value::from(array.as_string::<i32>().value(index)),
        DataType::LargeUtf8 => Value::from(array.as_string::<i64>().value(index)),
        DataType::Utf8View => Value::from(array.as_string_view().value(index)),
        DataType::Timestamp(unit, _) => timestamp_micros(array, index, *unit),
        DataType::Date32 => displayed(array.as_primitive::<Date32Type>().value_as_date(index)),
        DataType::Date64 => displayed(array.as_primitive::<Date64Type>().value_as_date(index)),
        DataType::Time32(TimeUnit::Second) => displayed(
            array
                .as_primitive::<Time32SecondType>()
                .value_as_time(index),
        ),
        DataType::Time32(TimeUnit::Millisecond) => displayed(
            array
                .as_primitive::<Time32MillisecondType>()
                .value_as_time(index),
        ),
        DataType::Time64(TimeUnit::Microsecond) => displayed(
            array
                .as_primitive::<Time64MicrosecondType>()
                .value_as_time(index),
        ),
        DataType::Time64(TimeUnit::Nanosecond) => displayed(
            array
                .as_primitive::<Time64NanosecondType>()
                .value_as_time(index),
        ),
        DataType::List(_) => elements(&array.as_list::<i32>().value(index)),
        DataType::LargeList(_) => elements(&array.as_list::<i64>().value(index)),
        DataType::ListView(_) => elements(&array.as_list_view::<i32>().value(index)),
        DataType::LargeListView(_) => elements(&array.as_list_view::<i64>().value(index)),
        DataType::FixedSizeList(_, _) => elements(&array.as_fixed_size_list().value(index)),
        DataType::Struct(_) => object(array.as_struct(), index),
        DataType::Map(_, _) => entries(&array.as_map().value(index)),
        DataType::Dictionary(_, _) => dictionary_value(array, index),
        _ => text(array, index).map_or(Value::Null, Value::String),
    }
}

fn number<T>(array: &dyn Array, index: usize) -> Value
where
    T: ArrowPrimitiveType,
    T::Native: Into<Number>,
{
    Value::Number(array.as_primitive::<T>().value(index).into())
}

/// 按 `f32` 的最短十进制表示转换，`0.1_f32` 输出 `0.1` 而不是 `0.10000000149011612`。
fn single_precision(value: f32) -> Value {
    value
        .to_string()
        .parse::<f64>()
        .map_or(Value::Null, Value::from)
}

fn decimal(array: &dyn Array, index: usize, precision: u8) -> Value {
    let Some(text) = text(array, index) else {
        return Value::Null;
    };

    if precision <= EXACT_DECIMAL_DIGITS
        && let Ok(value) = text.parse::<f64>()
    {
        return Value::from(value);
    }

    Value::String(text)
}

/// 所有时间单位统一成微秒，与 `_timestamp` 列的约定一致。
fn timestamp_micros(array: &dyn Array, index: usize, unit: TimeUnit) -> Value {
    Value::from(match unit {
        TimeUnit::Second => array
            .as_primitive::<TimestampSecondType>()
            .value(index)
            .saturating_mul(1_000_000),
        TimeUnit::Millisecond => array
            .as_primitive::<TimestampMillisecondType>()
            .value(index)
            .saturating_mul(1_000),
        TimeUnit::Microsecond => array
            .as_primitive::<TimestampMicrosecondType>()
            .value(index),
        TimeUnit::Nanosecond => array
            .as_primitive::<TimestampNanosecondType>()
            .value(index)
            .div_euclid(1_000),
    })
}

fn displayed<T: std::fmt::Display>(value: Option<T>) -> Value {
    value.map_or(Value::Null, |value| Value::String(value.to_string()))
}

fn elements(values: &ArrayRef) -> Value {
    Value::Array(
        (0..values.len())
            .map(|index| cell_to_json(values.as_ref(), index))
            .collect(),
    )
}

fn object(array: &StructArray, index: usize) -> Value {
    Value::Object(
        array
            .fields()
            .iter()
            .zip(array.columns())
            .map(|(field, column)| (field.name().clone(), cell_to_json(column.as_ref(), index)))
            .collect(),
    )
}

/// Map 的一行是 `(key, value)` 条目；键不是字符串时取它的 JSON 文本。
fn entries(rows: &StructArray) -> Value {
    let (keys, values) = (rows.column(0), rows.column(1));

    Value::Object(
        (0..rows.len())
            .map(|row| {
                let key = match cell_to_json(keys.as_ref(), row) {
                    Value::String(key) => key,
                    other => other.to_string(),
                };

                (key, cell_to_json(values.as_ref(), row))
            })
            .collect(),
    )
}

fn dictionary_value(array: &dyn Array, index: usize) -> Value {
    downcast_dictionary_array!(
        array => match array.key(index) {
            Some(key) => cell_to_json(array.values().as_ref(), key),
            None => Value::Null,
        },
        _ => Value::Null
    )
}

/// Arrow 自带的文本格式；Arrow 没有该类型的格式化器时返回 `None`。
fn text(array: &dyn Array, index: usize) -> Option<String> {
    let formatter = ArrayFormatter::try_new(array, &FormatOptions::default()).ok()?;
    Some(formatter.value(index).to_string())
}
