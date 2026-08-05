// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 引擎内部物理数据集选择与 ParquetFileMeta 合并。

use std::sync::Arc;

use futures::future::try_join_all;

use crate::{
    domain::{
        storage::{ParquetFileMeta, ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimeRange},
};

pub(super) async fn load_files(
    repository: &Arc<dyn ParquetFileMetaRepository>,
    org_id: &Id,
    stream: &str,
    stream_type: StreamType,
    selected: Option<PhysicalDatasetKind>,
    range: TimeRange,
) -> Result<Vec<ParquetFileMeta>> {
    let mut files = if let Some(dataset_kind) = selected {
        repository
            .find_dataset(org_id, stream, stream_type, dataset_kind, range)
            .await?
    } else {
        let lookups = crate::domain::storage::logical_query_datasets(stream_type)
            .iter()
            .map(|dataset_kind| {
                repository.find_dataset(org_id, stream, stream_type, *dataset_kind, range)
            });
        try_join_all(lookups).await?.into_iter().flatten().collect()
    };
    files.sort_by(|left, right| {
        right
            .time_range
            .end
            .cmp(&left.time_range.end)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    files.dedup_by(|left, right| left.id == right.id);
    Ok(files)
}
