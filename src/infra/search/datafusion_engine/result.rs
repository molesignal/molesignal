// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow 查询结果到 HTTP 领域结果的转换。

use arrow::array::{Array, RecordBatch};

pub(super) fn batches_to_json(
    batches: &[RecordBatch],
) -> (Vec<String>, Vec<Vec<serde_json::Value>>) {
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

fn cell_to_json(array: &dyn Array, index: usize) -> serde_json::Value {
    use arrow::array::{
        BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };
    if array.is_null(index) {
        return serde_json::Value::Null;
    }
    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return serde_json::Value::Bool(values.value(index));
    }
    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return serde_json::Value::from(values.value(index));
    }
    if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        return serde_json::Value::from(values.value(index));
    }
    if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        return serde_json::Value::from(values.value(index).to_string());
    }
    if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return serde_json::Value::from(values.value(index));
    }
    serde_json::Value::String(format!("{:?}", array.slice(index, 1)))
}
