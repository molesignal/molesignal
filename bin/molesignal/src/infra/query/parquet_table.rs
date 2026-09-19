// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 由 QueryFile 候选清单构造的 DataFusion Parquet TableProvider。

use std::sync::Arc;

use arrow::{array::RecordBatch, compute::SortOptions, datatypes::SchemaRef};
use async_trait::async_trait;
use datafusion::{
    catalog::{
        Session, TableProvider,
        memory::{DataSourceExec, MemorySourceConfig},
    },
    common::{DFSchema, ScalarValue},
    datasource::{
        listing::PartitionedFile,
        physical_plan::{FileGroup, FileScanConfigBuilder, ParquetSource},
    },
    execution::object_store::ObjectStoreUrl,
    logical_expr::{Expr, TableProviderFilterPushDown, TableType, utils::conjunction},
    physical_expr::{LexOrdering, PhysicalSortExpr, expressions::col as physical_col},
    physical_plan::{ExecutionPlan, empty::EmptyExec, union::UnionExec},
    prelude::{col, lit},
};

use crate::{
    domain::storage::{DatasetTypeId, QueryFile},
    infra::storage::parquet::partition::storage_sort_column_names,
    shared::time::TimeRange,
};

#[derive(Debug)]
pub struct PrunedParquetTable {
    schema: SchemaRef,
    files: Vec<PartitionedFile>,
    object_store_url: ObjectStoreUrl,
    time_range: TimeRange,
    dataset_type: Option<DatasetTypeId>,
    buffered_batches: Vec<RecordBatch>,
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
        }
    }

    pub fn with_buffered_batches(mut self, batches: Vec<RecordBatch>) -> Self {
        self.buffered_batches = batches;
        self
    }

    fn physical_ordering(&self) -> Vec<LexOrdering> {
        // Writer / compactor 均按这组键降序写每个物理文件。每文件
        // 作为独立 execution partition 并声明局部有序后，DataFusion 可以对
        // ORDER BY + LIMIT 使用 SortPreservingMerge / Top-K，而不是先读完所有文件。
        let columns = storage_sort_column_names(self.schema.as_ref(), self.dataset_type.as_ref())
            .into_iter()
            .filter(|name| self.schema.index_of(name).is_ok())
            .filter_map(|name| physical_col(name, self.schema.as_ref()).ok())
            .map(|expression| {
                PhysicalSortExpr::new(
                    expression,
                    SortOptions {
                        descending: true,
                        nulls_first: false,
                    },
                )
            })
            .collect::<Vec<_>>();
        LexOrdering::new(columns).into_iter().collect()
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

            let source = ParquetSource::new(self.schema.clone())
                .with_predicate(physical_filter)
                .with_pushdown_filters(true)
                .with_bloom_filter_on_read(true);
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
