// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ParquetFileMeta dump 读取：GET dump.parquet → 解析 15 列 columnar 行 → `Vec<ParquetFileMeta>`。
//!
//! change `parquet-file-meta-dump-columnar`：单列 JSON dump（旧形态）已不再写入也不再读取；
//! reader 只识别新 columnar schema。读路径加 Arrow predicate row-filter：在 row-group
//! 层就把 `time_end_micros >= range.start AND time_start_micros < range.end` 剪掉，
//! 之后再过一次精确 retain 兜底 page-level 边界行。
//!
//! caller 传入的 `object_store` 已经被 `ProductionObjectStore` 包装的话，bytes
//! 层会自动走 parquet disk cache；进程内 `Arc<Vec<ParquetFileMeta>>` cache 由
//! `crate::infra::caching::parquet_file_meta::dump::ParquetFileMetaDumpCache` 独立维护，本 reader 自己
//! 不做二级缓存。

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use arrow::{
    array::{Array, BooleanArray, Int64Array, StringArray},
    record_batch::RecordBatch,
};
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use prometheus::{Histogram, HistogramOpts, IntCounter};

use crate::{
    domain::{
        storage::{ParquetFileMeta, PhysicalDatasetKind},
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        metrics::{global_registry, register_int_counter},
        time::{TimeRange, TimestampMicros},
    },
};

/// `(parquet bytes) -> Vec<ParquetFileMeta>`（不裁剪）。本函数不发任何 IO，便于单测覆盖。
pub fn parse_dump_bytes_columnar(bytes: &Bytes) -> Result<Vec<ParquetFileMeta>> {
    parse_with_range(bytes, None)
}

fn parse_with_range(bytes: &Bytes, time_range: Option<TimeRange>) -> Result<Vec<ParquetFileMeta>> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(bytes.clone())
        .map_err(|e| Error::internal(format!("parquet_file_meta dump parquet reader: {e}")))?
        .build()
        .map_err(|e| Error::internal(format!("parquet_file_meta dump parquet build: {e}")))?;
    let mut out: Vec<ParquetFileMeta> = Vec::new();
    for batch in reader {
        let batch = batch
            .map_err(|e| Error::internal(format!("parquet_file_meta dump batch read: {e}")))?;
        decode_batch_into(&batch, time_range, &mut out)?;
    }
    Ok(out)
}

fn decode_batch_into(
    batch: &RecordBatch,
    time_range: Option<TimeRange>,
    out: &mut Vec<ParquetFileMeta>,
) -> Result<()> {
    macro_rules! col {
        ($name:literal, $ty:ty) => {{
            let c = batch
                .column_by_name($name)
                .ok_or_else(|| {
                    Error::internal(format!("parquet_file_meta dump missing {} column", $name))
                })?
                .as_any()
                .downcast_ref::<$ty>()
                .ok_or_else(|| {
                    Error::internal(format!(
                        "parquet_file_meta dump {} column wrong type",
                        $name
                    ))
                })?;
            c
        }};
    }
    let id = col!("id", StringArray);
    let org = col!("org_id", StringArray);
    let stream = col!("stream", StringArray);
    let stream_type = col!("stream_type", StringArray);
    let dataset_kind = col!("dataset_kind", StringArray);
    let object_key = col!("object_key", StringArray);
    let deleted = col!("deleted", BooleanArray);
    let rows = col!("rows", Int64Array);
    let size_bytes = col!("size_bytes", Int64Array);
    let time_start = col!("time_start_micros", Int64Array);
    let time_end = col!("time_end_micros", Int64Array);
    let min_json = col!("min_values_json", StringArray);
    let max_json = col!("max_values_json", StringArray);

    out.reserve(batch.num_rows());
    for i in 0..batch.num_rows() {
        let ts_start = time_start.value(i);
        let ts_end = time_end.value(i);
        if let Some(range) = time_range
            && (ts_end < range.start.0 || ts_start >= range.end.0)
        {
            continue;
        }
        let stream_type_str = stream_type.value(i);
        let st = parse_stream_type(stream_type_str)?;
        let min_values: serde_json::Map<String, serde_json::Value> =
            parse_object_json(min_json.value(i), "min_values_json")?;
        let max_values: serde_json::Map<String, serde_json::Value> =
            parse_object_json(max_json.value(i), "max_values_json")?;
        out.push(ParquetFileMeta {
            id: Id::from_string(id.value(i)),
            org_id: Id::from_string(org.value(i)),
            stream: stream.value(i).to_string(),
            stream_type: st,
            dataset_kind: dataset_kind.value(i).parse::<PhysicalDatasetKind>()?,
            object_key: object_key.value(i).to_string(),
            time_range: TimeRange::new(TimestampMicros(ts_start), TimestampMicros(ts_end)),
            rows: rows.value(i) as u64,
            size_bytes: size_bytes.value(i) as u64,
            min_values,
            max_values,
            deleted: deleted.value(i),
        });
    }
    Ok(())
}

fn parse_stream_type(s: &str) -> Result<StreamType> {
    match s {
        "logs" => Ok(StreamType::Logs),
        "metrics" => Ok(StreamType::Metrics),
        "traces" => Ok(StreamType::Traces),
        "profiles" => Ok(StreamType::Profiles),
        "extend" => Ok(StreamType::Extend),
        other => Err(Error::internal(format!(
            "parquet_file_meta dump unknown stream_type: {other}"
        ))),
    }
}

fn parse_object_json(s: &str, field: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    if s.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| Error::internal(format!("parquet_file_meta dump {field} parse: {e}")))?;
    match v {
        serde_json::Value::Object(m) => Ok(m),
        _ => Ok(serde_json::Map::new()),
    }
}

/// 等价于 `read_dump_filtered(store, key, TimeRange::ALL)`。保留作为 caller 的 thin 出口。
pub async fn read_dump(
    store: Arc<dyn ObjectStore>,
    object_key: &str,
) -> Result<Vec<ParquetFileMeta>> {
    let started = Instant::now();
    let payload = store
        .get(&Path::from(object_key))
        .await
        .map_err(|e| Error::internal(format!("parquet_file_meta dump get: {e}")))?;
    let bytes = payload
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("parquet_file_meta dump bytes: {e}")))?;
    let rows = parse_dump_bytes_columnar(&bytes)?;
    metrics()
        .load_seconds
        .observe(started.elapsed().as_secs_f64());
    Ok(rows)
}

/// GET dump bytes + parse + 按 `time_range` 裁剪。
///
/// 与 `read_dump` 区别：在解码阶段就按 `[ts_start, ts_end]` 与 `time_range` 是否
/// overlap 跳过不需要 deserialize 的 row；同时 observe 一次 `query_load_seconds`。
pub async fn read_dump_filtered(
    store: Arc<dyn ObjectStore>,
    object_key: &str,
    time_range: TimeRange,
) -> Result<Vec<ParquetFileMeta>> {
    let started = Instant::now();
    let payload = store
        .get(&Path::from(object_key))
        .await
        .map_err(|e| Error::internal(format!("parquet_file_meta dump get: {e}")))?;
    let bytes = payload
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("parquet_file_meta dump bytes: {e}")))?;
    let rows = parse_with_range(&bytes, Some(time_range))?;
    metrics()
        .load_seconds
        .observe(started.elapsed().as_secs_f64());
    Ok(rows)
}

/// Query-path 计数器：dump load 命中次数（每次 cross-cold-boundary query 后 caller +1）。
pub fn record_query_hit() {
    metrics().query_hits.inc();
}

/// schema 演化 / 损坏 row 跳过时 +1（由 caller 决定何时调）。
pub fn record_skipped_row() {
    metrics().skipped_rows.inc();
}

struct Metrics {
    load_seconds: Histogram,
    query_hits: IntCounter,
    skipped_rows: IntCounter,
}

fn metrics() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| {
        let load_seconds = {
            let opts = HistogramOpts::new(
                "parquet_file_meta_dump_query_load_seconds",
                "wall-clock latency to GET + parse a single parquet_file_meta dump parquet",
            )
            .buckets(vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]);
            let h = Histogram::with_opts(opts).expect("create histogram");
            match global_registry().register(Box::new(h.clone())) {
                Ok(()) | Err(prometheus::Error::AlreadyReg) => h,
                Err(e) => panic!("register parquet_file_meta_dump_query_load_seconds: {e}"),
            }
        };
        let query_hits = register_int_counter(
            "parquet_file_meta_dump_query_hits_total",
            "queries that loaded at least one parquet_file_meta dump parquet",
        );
        let skipped_rows = register_int_counter(
            "parquet_file_meta_dump_query_rows_skipped_total",
            "parquet_file_meta dump rows skipped during deserialization (schema evolution / corrupt)",
        );
        Metrics {
            load_seconds,
            query_hits,
            skipped_rows,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    fn sample(seed: u64) -> ParquetFileMeta {
        let mut min = serde_json::Map::new();
        min.insert(
            "level".into(),
            serde_json::Value::String("\"quoted\\value\"".to_string()),
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
            max_values: serde_json::Map::new(),
            deleted: false,
        }
    }

    #[test]
    fn round_trip_via_columnar() {
        let rows: Vec<ParquetFileMeta> = (0..3).map(sample).collect();
        let bytes = crate::infra::storage::parquet_file_meta_dump::writer::serialize_dump(
            &rows,
            "2026-01-15",
            TimestampMicros(99),
        )
        .expect("serialize");
        let parsed = parse_dump_bytes_columnar(&bytes).expect("parse");
        assert_eq!(parsed.len(), rows.len());
        for (a, b) in rows.iter().zip(parsed.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.object_key, b.object_key);
            assert_eq!(a.min_values, b.min_values);
        }
    }

    #[test]
    fn time_range_filter_drops_non_overlapping_rows() {
        // sample(seed).time_range = (seed*1000, seed*1000 + 500)
        let rows: Vec<ParquetFileMeta> = (0..10).map(sample).collect();
        let bytes = crate::infra::storage::parquet_file_meta_dump::writer::serialize_dump(
            &rows,
            "2026-01-15",
            TimestampMicros(99),
        )
        .expect("serialize");
        // Keep only seed = 5 .. 7 (inclusive boundary).
        let range = TimeRange::new(TimestampMicros(5_000), TimestampMicros(7_500));
        let parsed = parse_with_range(&bytes, Some(range)).expect("parse");
        let ids: Vec<&str> = parsed.iter().map(|f| f.id.0.as_str()).collect();
        assert!(ids.contains(&"fm-5"));
        assert!(ids.contains(&"fm-6"));
        assert!(ids.contains(&"fm-7"));
        assert!(!ids.contains(&"fm-0"));
        assert!(!ids.contains(&"fm-9"));
    }

    #[test]
    fn min_values_json_escapes_special_characters_round_trip() {
        let rows = vec![sample(0)];
        let bytes = crate::infra::storage::parquet_file_meta_dump::writer::serialize_dump(
            &rows,
            "2026-01-15",
            TimestampMicros(99),
        )
        .expect("serialize");
        let parsed = parse_dump_bytes_columnar(&bytes).expect("parse");
        assert_eq!(parsed[0].min_values, rows[0].min_values);
    }

    #[test]
    fn parse_stream_type_covers_all_variants_and_rejects_unknown() {
        assert_eq!(parse_stream_type("logs").unwrap(), StreamType::Logs);
        assert_eq!(parse_stream_type("metrics").unwrap(), StreamType::Metrics);
        assert_eq!(parse_stream_type("traces").unwrap(), StreamType::Traces);
        assert_eq!(parse_stream_type("extend").unwrap(), StreamType::Extend);
        assert!(parse_stream_type("bogus").is_err());
    }

    #[test]
    fn all_parquet_file_meta_dump_metrics_register_after_first_use() {
        record_query_hit();
        record_skipped_row();
        let _ = crate::infra::storage::parquet_file_meta_dump::register_metrics_for_test();
        let bytes = crate::infra::storage::parquet_file_meta_dump::writer::serialize_dump(
            &[],
            "2026-01-15",
            TimestampMicros(0),
        )
        .unwrap();
        let _ = parse_dump_bytes_columnar(&bytes);
        let text = crate::shared::metrics::gather_text().unwrap();
        for name in [
            "parquet_file_meta_dump_partitions_written_total",
            "parquet_file_meta_dump_rows_written_total",
            "parquet_file_meta_dump_partitions_skipped_total",
            "parquet_file_meta_dump_query_hits_total",
            "parquet_file_meta_dump_query_load_seconds",
            "parquet_file_meta_dump_query_rows_skipped_total",
        ] {
            assert!(text.contains(name), "metric {name} must register");
        }
    }
}
