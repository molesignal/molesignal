// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Hour-scoped metrics rollup orchestration.

use std::collections::BTreeMap;

use arrow::compute::concat_batches;
use object_store::{ObjectStoreExt, path::Path};

use super::{
    Compactor, cleanup::delete_file_outputs, downsampled, failures, partition::validate_group,
};
use crate::{
    domain::{
        storage::ParquetFileMeta,
        stream::{StreamDefinition, StreamType},
    },
    infra::storage::{downsample::downsample_batch, parquet::writer::is_downsampled_key},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

pub(super) async fn sweep(compactor: &Compactor, stream: &StreamDefinition) -> Result<usize> {
    let after_days = compactor.settings.downsample_after_days;
    let bucket_secs = compactor.settings.downsample_interval_secs;
    if after_days == 0 || bucket_secs == 0 || stream.stream_type != StreamType::Metrics {
        return Ok(0);
    }

    let cutoff = TimestampMicros::now().0 - i64::from(after_days) * 86_400 * 1_000_000;
    let range = TimeRange::new(TimestampMicros(0), TimestampMicros(cutoff));
    let candidates = compactor
        .parquet_file_meta
        .find(&stream.org_id, &stream.name, stream.stream_type, range)
        .await?;
    let old = candidates
        .into_iter()
        .filter(|file| !file.deleted && file.time_range.end.0 <= cutoff)
        .collect::<Vec<_>>();
    if old.is_empty() {
        return Ok(0);
    }

    let mut by_hour: BTreeMap<String, Vec<ParquetFileMeta>> = BTreeMap::new();
    for file in old {
        by_hour
            .entry(crate::domain::storage::hour_partition_path(
                file.time_range.start,
            ))
            .or_default()
            .push(file);
    }

    let mut processed = 0_usize;
    for group in by_hour.into_values() {
        validate_group(&group)?;
        let raw_files = group
            .iter()
            .filter(|file| !is_downsampled_key(&file.object_key))
            .count();
        if raw_files == 0 && group.len() <= 1 {
            continue;
        }
        match downsample_group(compactor, stream, &group, bucket_secs).await {
            Ok(true) => {
                processed += 1;
                downsampled().inc();
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(
                stream = %stream.name,
                group_size = group.len(),
                %error,
                "downsample group failed; will retry next sweep"
            ),
        }
    }
    Ok(processed)
}

async fn downsample_group(
    compactor: &Compactor,
    stream: &StreamDefinition,
    group: &[ParquetFileMeta],
    bucket_secs: u32,
) -> Result<bool> {
    let store = compactor.object_store.clone();
    let mut all_batches = Vec::new();
    for file in group {
        match compactor
            .reader
            .read_all_from_store(store.clone(), &file.object_key)
            .await
        {
            Ok(batches) => all_batches.extend(batches),
            Err(Error::NotFound(_)) => {
                failures().with_label_values(&["ghost_file"]).inc();
                tracing::warn!(
                    stream = %stream.name,
                    object_key = %file.object_key,
                    "downsample parquet_file_meta references missing object; will mark deleted"
                );
            }
            Err(error) => return Err(error),
        }
    }
    let group_ids = group
        .iter()
        .map(|file| file.id.clone())
        .collect::<Vec<Id>>();
    let group_keys = group
        .iter()
        .map(|file| file.object_key.clone())
        .collect::<Vec<_>>();
    if all_batches.is_empty() {
        compactor.parquet_file_meta.mark_deleted(&group_ids).await?;
        compactor.invalidate_tantivy_caches(&group_keys).await;
        return Ok(false);
    }

    let schema = crate::infra::storage::arrow_schema::to_arrow(&stream.schema);
    let aligned = all_batches
        .iter()
        .map(|batch| {
            crate::infra::storage::arrow_schema::align_batch_to_schema(batch, &schema)
                .map_err(|error| Error::internal(format!("downsample align schema: {error}")))
        })
        .collect::<Result<Vec<_>>>()?;
    let merged = concat_batches(&schema, &aligned)
        .map_err(|error| Error::internal(format!("downsample concat_batches: {error}")))?;
    let reduced = downsample_batch(merged, bucket_secs).await?;
    if reduced.num_rows() == 0 {
        return Ok(false);
    }

    let new_meta = compactor
        .writer
        .flush_downsampled_to_store(store.as_ref(), stream, reduced)
        .await?;
    let new_key = new_meta.object_key.clone();
    if let Err(error) = compactor
        .parquet_file_meta
        .replace(&group_ids, vec![new_meta])
        .await
    {
        failures().with_label_values(&["downsample_replace"]).inc();
        if let Err(delete_error) = store.delete(&Path::from(new_key.clone())).await {
            tracing::warn!(
                object_key = %new_key,
                error = %delete_error,
                "downsample cleanup delete failed; retention sweep will reclaim"
            );
        }
        return Err(error);
    }
    compactor.invalidate_tantivy_caches(&group_keys).await;
    for file in group {
        delete_file_outputs(store.as_ref(), file).await;
    }
    Ok(true)
}
