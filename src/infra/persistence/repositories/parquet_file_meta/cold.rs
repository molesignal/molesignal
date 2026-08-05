// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从 columnar ParquetFileMeta dump 读取指定物理数据集的冷层候选。

use std::sync::Arc;

use super::DumpQueryContext;
use crate::{
    domain::{
        storage::{ParquetFileMeta, PhysicalDatasetKind},
        stream::StreamType,
    },
    infra::{
        caching::parquet_file_meta::dump::DumpCacheKey, storage::parquet_file_meta_dump::reader,
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const DAY_MICROS: i64 = 24 * 3600 * 1_000_000;

pub(super) async fn load(
    context: &DumpQueryContext,
    org_id: &Id,
    stream: &str,
    stream_type: StreamType,
    dataset_kind: PhysicalDatasetKind,
    time_range: TimeRange,
) -> Result<Vec<ParquetFileMeta>> {
    let cold_window_start =
        TimestampMicros::now().0 - i64::from(context.cold_after_days) * DAY_MICROS;
    if time_range.start.0 >= cold_window_start {
        return Ok(Vec::new());
    }

    let dumps = context
        .dump_repo
        .find_by_time_range(org_id, stream, stream_type, dataset_kind, time_range)
        .await?;
    let mut cold = Vec::new();
    for dump in dumps {
        let cache_key = DumpCacheKey::new(
            &dump.org_id,
            &dump.stream,
            dump.stream_type,
            dump.dataset_kind,
            dump.partition_level,
            &dump.partition_key,
        );
        let all_rows = if let Some(cache) = context.dump_cache.as_ref()
            && let Some(rows) = cache.get(&cache_key).await
        {
            rows
        } else {
            match reader::read_dump(context.object_store.clone(), &dump.object_key).await {
                Ok(rows) => {
                    let rows = Arc::new(rows);
                    if let Some(cache) = context.dump_cache.as_ref() {
                        cache.insert(cache_key, rows.clone()).await;
                    }
                    rows
                }
                Err(error) => {
                    tracing::warn!(
                        object_key = %dump.object_key,
                        %error,
                        "parquet_file_meta dump load failed; skipping this dump"
                    );
                    continue;
                }
            }
        };
        cold.extend(
            all_rows
                .iter()
                .filter(|file| {
                    file.dataset_kind == dataset_kind
                        && file.time_range.end.0 >= time_range.start.0
                        && file.time_range.start.0 < time_range.end.0
                })
                .cloned(),
        );
    }
    if !cold.is_empty() {
        reader::record_query_hit();
    }
    Ok(cold)
}
