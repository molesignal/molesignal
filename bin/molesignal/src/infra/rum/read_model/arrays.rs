// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow scalar access shared by the RUM physical read models.

use arrow::{
    array::{
        Array, Float32Array, Float64Array, Int32Array, Int64Array, LargeStringArray, RecordBatch,
        StringArray, StringViewArray, TimestampMicrosecondArray, UInt32Array, UInt64Array,
    },
    datatypes::{DataType, TimeUnit},
};
use serde_json::Value;

pub(super) fn string_at<'a>(batch: &'a RecordBatch, name: &str, row: usize) -> Option<&'a str> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    if let Some(values) = column.as_any().downcast_ref::<StringArray>() {
        return Some(values.value(row));
    }
    if let Some(values) = column.as_any().downcast_ref::<LargeStringArray>() {
        return Some(values.value(row));
    }
    column
        .as_any()
        .downcast_ref::<StringViewArray>()
        .map(|values| values.value(row))
}

pub(super) fn string_any<'a>(
    batch: &'a RecordBatch,
    names: &[&str],
    row: usize,
) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| string_at(batch, name, row).filter(|value| !value.is_empty()))
}

pub(super) fn i64_at(batch: &RecordBatch, name: &str, row: usize) -> Option<i64> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    match column.data_type() {
        DataType::Int64 => Some(column.as_any().downcast_ref::<Int64Array>()?.value(row)),
        DataType::Int32 => Some(i64::from(
            column.as_any().downcast_ref::<Int32Array>()?.value(row),
        )),
        DataType::UInt64 => {
            i64::try_from(column.as_any().downcast_ref::<UInt64Array>()?.value(row)).ok()
        }
        DataType::UInt32 => Some(i64::from(
            column.as_any().downcast_ref::<UInt32Array>()?.value(row),
        )),
        DataType::Float64 => {
            Some(column.as_any().downcast_ref::<Float64Array>()?.value(row) as i64)
        }
        DataType::Float32 => {
            Some(column.as_any().downcast_ref::<Float32Array>()?.value(row) as i64)
        }
        DataType::Timestamp(TimeUnit::Microsecond, _) => Some(
            column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()?
                .value(row),
        ),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            string_at(batch, name, row)?.parse().ok()
        }
        _ => None,
    }
}

pub(super) fn f64_at(batch: &RecordBatch, name: &str, row: usize) -> Option<f64> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    match column.data_type() {
        DataType::Float64 => Some(column.as_any().downcast_ref::<Float64Array>()?.value(row)),
        DataType::Float32 => Some(f64::from(
            column.as_any().downcast_ref::<Float32Array>()?.value(row),
        )),
        DataType::Int64
        | DataType::Int32
        | DataType::UInt64
        | DataType::UInt32
        | DataType::Timestamp(TimeUnit::Microsecond, _) => {
            i64_at(batch, name, row).map(|value| value as f64)
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            string_at(batch, name, row)?.parse().ok()
        }
        _ => None,
    }
}

pub(super) fn json_at(batch: &RecordBatch, name: &str, row: usize) -> Option<Value> {
    let value = string_at(batch, name, row)?;
    serde_json::from_str(value)
        .ok()
        .or_else(|| Some(Value::String(value.to_string())))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Float64Array, Int64Array, RecordBatch, StringArray},
        datatypes::{Field, Schema},
    };

    use super::*;

    #[test]
    fn reads_common_scalar_representations() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("text", DataType::Utf8, true),
                Field::new("integer", DataType::Int64, true),
                Field::new("float", DataType::Float64, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("42")])),
                Arc::new(Int64Array::from(vec![Some(7)])),
                Arc::new(Float64Array::from(vec![Some(3.5)])),
            ],
        )
        .unwrap();
        assert_eq!(string_at(&batch, "text", 0), Some("42"));
        assert_eq!(i64_at(&batch, "integer", 0), Some(7));
        assert_eq!(f64_at(&batch, "float", 0), Some(3.5));
        assert_eq!(i64_at(&batch, "text", 0), Some(42));
    }
}
