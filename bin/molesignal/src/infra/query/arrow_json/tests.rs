// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, BooleanArray, Date32Array, Date64Array, Decimal128Array,
        DictionaryArray, DurationSecondArray, Float32Array, Float64Array, Int8Array, Int16Array,
        Int32Array, Int32Builder, Int64Array, LargeStringArray, ListArray, MapBuilder, RecordBatch,
        StringArray, StringBuilder, StringViewArray, StructArray, Time32SecondArray,
        Time64MicrosecondArray, TimestampMicrosecondArray, TimestampMillisecondArray,
        TimestampNanosecondArray, TimestampSecondArray, UInt8Array, UInt16Array, UInt32Array,
        UInt64Array,
    },
    compute::cast,
    datatypes::{DataType, Field, Int8Type, Int32Type, Schema},
};
use datafusion::prelude::SessionContext;
use serde_json::{Value, json};

use super::{batches_to_json, cell_to_json};

fn cells(array: &dyn Array) -> Vec<Value> {
    (0..array.len())
        .map(|index| cell_to_json(array, index))
        .collect()
}

#[test]
fn integers_of_every_width_become_numbers() {
    assert_eq!(
        cells(&Int8Array::from(vec![Some(-8), None])),
        [json!(-8), Value::Null]
    );
    assert_eq!(cells(&Int16Array::from(vec![300])), [json!(300)]);
    assert_eq!(
        cells(&Int32Array::from(vec![Some(5), None, Some(i32::MIN)])),
        [json!(5), Value::Null, json!(i32::MIN)]
    );
    assert_eq!(cells(&Int64Array::from(vec![i64::MAX])), [json!(i64::MAX)]);
    assert_eq!(cells(&UInt8Array::from(vec![255])), [json!(255)]);
    assert_eq!(cells(&UInt16Array::from(vec![65_535])), [json!(65_535)]);
    assert_eq!(cells(&UInt32Array::from(vec![u32::MAX])), [json!(u32::MAX)]);
    assert_eq!(cells(&UInt64Array::from(vec![u64::MAX])), [json!(u64::MAX)]);
}

#[test]
fn floats_become_numbers_and_non_finite_values_become_null() {
    assert_eq!(
        cells(&Float64Array::from(vec![1.5, f64::NAN, f64::INFINITY])),
        [json!(1.5), Value::Null, Value::Null]
    );
    // f32 keeps its shortest decimal form.
    assert_eq!(
        cells(&Float32Array::from(vec![0.1_f32, f32::NEG_INFINITY])),
        [json!(0.1), Value::Null]
    );
    let half = cast(&Float32Array::from(vec![1.5_f32]), &DataType::Float16).unwrap();
    assert_eq!(cells(half.as_ref()), [json!(1.5)]);
}

#[test]
fn strings_and_booleans_keep_their_values() {
    assert_eq!(
        cells(&StringArray::from(vec![Some("a"), None])),
        [json!("a"), Value::Null]
    );
    assert_eq!(cells(&LargeStringArray::from(vec!["b"])), [json!("b")]);
    assert_eq!(cells(&StringViewArray::from(vec!["c"])), [json!("c")]);
    assert_eq!(
        cells(&BooleanArray::from(vec![true, false])),
        [json!(true), json!(false)]
    );
}

#[test]
fn timestamps_of_every_unit_are_microseconds() {
    let micros = 1_700_000_000_123_456_i64;
    assert_eq!(
        cells(&TimestampSecondArray::from(vec![1_700_000_000])),
        [json!(1_700_000_000_000_000_i64)]
    );
    assert_eq!(
        cells(&TimestampMillisecondArray::from(vec![1_700_000_000_123])),
        [json!(1_700_000_000_123_000_i64)]
    );
    assert_eq!(
        cells(&TimestampMicrosecondArray::from(vec![micros]).with_timezone("UTC")),
        [json!(micros)]
    );
    assert_eq!(
        cells(&TimestampNanosecondArray::from(vec![
            1_700_000_000_123_456_789
        ])),
        [json!(micros)]
    );
    // 早于纪元的纳秒向下取整。
    assert_eq!(
        cells(&TimestampNanosecondArray::from(vec![-1])),
        [json!(-1)]
    );
}

#[test]
fn dates_and_times_are_iso_strings() {
    assert_eq!(
        cells(&Date32Array::from(vec![19_000])),
        [json!("2022-01-08")]
    );
    assert_eq!(
        cells(&Date64Array::from(vec![19_000 * 86_400_000])),
        [json!("2022-01-08")]
    );
    assert_eq!(
        cells(&Time32SecondArray::from(vec![3_661])),
        [json!("01:01:01")]
    );
    assert_eq!(
        cells(&Time64MicrosecondArray::from(vec![3_661_500_000])),
        [json!("01:01:01.500")]
    );
}

#[test]
fn decimals_are_numbers_unless_digits_would_be_lost() {
    let narrow = Decimal128Array::from(vec![1234, -5])
        .with_precision_and_scale(10, 2)
        .unwrap();
    assert_eq!(cells(&narrow), [json!(12.34), json!(-0.05)]);
    let wide = Decimal128Array::from(vec![123_456_789_012_345_678_901_234_567_890_i128])
        .with_precision_and_scale(38, 10)
        .unwrap();
    assert_eq!(cells(&wide), [json!("12345678901234567890.1234567890")]);
}

#[test]
fn binary_is_lowercase_hex() {
    assert_eq!(
        cells(&BinaryArray::from(vec![&[0x0a_u8, 0xff][..]])),
        [json!("0aff")]
    );
}

#[test]
fn lists_structs_and_maps_nest() {
    let list = ListArray::from_iter_primitive::<Int32Type, _, _>(vec![
        Some(vec![Some(1), None]),
        None,
        Some(vec![]),
    ]);
    assert_eq!(cells(&list), [json!([1, null]), Value::Null, json!([])]);

    let structs = StructArray::from(vec![
        (
            Arc::new(Field::new("id", DataType::Int32, false)),
            Arc::new(Int32Array::from(vec![7])) as ArrayRef,
        ),
        (
            Arc::new(Field::new("name", DataType::Utf8, true)),
            Arc::new(StringArray::from(vec![None::<&str>])) as ArrayRef,
        ),
    ]);
    assert_eq!(cells(&structs), [json!({"id": 7, "name": null})]);

    let mut builder = MapBuilder::new(None, StringBuilder::new(), Int32Builder::new());
    builder.keys().append_value("a");
    builder.values().append_value(1);
    builder.append(true).unwrap();
    assert_eq!(cells(&builder.finish()), [json!({"a": 1})]);
}

#[test]
fn dictionary_cells_resolve_to_their_values() {
    let dictionary: DictionaryArray<Int8Type> = vec![Some("a"), None, Some("b"), Some("a")]
        .into_iter()
        .collect();
    assert_eq!(
        cells(&dictionary),
        [json!("a"), Value::Null, json!("b"), json!("a")]
    );
}

#[test]
fn other_types_use_the_arrow_text_format() {
    assert_eq!(
        cells(&DurationSecondArray::from(vec![90])),
        [json!("PT90S")]
    );
}

#[test]
fn rows_follow_column_positions_across_batches() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("n", DataType::Int32, false),
        Field::new("n", DataType::Utf8, true),
    ]));
    let batch = |numbers: Vec<i32>, labels: Vec<Option<&str>>| {
        RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(numbers)) as ArrayRef,
                Arc::new(StringArray::from(labels)) as ArrayRef,
            ],
        )
        .unwrap()
    };
    let (columns, rows) =
        batches_to_json(&[batch(vec![1], vec![Some("a")]), batch(vec![2], vec![None])]);
    assert_eq!(columns, ["n", "n"]);
    assert_eq!(rows, [[json!(1), json!("a")], [json!(2), Value::Null]]);

    let (columns, rows) = batches_to_json(&[]);
    assert!(columns.is_empty() && rows.is_empty());
}

#[tokio::test]
async fn sql_results_of_narrow_types_render_as_json_values() {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "message",
        DataType::Utf8,
        true,
    )]));
    let batch = RecordBatch::try_new(
        schema,
        vec![Arc::new(StringArray::from(vec!["hi", "hello", "hey"])) as ArrayRef],
    )
    .unwrap();
    let ctx = SessionContext::new();
    ctx.register_batch("logs", batch).unwrap();

    let run = |sql: &'static str| {
        let ctx = ctx.clone();
        async move {
            let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
            batches_to_json(&batches)
        }
    };

    let (columns, rows) = run("SELECT max(length(message)) AS longest FROM logs").await;
    assert_eq!(columns, ["longest"]);
    assert_eq!(rows, [[json!(5)]]);

    let (_, rows) = run(
        "SELECT CAST(1 AS TINYINT), CAST(2 AS SMALLINT), CAST(3 AS INT), \
         CAST(4 AS INT UNSIGNED), CAST(0.5 AS FLOAT), CAST('2022-01-08' AS DATE)",
    )
    .await;
    assert_eq!(
        rows,
        [[
            json!(1),
            json!(2),
            json!(3),
            json!(4),
            json!(0.5),
            json!("2022-01-08")
        ]]
    );
}
