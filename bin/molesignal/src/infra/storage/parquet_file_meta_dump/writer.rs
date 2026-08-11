// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ParquetFileMeta dump 写入：把一组 [`ParquetFileMeta`] 序列化成 columnar parquet，PUT 到 object_store。
//!
//! change `parquet-file-meta-dump-columnar`：dump parquet schema 从单列 JSON 切到 15 列
//! 结构化 Arrow，让 reader 走 row-filter / pushdown，避免每次冷查都 JSON parse 整 partition。
//!
//! Schema（15 列，固定）：
//!   id (Utf8), org_id (Utf8), stream (Utf8), stream_type (Utf8), dataset_kind (Utf8), date (Utf8),
//!   object_key (Utf8), deleted (Boolean), rows (Int64), size_bytes (Int64),
//!   time_start_micros (Int64), time_end_micros (Int64),
//!   min_values_json (Utf8), max_values_json (Utf8), updated_at_micros (Int64)
//!
//! 对象命名：`{org}/_parquet_file_meta_dump/{stream_type}/{dataset_kind}/{stream}/{partition_key}.parquet`，
//! daily 时 `partition_key = YYYY-MM-DD`，hourly 时 `YYYY-MM-DD-HH`。
//! delete_by_time_range 的部分重写会产 `…/{partition_key}.r{n}.parquet`（n 起 1，单调）。

use std::sync::Arc;

use arrow::{
    array::{BooleanBuilder, Int64Builder, RecordBatch, StringBuilder},
    datatypes::{DataType, Field, Schema as ArrowSchema},
};
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use parquet::{arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties};

use crate::{
    domain::{
        storage::{ParquetFileMeta, PartitionLevel, PhysicalDatasetKind},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// Dump parquet 的 schema：15 列（change `parquet-file-meta-dump-columnar`）。
pub fn dump_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("org_id", DataType::Utf8, false),
        Field::new("stream", DataType::Utf8, false),
        Field::new("stream_type", DataType::Utf8, false),
        Field::new("dataset_kind", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("object_key", DataType::Utf8, false),
        Field::new("deleted", DataType::Boolean, false),
        Field::new("rows", DataType::Int64, false),
        Field::new("size_bytes", DataType::Int64, false),
        Field::new("time_start_micros", DataType::Int64, false),
        Field::new("time_end_micros", DataType::Int64, false),
        Field::new("min_values_json", DataType::Utf8, false),
        Field::new("max_values_json", DataType::Utf8, false),
        Field::new("updated_at_micros", DataType::Int64, false),
    ]))
}

/// Stream type → object key 路径段。
fn stream_type_dir(stream_type: StreamType) -> &'static str {
    match stream_type {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

/// `(org, stream_type, dataset_kind, stream, partition_level, partition_key)`
/// → dump object key。
///
/// `partition_key` 形态由 caller 保证：daily → `YYYY-MM-DD`，hourly → `YYYY-MM-DD-HH`。
/// 函数不做额外格式校验（partition_level 仅用于注释和未来扩展，不参与拼接）。
pub fn dump_object_key(
    org_id: &Id,
    stream: &str,
    stream_type: StreamType,
    dataset_kind: PhysicalDatasetKind,
    _partition_level: PartitionLevel,
    partition_key: &str,
) -> String {
    let st = stream_type_dir(stream_type);
    format!(
        "{}/_parquet_file_meta_dump/{}/{}/{}/{}.parquet",
        org_id.0,
        st,
        dataset_kind.as_str(),
        stream,
        partition_key
    )
}

/// `delete_by_time_range` 部分重写产生的 object key：`{base}/{partition_key}.r{n}.parquet`。
/// `seq` 起 1，由 caller 保证单调递增。
pub fn rewrite_object_key(
    org_id: &Id,
    stream: &str,
    stream_type: StreamType,
    dataset_kind: PhysicalDatasetKind,
    partition_key: &str,
    seq: u32,
) -> String {
    debug_assert!(seq >= 1, "rewrite_seq must start at 1");
    let st = stream_type_dir(stream_type);
    format!(
        "{}/_parquet_file_meta_dump/{}/{}/{}/{}.r{}.parquet",
        org_id.0,
        st,
        dataset_kind.as_str(),
        stream,
        partition_key,
        seq
    )
}

/// 计算 dump 行集合的聚合（min_ts / max_ts / size_bytes 在 caller 写完 parquet
/// 后才能补 `size_bytes`；这里只算前两项 + rows）。
pub struct DumpAggregate {
    pub rows: i64,
    pub min_ts_micros: i64,
    pub max_ts_micros: i64,
}

impl DumpAggregate {
    pub fn from_rows(rows: &[ParquetFileMeta]) -> Self {
        let rows_count = rows.len() as i64;
        if rows.is_empty() {
            return Self {
                rows: 0,
                min_ts_micros: 0,
                max_ts_micros: 0,
            };
        }
        let mut min_ts = i64::MAX;
        let mut max_ts = i64::MIN;
        for r in rows {
            if r.time_range.start.0 < min_ts {
                min_ts = r.time_range.start.0;
            }
            if r.time_range.end.0 > max_ts {
                max_ts = r.time_range.end.0;
            }
        }
        Self {
            rows: rows_count,
            min_ts_micros: min_ts,
            max_ts_micros: max_ts,
        }
    }
}

/// Stream type → string for the `stream_type` column (lowercase, matches PG).
fn stream_type_col(stream_type: StreamType) -> &'static str {
    match stream_type {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

/// 序列化 ParquetFileMeta 列表为 columnar dump.parquet bytes。
///
/// `date_for_partition` 由 caller 提供（daily → YYYY-MM-DD；hourly → YYYY-MM-DD，
/// hour 信息单独由 partition_key 编码）。`updated_at_micros` 由 caller 在 dump
/// 写出时刻统一注入（让所有 row 共享同一 timestamp，便于 rewrite 时区分）。
pub fn serialize_dump(
    rows: &[ParquetFileMeta],
    date_for_partition: &str,
    updated_at: TimestampMicros,
) -> Result<Bytes> {
    let schema = dump_schema();

    let n = rows.len();
    let mut id_b = StringBuilder::with_capacity(n, n * 24);
    let mut org_b = StringBuilder::with_capacity(n, n * 24);
    let mut stream_b = StringBuilder::with_capacity(n, n * 32);
    let mut stream_type_b = StringBuilder::with_capacity(n, n * 8);
    let mut dataset_kind_b = StringBuilder::with_capacity(n, n * 24);
    let mut date_b = StringBuilder::with_capacity(n, n * 10);
    let mut object_key_b = StringBuilder::with_capacity(n, n * 128);
    let mut deleted_b = BooleanBuilder::with_capacity(n);
    let mut rows_b = Int64Builder::with_capacity(n);
    let mut size_bytes_b = Int64Builder::with_capacity(n);
    let mut time_start_b = Int64Builder::with_capacity(n);
    let mut time_end_b = Int64Builder::with_capacity(n);
    let mut min_json_b = StringBuilder::with_capacity(n, n * 64);
    let mut max_json_b = StringBuilder::with_capacity(n, n * 64);
    let mut updated_at_b = Int64Builder::with_capacity(n);

    let updated_at_value = updated_at.0;
    for fm in rows {
        id_b.append_value(&fm.id.0);
        org_b.append_value(&fm.org_id.0);
        stream_b.append_value(&fm.stream);
        stream_type_b.append_value(stream_type_col(fm.stream_type));
        dataset_kind_b.append_value(fm.dataset_kind.as_str());
        date_b.append_value(date_for_partition);
        object_key_b.append_value(&fm.object_key);
        deleted_b.append_value(fm.deleted);
        rows_b.append_value(fm.rows as i64);
        size_bytes_b.append_value(fm.size_bytes as i64);
        time_start_b.append_value(fm.time_range.start.0);
        time_end_b.append_value(fm.time_range.end.0);
        let min_json = serde_json::to_string(&fm.min_values)
            .map_err(|e| Error::internal(format!("parquet_file_meta dump min_values json: {e}")))?;
        let max_json = serde_json::to_string(&fm.max_values)
            .map_err(|e| Error::internal(format!("parquet_file_meta dump max_values json: {e}")))?;
        min_json_b.append_value(&min_json);
        max_json_b.append_value(&max_json);
        updated_at_b.append_value(updated_at_value);
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(id_b.finish()),
            Arc::new(org_b.finish()),
            Arc::new(stream_b.finish()),
            Arc::new(stream_type_b.finish()),
            Arc::new(dataset_kind_b.finish()),
            Arc::new(date_b.finish()),
            Arc::new(object_key_b.finish()),
            Arc::new(deleted_b.finish()),
            Arc::new(rows_b.finish()),
            Arc::new(size_bytes_b.finish()),
            Arc::new(time_start_b.finish()),
            Arc::new(time_end_b.finish()),
            Arc::new(min_json_b.finish()),
            Arc::new(max_json_b.finish()),
            Arc::new(updated_at_b.finish()),
        ],
    )
    .map_err(|e| Error::internal(format!("parquet_file_meta dump record batch: {e}")))?;

    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    {
        let mut writer = ArrowWriter::try_new(&mut buf, schema, Some(props))
            .map_err(|e| Error::internal(format!("parquet_file_meta dump writer: {e}")))?;
        writer
            .write(&batch)
            .map_err(|e| Error::internal(format!("parquet_file_meta dump write: {e}")))?;
        writer
            .close()
            .map_err(|e| Error::internal(format!("parquet_file_meta dump close: {e}")))?;
    }
    Ok(Bytes::from(buf))
}

/// 上传 dump bytes 到指定 object_store。
pub async fn put_dump(store: &dyn ObjectStore, key: &str, bytes: Bytes) -> Result<()> {
    store
        .put(&Path::from(key), PutPayload::from_bytes(bytes))
        .await
        .map_err(|e| Error::internal(format!("parquet_file_meta dump put: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    fn sample_meta(seed: u64) -> ParquetFileMeta {
        let mut min = serde_json::Map::new();
        min.insert(
            "level".into(),
            serde_json::Value::String(format!("info-{seed}")),
        );
        let mut max = serde_json::Map::new();
        max.insert(
            "level".into(),
            serde_json::Value::String(format!("warn-{seed}")),
        );
        ParquetFileMeta {
            id: Id::from_string(format!("fm-{seed}")),
            org_id: Id::from_string("org-x"),
            stream: "app".into(),
            stream_type: StreamType::Logs,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: format!("k/{seed}.parquet"),
            time_range: TimeRange::new(
                TimestampMicros(seed as i64 * 1000),
                TimestampMicros(seed as i64 * 1000 + 500),
            ),
            rows: 10 + seed,
            size_bytes: 1024 + seed,
            min_values: min,
            max_values: max,
            deleted: false,
        }
    }

    #[test]
    fn serialize_empty_yields_valid_parquet() {
        let bytes =
            serialize_dump(&[], "2026-01-15", TimestampMicros(1_700_000_000_000)).expect("ok");
        assert!(
            !bytes.is_empty(),
            "even empty list must yield a parquet header"
        );
    }

    #[test]
    fn serialize_round_trip_keeps_all_rows() {
        let rows: Vec<ParquetFileMeta> = (0..5).map(sample_meta).collect();
        let bytes =
            serialize_dump(&rows, "2026-01-15", TimestampMicros(1_700_000_000_000)).expect("ok");
        let parsed =
            crate::infra::storage::parquet_file_meta_dump::reader::parse_dump_bytes_columnar(
                &bytes,
            )
            .expect("parse");
        assert_eq!(parsed.len(), rows.len());
        for (a, b) in rows.iter().zip(parsed.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.object_key, b.object_key);
            assert_eq!(a.rows, b.rows);
            assert_eq!(a.size_bytes, b.size_bytes);
            assert_eq!(a.time_range.start.0, b.time_range.start.0);
            assert_eq!(a.time_range.end.0, b.time_range.end.0);
            assert_eq!(a.min_values, b.min_values);
            assert_eq!(a.max_values, b.max_values);
        }
    }

    #[test]
    fn dump_object_key_daily_format_matches_spec() {
        let key = dump_object_key(
            &Id::from_string("orgA"),
            "log_app",
            StreamType::Logs,
            PhysicalDatasetKind::Raw,
            PartitionLevel::Daily,
            "2026-01-15",
        );
        assert_eq!(
            key,
            "orgA/_parquet_file_meta_dump/logs/raw/log_app/2026-01-15.parquet"
        );
    }

    #[test]
    fn dump_object_key_hourly_format() {
        let key = dump_object_key(
            &Id::from_string("orgA"),
            "stream",
            StreamType::Metrics,
            PhysicalDatasetKind::MetricRollup,
            PartitionLevel::Hourly,
            "2026-01-15-13",
        );
        assert_eq!(
            key,
            "orgA/_parquet_file_meta_dump/metrics/metric_rollup/stream/2026-01-15-13.parquet"
        );
    }

    #[test]
    fn rewrite_object_key_increments_seq() {
        assert_eq!(
            rewrite_object_key(
                &Id::from_string("orgA"),
                "stream",
                StreamType::Logs,
                PhysicalDatasetKind::Raw,
                "2026-01-15",
                1
            ),
            "orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.r1.parquet"
        );
        assert_eq!(
            rewrite_object_key(
                &Id::from_string("orgA"),
                "stream",
                StreamType::Logs,
                PhysicalDatasetKind::Raw,
                "2026-01-15",
                7
            ),
            "orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.r7.parquet"
        );
    }

    #[test]
    fn dump_aggregate_computes_min_max_ts() {
        let rows: Vec<ParquetFileMeta> = (1..=4).map(sample_meta).collect();
        let agg = DumpAggregate::from_rows(&rows);
        assert_eq!(agg.rows, 4);
        // sample_meta(seed) -> time_range = (seed * 1000, seed * 1000 + 500)
        assert_eq!(agg.min_ts_micros, 1_000);
        assert_eq!(agg.max_ts_micros, 4 * 1000 + 500);
    }
}
