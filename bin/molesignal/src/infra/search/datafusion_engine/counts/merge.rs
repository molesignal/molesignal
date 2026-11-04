// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把索引数出的计数与 DataFusion 对其余文件与缓冲的结果合成语句的回答。

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use arrow::{
    array::{Array, ArrayRef, Int64Array, RecordBatch, StringArray, UInt64Array},
    compute::cast,
    datatypes::{DataType, Field, Schema, SchemaRef},
};

use crate::{
    infra::search::tantivy_index::BarTally,
    shared::{Error, Result},
};

/// 把 Catalog 或索引已经数出的行数加进 `count(*)` 的结果；DataFusion 只数了其余文件与缓冲。
pub(super) fn add_rows(batches: &mut [RecordBatch], counted: u64) -> Result<()> {
    let Some(batch) = batches.iter_mut().find(|batch| batch.num_rows() > 0) else {
        return Ok(());
    };

    let counts = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .filter(|counts| batch.num_columns() == 1 && counts.len() == 1)
        .ok_or_else(|| Error::internal("a counted query must return one Int64 count"))?;

    let total = counts.value(0).saturating_add(to_i64(counted));

    *batch = RecordBatch::try_new(
        batch.schema(),
        vec![Arc::new(Int64Array::from(vec![total]))],
    )
    .map_err(|error| Error::internal(format!("count result: {error}")))?;

    Ok(())
}

/// 把索引数出的柱子与 DataFusion 返回的柱子按柱号相加，得到一个批次：柱号、行数，`split`
/// 时还有错误数，各是一列 Int64。
pub(super) fn add_bars(
    batches: &mut Vec<RecordBatch>,
    schema: SchemaRef,
    counted: &BTreeMap<i64, BarTally>,
    split: bool,
) -> Result<()> {
    let mut bars: BTreeMap<i64, (i64, i64)> = BTreeMap::new();

    for batch in batches.iter() {
        let index = counts(batch, 0)?;
        let total = counts(batch, 1)?;
        let errors = split.then(|| counts(batch, 2)).transpose()?;

        for row in 0..batch.num_rows() {
            let bar = bars.entry(index.value(row)).or_default();
            bar.0 = bar.0.saturating_add(total.value(row));
            bar.1 = bar
                .1
                .saturating_add(errors.map_or(0, |errors| errors.value(row)));
        }
    }

    for (index, tally) in counted {
        let bar = bars.entry(*index).or_default();
        bar.0 = bar.0.saturating_add(to_i64(tally.total));
        bar.1 = bar.1.saturating_add(to_i64(tally.errors));
    }

    let mut columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from_iter_values(bars.keys().copied())),
        Arc::new(Int64Array::from_iter_values(bars.values().map(|bar| bar.0))),
    ];

    if split {
        columns.push(Arc::new(Int64Array::from_iter_values(
            bars.values().map(|bar| bar.1),
        )));
    }

    let merged = RecordBatch::try_new(schema, columns)
        .map_err(|error| Error::internal(format!("histogram result: {error}")))?;

    *batches = vec![merged];
    Ok(())
}

/// 把索引数出的每个值的行数与 DataFusion 对其余文件数出的相加，按行数从多到少、同样多时按值
/// 升序取前 `limit` 个，得到一个批次：值一列、行数一列，类型照 `schema`。
pub(super) fn add_values(
    batches: &mut Vec<RecordBatch>,
    schema: SchemaRef,
    counted: &HashMap<String, u64>,
    limit: usize,
) -> Result<()> {
    let mut all = counted.clone();

    for batch in batches.iter() {
        each_value(batch, |value, count| {
            if let Some(value) = value {
                *all.entry(value.to_owned()).or_default() += count;
            }
        })?;
    }

    let mut ranked = all.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.truncate(limit);

    let values: ArrayRef = Arc::new(StringArray::from_iter_values(
        ranked.iter().map(|(value, _)| value.as_str()),
    ));

    let values = cast(&values, schema.field(0).data_type())
        .map_err(|error| Error::internal(format!("value result: {error}")))?;

    let rows = Arc::new(Int64Array::from_iter_values(
        ranked.iter().map(|(_, count)| to_i64(*count)),
    ));

    let merged = RecordBatch::try_new(schema, vec![values, rows])
        .map_err(|error| Error::internal(format!("value result: {error}")))?;

    *batches = vec![merged];
    Ok(())
}

/// 把索引数出的行数与每个值的行数，与 DataFusion 对其余文件与缓冲数出的每个值（没有值的行为
/// NULL）合起来，得到汇总的一行：行数、有值的行数、不同值的个数，列名照原语句的 `names`，类型
/// 照 DataFusion 给 `COUNT` 与 `approx_distinct` 的类型。不同值是数准的，不是估计。
pub(super) fn add_summary(
    batches: &mut Vec<RecordBatch>,
    names: &[String; 3],
    counted_rows: u64,
    counted: &HashMap<String, u64>,
) -> Result<()> {
    let mut rows = counted_rows;
    let mut values = counted.clone();

    for batch in batches.iter() {
        each_value(batch, |value, count| {
            rows = rows.saturating_add(count);

            if let Some(value) = value {
                *values.entry(value.to_owned()).or_default() += count;
            }
        })?;
    }

    let present = values.values().copied().fold(0_u64, u64::saturating_add);

    let schema = Arc::new(Schema::new(vec![
        Field::new(&names[0], DataType::Int64, false),
        Field::new(&names[1], DataType::Int64, false),
        Field::new(&names[2], DataType::UInt64, false),
    ]));

    let summary = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(vec![to_i64(rows)])),
            Arc::new(Int64Array::from(vec![to_i64(present)])),
            Arc::new(UInt64Array::from(vec![values.len() as u64])),
        ],
    )
    .map_err(|error| Error::internal(format!("summary result: {error}")))?;

    *batches = vec![summary];
    Ok(())
}

/// 按值计数的结果里的每一行：值（转成文本；NULL 为 `None`）与行数。
fn each_value(batch: &RecordBatch, mut visit: impl FnMut(Option<&str>, u64)) -> Result<()> {
    let values = cast(batch.column(0), &DataType::Utf8)
        .map_err(|error| Error::internal(format!("value result: {error}")))?;

    let values = values
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| Error::internal("values must come back as text"))?;

    let rows = counts(batch, 1)?;

    for row in 0..batch.num_rows() {
        let value = (!values.is_null(row)).then(|| values.value(row));
        visit(value, u64::try_from(rows.value(row)).unwrap_or_default());
    }

    Ok(())
}

/// 计数结果的第 `column` 列：没有空值的 Int64。
fn counts(batch: &RecordBatch, column: usize) -> Result<&Int64Array> {
    batch
        .columns()
        .get(column)
        .and_then(|array| array.as_any().downcast_ref::<Int64Array>())
        .filter(|array| array.null_count() == 0)
        .ok_or_else(|| Error::internal("a counted statement must return Int64 counts"))
}

fn to_i64(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(split: bool) -> SchemaRef {
        let mut fields = vec![
            Field::new("__histogram_bucket", DataType::Int64, true),
            Field::new("__histogram_total", DataType::Int64, false),
        ];

        if split {
            fields.push(Field::new("__histogram_errors", DataType::Int64, true));
        }

        Arc::new(Schema::new(fields))
    }

    fn rows(batches: &[RecordBatch]) -> Vec<Vec<i64>> {
        let mut rows = Vec::new();

        for batch in batches {
            for row in 0..batch.num_rows() {
                rows.push(
                    (0..batch.num_columns())
                        .map(|column| counts(batch, column).unwrap().value(row))
                        .collect(),
                );
            }
        }

        rows
    }

    #[test]
    fn index_bars_are_added_to_the_scanned_ones() {
        let scanned = RecordBatch::try_new(
            schema(true),
            vec![
                Arc::new(Int64Array::from(vec![3, 1])),
                Arc::new(Int64Array::from(vec![5, 2])),
                Arc::new(Int64Array::from(vec![1, 0])),
            ],
        )
        .unwrap();

        let mut batches = vec![scanned];

        let counted = BTreeMap::from([
            (
                1,
                BarTally {
                    total: 4,
                    errors: 4,
                },
            ),
            (
                2,
                BarTally {
                    total: 1,
                    errors: 0,
                },
            ),
        ]);

        add_bars(&mut batches, schema(true), &counted, true).unwrap();

        assert_eq!(
            rows(&batches),
            vec![vec![1, 6, 4], vec![2, 1, 0], vec![3, 5, 1]]
        );
    }

    #[test]
    fn index_bars_stand_alone_when_nothing_was_scanned() {
        let mut batches = Vec::new();
        let counted = BTreeMap::from([(
            7,
            BarTally {
                total: 2,
                errors: 1,
            },
        )]);

        add_bars(&mut batches, schema(false), &counted, false).unwrap();

        assert_eq!(rows(&batches), vec![vec![7, 2]]);
    }

    /// The rows without a value count among the rows only; a value seen both in the index and in
    /// the scan is one value.
    #[test]
    fn a_summary_adds_the_scanned_values_to_the_counted_ones() {
        let grouped = Arc::new(Schema::new(vec![
            Field::new("__summary_value", DataType::Utf8, true),
            Field::new("__summary_rows", DataType::Int64, false),
        ]));

        let scanned = RecordBatch::try_new(
            grouped,
            vec![
                Arc::new(StringArray::from(vec![Some("api"), None, Some("db")])),
                Arc::new(Int64Array::from(vec![2, 3, 1])),
            ],
        )
        .unwrap();

        let mut batches = vec![scanned];
        let names = ["__fs_total", "__fs_present", "__fs_distinct"].map(str::to_owned);
        let counted = HashMap::from([("api".to_owned(), 5), ("web".to_owned(), 1)]);

        add_summary(&mut batches, &names, 10, &counted).unwrap();

        let [summary] = batches.as_slice() else {
            panic!("one batch");
        };

        let schema = summary.schema();
        let read = |column: usize| -> u64 {
            let array = summary.column(column);

            match array.data_type() {
                DataType::Int64 => array
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap()
                    .value(0) as u64,
                _ => array
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap()
                    .value(0),
            }
        };

        assert_eq!(
            schema
                .fields()
                .iter()
                .map(|field| field.name().as_str())
                .collect::<Vec<_>>(),
            names.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!((read(0), read(1), read(2)), (16, 9, 3));
    }
}
