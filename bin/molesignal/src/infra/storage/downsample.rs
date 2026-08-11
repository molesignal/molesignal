// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 指标降采样：把老数据按时间桶预聚合，降低分辨率以省存储（compactor 用）。
//!
//! 模型：按 `_timestamp` 的 `date_bin` 时间桶 + 全部**非数值**列分组，对**数值**列
//! （Int64/Float64）取 `avg` 并 **cast 回原类型**，其余列原样作分组键。产物 schema 与
//! 输入完全一致（经 [`align_batch_to_schema`] 兜底）→ 降采样文件与原始文件可同读、同一
//! 权威 schema 投影不报错。
//!
//! 仅对 metrics 流有意义（数值时序）；对文本/日志做时间桶分组会把每行打散成自己的桶。
//! 复用 DataFusion 内建 `date_bin` + `avg`（无需 UDF），单 `SessionContext` 内存执行。

use arrow::{
    array::{Array, BooleanArray, RecordBatch},
    compute::{concat_batches, filter_record_batch},
    datatypes::{DataType, TimeUnit},
};
use datafusion::prelude::SessionContext;

use super::arrow_schema::{TS_COL, align_batch_to_schema};
use crate::{
    domain::metrics::PROMETHEUS_EXEMPLAR_MARKER_FIELD,
    shared::{Error, Result},
};

/// 数值列 → SQL cast 目标类型（同时标识它是"度量"列）；非数值返回 None（作分组键）。
fn measure_cast_type(dt: &DataType) -> Option<&'static str> {
    match dt {
        DataType::Int64 => Some("BIGINT"),
        DataType::Float64 => Some("DOUBLE"),
        _ => None,
    }
}

/// 时间戳类型 → `arrow_cast` 可解析的类型串（`date_bin` 默认产 ns，需 cast 回原单位/时区，
/// 否则与权威 schema 的 `_timestamp` 类型不符）。非时间戳返回 None。
fn timestamp_arrow_cast_type(dt: &DataType) -> Option<String> {
    let DataType::Timestamp(unit, tz) = dt else {
        return None;
    };
    let unit_s = match unit {
        TimeUnit::Second => "Second",
        TimeUnit::Millisecond => "Millisecond",
        TimeUnit::Microsecond => "Microsecond",
        TimeUnit::Nanosecond => "Nanosecond",
    };
    Some(match tz {
        Some(z) => format!("Timestamp({unit_s}, Some(\"{z}\"))"),
        None => format!("Timestamp({unit_s}, None)"),
    })
}

/// 双引号标识符（转义内部引号），避免字段名含特殊字符时 SQL 解析失败。
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 把一批指标数据降采样到 `bucket_secs` 秒的时间桶。空批 / `bucket_secs==0` 原样返回。
///
/// 产物行数 = 不同 (时间桶, 分组键) 组合数；schema 与输入逐列一致。
pub async fn downsample_batch(batch: RecordBatch, bucket_secs: u32) -> Result<RecordBatch> {
    if bucket_secs == 0 || batch.num_rows() == 0 {
        return Ok(batch);
    }
    // Exemplar 是与普通 sample 共文件的独立事件，不能参与时间桶
    // avg/grouping。保留原行，只降采样非 exemplar 部分，否则历史
    // trace_id exemplar 会被合并或改写。
    if let Ok(marker_index) = batch.schema().index_of(PROMETHEUS_EXEMPLAR_MARKER_FIELD)
        && let Some(markers) = batch
            .column(marker_index)
            .as_any()
            .downcast_ref::<BooleanArray>()
    {
        let exemplar_mask = BooleanArray::from(
            (0..batch.num_rows())
                .map(|row| !markers.is_null(row) && markers.value(row))
                .collect::<Vec<_>>(),
        );
        if exemplar_mask.true_count() > 0 {
            let sample_mask = BooleanArray::from(
                (0..batch.num_rows())
                    .map(|row| markers.is_null(row) || !markers.value(row))
                    .collect::<Vec<_>>(),
            );
            let exemplars = filter_record_batch(&batch, &exemplar_mask)
                .map_err(|error| Error::internal(format!("filter metric exemplars: {error}")))?;
            let samples = filter_record_batch(&batch, &sample_mask)
                .map_err(|error| Error::internal(format!("filter metric samples: {error}")))?;
            if samples.num_rows() == 0 {
                return Ok(exemplars);
            }
            let reduced = downsample_samples(samples, bucket_secs).await?;
            return concat_batches(&batch.schema(), &[reduced, exemplars])
                .map_err(|error| Error::internal(format!("append metric exemplars: {error}")));
        }
    }
    downsample_samples(batch, bucket_secs).await
}

async fn downsample_samples(batch: RecordBatch, bucket_secs: u32) -> Result<RecordBatch> {
    let input_schema = batch.schema();

    // 找到 `_timestamp` 列（桶化基准）。缺失/非时间戳则无法降采样，原样返回。
    let Ok(ts_idx) = input_schema.index_of(TS_COL) else {
        return Ok(batch);
    };
    let Some(ts_cast) = timestamp_arrow_cast_type(input_schema.field(ts_idx).data_type()) else {
        return Ok(batch);
    };

    // date_bin 默认产 ns；arrow_cast 回 `_timestamp` 原类型，保持与权威 schema 一致。
    let bucket = format!(
        "arrow_cast(date_bin(INTERVAL '{bucket_secs} seconds', {ts}), '{ts_cast}')",
        ts = quote_ident(TS_COL)
    );

    // 按输入 schema 顺序构造 SELECT 列；GROUP BY = 桶 + 全部非度量非 ts 列。
    let mut select_items: Vec<String> = Vec::with_capacity(input_schema.fields().len());
    let mut group_items: Vec<String> = vec![bucket.clone()];
    for f in input_schema.fields() {
        let q = quote_ident(f.name());
        if f.name() == TS_COL {
            select_items.push(format!("{bucket} AS {q}"));
        } else if let Some(cast_ty) = measure_cast_type(f.data_type()) {
            // 度量：avg 再 cast 回原类型（保持 schema）。
            select_items.push(format!("CAST(avg({q}) AS {cast_ty}) AS {q}"));
        } else {
            // 维度：原样分组。
            select_items.push(q.clone());
            group_items.push(q);
        }
    }

    let sql = format!(
        "SELECT {select} FROM t GROUP BY {group}",
        select = select_items.join(", "),
        group = group_items.join(", "),
    );

    let ctx = SessionContext::new();
    ctx.register_batch("t", batch)
        .map_err(|e| Error::internal(format!("downsample register_batch: {e}")))?;
    let df = ctx
        .sql(&sql)
        .await
        .map_err(|e| Error::internal(format!("downsample plan ({sql}): {e}")))?;
    let out = df
        .collect()
        .await
        .map_err(|e| Error::internal(format!("downsample collect: {e}")))?;

    if out.is_empty() {
        // 不该发生（输入非空），保险起见返回一个空的输入 schema 批。
        return RecordBatch::try_new(input_schema.clone(), vec![])
            .map_err(|e| Error::internal(format!("downsample empty batch: {e}")));
    }
    let out_schema = out[0].schema();
    let merged = concat_batches(&out_schema, &out)
        .map_err(|e| Error::internal(format!("downsample concat: {e}")))?;
    // 把聚合产物对齐回输入 schema（列序 + nullability 归一；类型已由 cast 对齐）。
    align_batch_to_schema(&merged, &input_schema)
        .map_err(|e| Error::internal(format!("downsample align to schema: {e}")))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };

    use super::*;

    fn ts_field() -> Field {
        Field::new(
            TS_COL,
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        )
    }

    /// 三行落在同一小时桶 + 一行落在下一小时；含一个 label 维度、一个 Float64 度量、一个
    /// Int64 度量。降采样到 3600s 后应得 2 行（两个桶），度量取 avg 且类型不变。
    fn sample_batch() -> RecordBatch {
        let hour_us: i64 = 3600 * 1_000_000;
        let schema = Arc::new(Schema::new(vec![
            ts_field(),
            Field::new("host", DataType::Utf8, true),
            Field::new("cpu", DataType::Float64, true),
            Field::new("hits", DataType::Int64, true),
        ]));
        let ts = TimestampMicrosecondArray::from(vec![
            10,          // bucket 0
            20,          // bucket 0
            30,          // bucket 0
            hour_us + 5, // bucket 1
        ])
        .with_timezone("UTC");
        let host = StringArray::from(vec!["a", "a", "a", "a"]);
        let cpu = Float64Array::from(vec![1.0, 2.0, 3.0, 9.0]);
        let hits = Int64Array::from(vec![10, 20, 30, 7]);
        RecordBatch::try_new(
            schema,
            vec![Arc::new(ts), Arc::new(host), Arc::new(cpu), Arc::new(hits)],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn downsample_preserves_schema_and_averages_buckets() {
        let batch = sample_batch();
        let input_schema = batch.schema();
        let out = downsample_batch(batch, 3600).await.unwrap();

        // schema 逐列一致（名字 + 类型）——这是与原始文件同读的前提。
        assert_eq!(out.schema().fields().len(), input_schema.fields().len());
        for (a, b) in out
            .schema()
            .fields()
            .iter()
            .zip(input_schema.fields().iter())
        {
            assert_eq!(a.name(), b.name(), "column name preserved");
            assert_eq!(a.data_type(), b.data_type(), "column type preserved");
        }

        // 两个时间桶 → 两行。
        assert_eq!(out.num_rows(), 2, "rows collapsed into 2 hourly buckets");

        // 找到 bucket 0 那行（host=a，cpu=avg(1,2,3)=2.0，hits=avg(10,20,30)=20）。
        let cpu = out
            .column(out.schema().index_of("cpu").unwrap())
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let hits = out
            .column(out.schema().index_of("hits").unwrap())
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let mut cpus: Vec<f64> = (0..out.num_rows()).map(|i| cpu.value(i)).collect();
        cpus.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // bucket1 cpu=9.0, bucket0 cpu=2.0
        assert_eq!(cpus, vec![2.0, 9.0]);
        let mut hit_vals: Vec<i64> = (0..out.num_rows()).map(|i| hits.value(i)).collect();
        hit_vals.sort();
        // bucket0 avg(10,20,30)=20, bucket1 avg(7)=7
        assert_eq!(hit_vals, vec![7, 20]);
    }

    #[tokio::test]
    async fn downsample_noop_on_empty_or_zero_bucket() {
        let batch = sample_batch();
        let n = batch.num_rows();
        // bucket_secs=0 → 原样返回。
        let out = downsample_batch(batch, 0).await.unwrap();
        assert_eq!(out.num_rows(), n);
    }

    #[tokio::test]
    async fn downsample_groups_by_dimension() {
        // 同一桶内两个不同 host → 两行（按维度分组，不跨 host 聚合）。
        let schema = Arc::new(Schema::new(vec![
            ts_field(),
            Field::new("host", DataType::Utf8, true),
            Field::new("cpu", DataType::Float64, true),
        ]));
        let ts = TimestampMicrosecondArray::from(vec![10, 20, 30, 40]).with_timezone("UTC");
        let host = StringArray::from(vec!["a", "a", "b", "b"]);
        let cpu = Float64Array::from(vec![1.0, 3.0, 10.0, 20.0]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(host), Arc::new(cpu)])
            .unwrap();
        let out = downsample_batch(batch, 3600).await.unwrap();
        assert_eq!(out.num_rows(), 2, "one bucket × two hosts → 2 rows");
        let _ = TimeUnit::Microsecond;
    }

    #[tokio::test]
    async fn downsample_keeps_exemplar_rows_exact() {
        let schema = Arc::new(Schema::new(vec![
            ts_field(),
            Field::new("service", DataType::Utf8, true),
            Field::new("value", DataType::Float64, true),
            Field::new(PROMETHEUS_EXEMPLAR_MARKER_FIELD, DataType::Boolean, true),
            Field::new("__molesignal_exemplar_value", DataType::Float64, true),
            Field::new("__molesignal_exemplar_labels", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![10_i64, 20, 15]).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["api", "api", "api"])),
                Arc::new(Float64Array::from(vec![Some(1.0), Some(3.0), None])),
                Arc::new(BooleanArray::from(vec![None, None, Some(true)])),
                Arc::new(Float64Array::from(vec![None, None, Some(2.5)])),
                Arc::new(StringArray::from(vec![
                    None,
                    None,
                    Some(r#"{"trace_id":"trace-1"}"#),
                ])),
            ],
        )
        .unwrap();

        let out = downsample_batch(batch, 3_600).await.unwrap();
        assert_eq!(
            out.num_rows(),
            2,
            "two samples collapse; exemplar stays separate"
        );
        let markers = out
            .column_by_name(PROMETHEUS_EXEMPLAR_MARKER_FIELD)
            .unwrap()
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let exemplar_row = (0..out.num_rows())
            .find(|row| !markers.is_null(*row) && markers.value(*row))
            .expect("exemplar row");
        let timestamps = out
            .column_by_name(TS_COL)
            .unwrap()
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        assert_eq!(timestamps.value(exemplar_row), 15);
        let labels = out
            .column_by_name("__molesignal_exemplar_labels")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(labels.value(exemplar_row), r#"{"trace_id":"trace-1"}"#);
    }
}
