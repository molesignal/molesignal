// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按数据流现在的索引设置重建一个段的全部索引产物：读一遍它的 Parquet 文件里建索引要用的列，
//! 同时得到 Tantivy 归档、文件级过滤器与列统计。改了已有字段的索引以后，之前写下的段这样补建。

use std::sync::Arc;

use futures::StreamExt;
use object_store::ObjectStore;

use super::upload_archive;
use crate::{
    domain::{
        storage::{Artifact, ColumnStats, DataSegment, PhysicalDatasetId},
        stream::StreamDefinition,
    },
    infra::{
        search::tantivy_index::{TIMESTAMP_FIELD, TantivyArchiveBuilder},
        storage::{
            filters::{Filters, SegmentFilters},
            parquet::{
                reader::{ParquetReader, ReadOptions},
                writer::{
                    add_batch_docs,
                    column_stats::{ColumnStatsBuilder, stats_columns},
                },
            },
        },
    },
    shared::{Error, Result, ids::Id},
};

/// 一个段按数据流现在的索引设置重建出的产物。
pub(crate) struct Reindexed {
    /// 新的 Tantivy 归档：要了归档、数据流现在有建归档的字段、段的文件里有这些列时才有。
    pub archive: Option<Artifact>,
    /// 段的文件级过滤器（Bloom、N-gram 与整数字段），封存分区的分区过滤器用它。
    pub filters: Option<Filters>,
    /// 段的列统计，与重建的时刻；分片方式照旧。
    pub stats: ColumnStats,
}

/// 读 `segment` 的 Parquet 文件，按 `stream` 现在的索引设置重建它的索引产物；`archive` 为 `false`
/// 时不建 Tantivy 归档。文件按行组流式读取，只读建索引与统计要用的列。
pub(crate) async fn reindex_segment(
    source: Arc<dyn ObjectStore>,
    origin: Arc<dyn ObjectStore>,
    stream: &StreamDefinition,
    organization_id: &Id,
    dataset_id: &PhysicalDatasetId,
    segment: &DataSegment,
    archive: bool,
) -> Result<Reindexed> {
    let mut builder = match archive {
        true => TantivyArchiveBuilder::try_new(stream)
            .map_err(|error| Error::internal(format!("tantivy builder: {error}")))?,
        false => None,
    };

    let mut filters = SegmentFilters::new(stream);
    let mut stats = ColumnStatsBuilder::new(stream);

    let columns = stats_columns(stream)
        .into_iter()
        .chain([TIMESTAMP_FIELD.to_owned()])
        .collect::<Vec<_>>();

    let columns = columns.iter().map(String::as_str).collect::<Vec<_>>();

    let mut batches = ParquetReader::new(source.clone())
        .stream_from_store(
            source,
            segment.primary.object.key.as_str(),
            ReadOptions::new().with_columns(&columns),
        )
        .await?;

    let mut rows = 0_usize;

    while let Some(batch) = batches.next().await {
        let batch = batch?;
        rows += batch.num_rows();

        // 文件里没有建归档的列（字段是后来才有的）：这个段不要归档。
        if let Some(index) = builder.as_mut()
            && !add_batch_docs(index, stream, &batch)?
        {
            builder = None;
        }

        if let Some(filters) = filters.as_mut() {
            filters.add(&batch);
        }

        stats.add(stream, &batch);
    }

    if rows == 0 {
        return Err(Error::internal(
            "primary artifact contained no record batches",
        ));
    }

    let mut stats = stats.finish();
    stats.partitions = segment.column_stats.partitions.clone();

    let archive = match builder {
        Some(builder) => Some(
            upload_archive(
                origin,
                builder,
                stream,
                organization_id,
                dataset_id,
                segment,
            )
            .await?,
        ),
        None => None,
    };

    Ok(Reindexed {
        archive,
        filters: filters.and_then(|filters| filters.finish(segment.id.as_str())),
        stats,
    })
}
