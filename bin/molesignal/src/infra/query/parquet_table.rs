// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 由 QueryFile 候选清单构造的 DataFusion Parquet TableProvider。

use std::{collections::HashMap, sync::Arc};

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use async_trait::async_trait;
use datafusion::{
    catalog::{
        Session, TableProvider,
        memory::{DataSourceExec, MemorySourceConfig},
    },
    common::{DFSchema, ScalarValue},
    datasource::{
        listing::PartitionedFile,
        physical_plan::{
            FileGroup, FileScanConfigBuilder, ParquetSource,
            parquet::{CachedParquetFileReaderFactory, ParquetRowSelection},
        },
    },
    execution::{
        cache::{
            cache_manager::{CachedFileMetadataEntry, FileMetadataCache},
            default_cache::DefaultCache,
        },
        object_store::ObjectStoreUrl,
    },
    logical_expr::{Expr, TableProviderFilterPushDown, TableType, utils::conjunction},
    physical_expr::LexOrdering,
    physical_plan::{ExecutionPlan, empty::EmptyExec, union::UnionExec},
    prelude::{col, lit},
};

use crate::{
    domain::storage::{DatasetTypeId, QueryFile},
    infra::storage::parquet::partition::storage_ordering,
    shared::time::TimeRange,
};

/// Footer bytes read with the first request to each file: the Thrift footer and the page index
/// that precedes it usually fit, so opening a file costs one request instead of two or three.
const FOOTER_READ_BYTES: usize = 512 * 1024;

/// Parsed Parquet footers (with page indexes) shared across queries, keyed by object key.
/// Primary Artifact objects are immutable, so an entry never goes stale.
pub type ParquetMetadataCache = Arc<FileMetadataCache>;

/// `None` when `max_bytes` is 0 (the cache is off).
pub fn parquet_metadata_cache(max_bytes: u64) -> Option<ParquetMetadataCache> {
    let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);

    (limit > 0).then(|| {
        Arc::new(
            DefaultCache::<object_store::path::Path, CachedFileMetadataEntry>::new(limit)
                .with_name("parquet_metadata"),
        ) as ParquetMetadataCache
    })
}

#[derive(Debug)]
pub struct PrunedParquetTable {
    schema: SchemaRef,
    files: Vec<PartitionedFile>,
    object_store_url: ObjectStoreUrl,
    time_range: TimeRange,
    dataset_type: Option<DatasetTypeId>,
    buffered_batches: Vec<RecordBatch>,
    metadata_cache: Option<ParquetMetadataCache>,
}

impl PrunedParquetTable {
    pub fn new(
        schema: SchemaRef,
        query_files: &[QueryFile],
        object_store_url: ObjectStoreUrl,
        time_range: TimeRange,
        dataset_type: Option<DatasetTypeId>,
    ) -> Self {
        Self {
            schema,
            files: query_files
                .iter()
                .map(|file| PartitionedFile::new(file.object_key.clone(), file.size_bytes))
                .collect(),
            object_store_url,
            time_range,
            dataset_type,
            buffered_batches: Vec::new(),
            metadata_cache: None,
        }
    }

    pub fn with_buffered_batches(mut self, batches: Vec<RecordBatch>) -> Self {
        self.buffered_batches = batches;
        self
    }

    pub fn with_metadata_cache(mut self, cache: Option<ParquetMetadataCache>) -> Self {
        self.metadata_cache = cache;
        self
    }

    /// Rows to read from some files, by object key: a file with a selection is read only where
    /// those rows lie. The filters still run on every row read, so a selection only has to hold
    /// every row the query can return.
    pub fn with_row_selections(
        mut self,
        selections: &HashMap<String, ParquetRowSelection>,
    ) -> Self {
        if selections.is_empty() {
            return self;
        }

        self.files = std::mem::take(&mut self.files)
            .into_iter()
            .map(
                |file| match selections.get(file.object_meta.location.as_ref()) {
                    Some(selection) => file.with_extension(selection.clone()),
                    None => file,
                },
            )
            .collect();

        self
    }

    fn physical_ordering(&self) -> Vec<LexOrdering> {
        // Writer / compactor 均按这组键降序写每个物理文件。每文件
        // 作为独立 execution partition 并声明局部有序后，DataFusion 可以对
        // ORDER BY + LIMIT 使用 SortPreservingMerge / Top-K，而不是先读完所有文件。
        storage_ordering(self.schema.as_ref(), self.dataset_type.as_ref())
            .into_iter()
            .collect()
    }
}

#[async_trait]
impl TableProvider for PrunedParquetTable {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> datafusion::common::Result<Vec<TableProviderFilterPushDown>> {
        Ok(vec![TableProviderFilterPushDown::Inexact; filters.len()])
    }

    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> datafusion::common::Result<Arc<dyn ExecutionPlan>> {
        if self.files.is_empty() && self.buffered_batches.is_empty() {
            let schema = projection
                .map(|indices| self.schema.project(indices).map(Arc::new))
                .transpose()?
                .unwrap_or_else(|| self.schema.clone());

            return Ok(Arc::new(EmptyExec::new(schema)));
        }

        let mut plans: Vec<Arc<dyn ExecutionPlan>> = Vec::with_capacity(2);

        if !self.files.is_empty() {
            // QueryRequest 的绝对时间窗在物理扫描层强制执行。使用带 UTC 类型的 literal，
            // 避免 Timestamp(µs, UTC) 与 Int64 比较时的规划错误。
            let timezone = Some("UTC".into());

            let time_filter = col("_timestamp")
                .gt_eq(lit(ScalarValue::TimestampMicrosecond(
                    Some(self.time_range.start.0),
                    timezone.clone(),
                )))
                .and(col("_timestamp").lt(lit(ScalarValue::TimestampMicrosecond(
                    Some(self.time_range.end.0),
                    timezone,
                ))));

            let mut predicates = filters.to_vec();
            predicates.push(time_filter);
            let logical_filter = conjunction(predicates).unwrap_or_else(|| lit(true));
            let df_schema = DFSchema::try_from(self.schema.clone())?;
            let physical_filter = state.create_physical_expr(logical_filter, &df_schema)?;

            let mut source = ParquetSource::new(self.schema.clone())
                .with_predicate(physical_filter)
                .with_pushdown_filters(true)
                .with_bloom_filter_on_read(true)
                .with_metadata_size_hint(FOOTER_READ_BYTES);

            if let Some(cache) = &self.metadata_cache {
                // The org-scoped store this query registered; the cache only skips footer reads.
                let store = state.runtime_env().object_store(&self.object_store_url)?;

                source = source.with_parquet_file_reader_factory(Arc::new(
                    CachedParquetFileReaderFactory::new(store, cache.clone()),
                ));
            }

            let file_groups = self
                .files
                .iter()
                .cloned()
                .map(|file| FileGroup::new(vec![file]))
                .collect();

            let config =
                FileScanConfigBuilder::new(self.object_store_url.clone(), Arc::new(source))
                    .with_projection_indices(projection.cloned())?
                    .with_limit(limit)
                    .with_file_groups(file_groups)
                    .with_output_ordering(self.physical_ordering())
                    .build();

            plans.push(Arc::new(DataSourceExec::new(Arc::new(config))));
        }

        if !self.buffered_batches.is_empty() {
            plans.push(MemorySourceConfig::try_new_exec(
                std::slice::from_ref(&self.buffered_batches),
                self.schema.clone(),
                projection.cloned(),
            )?);
        }

        UnionExec::try_new(plans)
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Int64Array, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::{common::TableReference, prelude::SessionContext};

    use super::*;
    use crate::shared::time::TimestampMicros;

    #[tokio::test]
    async fn buffered_batches_participate_in_normal_filter_and_projection() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![10, 20, 30]).with_timezone("UTC")),
                Arc::new(Int64Array::from(vec![1, 2, 3])),
            ],
        )
        .unwrap();
        let table = PrunedParquetTable::new(
            schema,
            &[],
            ObjectStoreUrl::parse("molesignal://buffer-test").unwrap(),
            TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
            None,
        )
        .with_buffered_batches(vec![batch]);
        let context = SessionContext::new();
        context
            .register_table(TableReference::bare("metrics"), Arc::new(table))
            .unwrap();

        let batches = context
            .sql("SELECT value FROM metrics WHERE value >= 2 ORDER BY value")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let values = batches
            .iter()
            .flat_map(|batch| {
                batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap()
                    .values()
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(values, vec![2, 3]);
    }
}
