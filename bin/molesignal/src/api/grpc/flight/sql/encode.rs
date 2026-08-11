// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `QueryResult`（JSON rows）→ Arrow `RecordBatch`（spec flight-sql）。
//!
//! 查询栈对外统一返回 `QueryResult { columns, rows: Vec<Vec<serde_json::Value>> }`，
//! 没有 RecordBatch 级别 API（design D3）；这里按列扫描推断 Arrow 类型：
//!
//! - 全 bool → `Boolean`；全 i64 → `Int64`；数值混合 → `Float64`
//! - 其余（字符串 / bool 与数值混合 / 嵌套对象数组转 JSON 文本）→ `Utf8`
//! - 全 null 列 → 可空 `Utf8`
//! - 列名 `_timestamp`（epoch micros）→ `Timestamp(Microsecond)`
//!
//! 类型保真度损失（如普通 timestamp 列退化 Int64）记录在 design Risks；
//! engine 级原生 batch 是独立 follow-up。

use std::sync::Arc;

use arrow::{
    array::{
        ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, RecordBatchOptions,
        StringArray, TimestampMicrosecondArray,
    },
    datatypes::{DataType, Field, Schema, TimeUnit},
    error::ArrowError,
};
use serde_json::Value;

use crate::domain::query::QueryResult;

/// 推断出的列类型；优先级见 [`infer_column`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColType {
    Bool,
    Int,
    Float,
    Text,
    TimestampMicros,
}

/// 单 batch 转换：行数与 `QueryResult` 一致（上游已受 query limit / matrix cap
/// 约束，与 HTTP JSON 路径同界）。空结果返回零行 batch（带 schema），不报错。
pub fn query_result_to_batch(out: &QueryResult) -> Result<RecordBatch, ArrowError> {
    if out.columns.is_empty() {
        return RecordBatch::try_new_with_options(
            Arc::new(Schema::empty()),
            Vec::new(),
            &RecordBatchOptions::new().with_row_count(Some(0)),
        );
    }

    let mut fields = Vec::with_capacity(out.columns.len());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(out.columns.len());
    for (idx, name) in out.columns.iter().enumerate() {
        let ty = infer_column(name, &out.rows, idx);
        let (data_type, array) = build_column(ty, &out.rows, idx);
        fields.push(Field::new(name, data_type, true));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
}

fn infer_column(name: &str, rows: &[Vec<Value>], idx: usize) -> ColType {
    let mut saw_bool = false;
    let mut saw_int = false;
    let mut saw_float = false;
    let mut saw_text = false;
    for row in rows {
        match row.get(idx).unwrap_or(&Value::Null) {
            Value::Null => {}
            Value::Bool(_) => saw_bool = true,
            Value::Number(n) => {
                if n.as_i64().is_some() {
                    saw_int = true;
                } else {
                    saw_float = true;
                }
            }
            Value::String(_) | Value::Array(_) | Value::Object(_) => saw_text = true,
        }
    }
    let ty = if saw_text || (saw_bool && (saw_int || saw_float)) {
        ColType::Text
    } else if saw_bool {
        ColType::Bool
    } else if saw_float {
        ColType::Float
    } else if saw_int {
        ColType::Int
    } else {
        // 全 null：可空 Utf8。
        ColType::Text
    };
    // `_timestamp` 是引擎注入的 epoch-micros 列；编码成时间类型方便 DB 客户端显示。
    if name == "_timestamp" && ty == ColType::Int {
        return ColType::TimestampMicros;
    }
    ty
}

fn build_column(ty: ColType, rows: &[Vec<Value>], idx: usize) -> (DataType, ArrayRef) {
    let cell = |row: &Vec<Value>| row.get(idx).cloned().unwrap_or(Value::Null);
    match ty {
        ColType::Bool => {
            let it = rows.iter().map(|r| cell(r).as_bool());
            (DataType::Boolean, Arc::new(BooleanArray::from_iter(it)))
        }
        ColType::Int => {
            let it = rows.iter().map(|r| cell(r).as_i64());
            (DataType::Int64, Arc::new(Int64Array::from_iter(it)))
        }
        ColType::TimestampMicros => {
            let it = rows.iter().map(|r| cell(r).as_i64());
            (
                DataType::Timestamp(TimeUnit::Microsecond, None),
                Arc::new(TimestampMicrosecondArray::from_iter(it)),
            )
        }
        ColType::Float => {
            let it = rows.iter().map(|r| cell(r).as_f64());
            (DataType::Float64, Arc::new(Float64Array::from_iter(it)))
        }
        ColType::Text => {
            let it = rows.iter().map(|r| match cell(r) {
                Value::Null => None,
                Value::String(s) => Some(s),
                // 数字 / bool / 嵌套值按 JSON 原文转字符串。
                other => Some(other.to_string()),
            });
            (DataType::Utf8, Arc::new(StringArray::from_iter(it)))
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::Array;
    use serde_json::json;

    use super::*;

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    #[test]
    fn int_float_bool_text_columns() {
        let out = result(
            &["i", "f", "b", "s"],
            vec![
                vec![json!(1), json!(1.5), json!(true), json!("a")],
                vec![json!(2), json!(2), json!(false), Value::Null],
            ],
        );
        let batch = query_result_to_batch(&out).unwrap();
        let schema = batch.schema();
        assert_eq!(schema.field(0).data_type(), &DataType::Int64);
        // 1.5 与 2 混合 → Float64（整数行 as_f64 仍可取）
        assert_eq!(schema.field(1).data_type(), &DataType::Float64);
        assert_eq!(schema.field(2).data_type(), &DataType::Boolean);
        assert_eq!(schema.field(3).data_type(), &DataType::Utf8);
        assert_eq!(batch.num_rows(), 2);
        let s = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(s.is_null(1));
    }

    #[test]
    fn mixed_number_and_string_degrades_to_utf8() {
        let out = result(
            &["x"],
            vec![vec![json!(42)], vec![json!("oops")], vec![Value::Null]],
        );
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), "42");
        assert_eq!(col.value(1), "oops");
        assert!(col.is_null(2));
    }

    #[test]
    fn timestamp_column_encoded_as_timestamp_micros() {
        let out = result(
            &["_timestamp", "msg"],
            vec![vec![json!(1_700_000_000_000_000_i64), json!("hi")]],
        );
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(
            batch.schema().field(0).data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, None)
        );
    }

    #[test]
    fn nested_values_serialized_as_json_text() {
        let out = result(
            &["payload"],
            vec![vec![json!({"k": [1, 2]})], vec![json!([true, null])]],
        );
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), r#"{"k":[1,2]}"#);
        assert_eq!(col.value(1), "[true,null]");
    }

    #[test]
    fn all_null_column_is_nullable_utf8() {
        let out = result(&["x"], vec![vec![Value::Null], vec![Value::Null]]);
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
        assert_eq!(batch.column(0).null_count(), 2);
    }

    #[test]
    fn empty_result_yields_zero_row_batch() {
        let out = result(&[], Vec::new());
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 0);

        // 有列无行也要能编码
        let out = result(&["a"], Vec::new());
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 1);
    }

    #[test]
    fn u64_overflow_degrades_to_float() {
        let out = result(&["x"], vec![vec![json!(u64::MAX)], vec![json!(1)]]);
        let batch = query_result_to_batch(&out).unwrap();
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Float64);
    }
}
