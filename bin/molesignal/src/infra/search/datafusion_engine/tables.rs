// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次查询注册的表与执行：每张表的文件、内存缓冲与 schema，以及把语句交给 DataFusion 执行、
//! 收集结果。

use std::{collections::HashMap, sync::Arc};

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use datafusion::{
    common::TableReference,
    datasource::physical_plan::parquet::ParquetRowSelection,
    error::DataFusionError,
    execution::object_store::ObjectStoreUrl,
    prelude::{DataFrame, SessionContext},
};

use super::DataFusionEngine;
use crate::{
    domain::storage::{DatasetTypeId, QueryFile},
    infra::query::{current_views::CurrentView, parquet_table::PrunedParquetTable},
    shared::{Error, Result, time::TimeRange},
};

/// 一张注册进本次查询的表：它的文件、内存缓冲与 schema，以及某些文件只读哪些行。按时间分批
/// 执行时，主表用其中一部分文件重新注册。
pub(super) struct TableInput {
    pub(super) name: String,
    pub(super) schema: SchemaRef,
    pub(super) files: Vec<QueryFile>,
    pub(super) buffered: Vec<RecordBatch>,
    pub(super) dataset: Option<DatasetTypeId>,
    /// 按 object key：文件的 Tantivy Artifact，从中找等值条件命中的行。
    pub(super) index_keys: HashMap<String, String>,
    /// 按 object key：这些文件只读选中的行。
    pub(super) selections: HashMap<String, ParquetRowSelection>,
    /// 语句读的、由这张表派生的当前版本视图；表每注册一次，视图跟着重新注册。
    pub(super) views: Vec<&'static CurrentView>,
}

impl TableInput {
    pub(super) fn register(
        &self,
        ctx: &SessionContext,
        engine: &DataFusionEngine,
        files: &[QueryFile],
        object_store_url: &ObjectStoreUrl,
        time_range: TimeRange,
    ) -> Result<()> {
        let table = Arc::new(
            PrunedParquetTable::new(
                self.schema.clone(),
                files,
                object_store_url.clone(),
                time_range,
                self.dataset.clone(),
            )
            .with_buffered_batches(self.buffered.clone())
            .with_metadata_cache(engine.parquet_metadata.clone())
            .with_row_selections(&self.selections),
        );

        let name = TableReference::bare(self.name.clone());

        ctx.deregister_table(name.clone())
            .map_err(|error| Error::internal(format!("datafusion deregister: {error}")))?;

        ctx.register_table(name, table.clone())
            .map_err(|error| Error::internal(format!("datafusion register: {error}")))?;

        for view in &self.views {
            view.register(ctx, table.clone())?;
        }

        Ok(())
    }
}

pub(super) fn plan_error(sql: &str) -> impl FnOnce(DataFusionError) -> Error + '_ {
    move |error| {
        tracing::warn!(%error, %sql, "query planning failed");

        Error::invalid(
            "query could not be planned: check the SQL syntax and that every referenced field exists in the stream",
        )
    }
}

pub(super) async fn collect(
    ctx: &SessionContext,
    sql: &str,
    limit: Option<usize>,
) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    let dataframe = ctx.sql(sql).await.map_err(plan_error(sql))?;
    collect_planned(dataframe, limit).await
}

pub(super) async fn collect_planned(
    dataframe: DataFrame,
    limit: Option<usize>,
) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    let dataframe = match limit {
        Some(limit) => dataframe
            .limit(0, Some(limit))
            .map_err(|error| Error::internal(format!("datafusion limit: {error}")))?,
        None => dataframe,
    };

    let schema = dataframe.schema().inner().clone();
    let batches = dataframe.collect().await.map_err(collect_error)?;

    Ok((schema, batches))
}

/// `collect()` 才会触发 DataFusion 的惰性分析（类型强制等），这类失败是 SQL 本身的问题，
/// 按无效请求应答 400；其余执行期错误是内部错误。
pub(super) fn collect_error(error: DataFusionError) -> Error {
    let root = error.find_root();

    if matches!(
        root,
        DataFusionError::Plan(_) | DataFusionError::SchemaError(..)
    ) {
        tracing::warn!(%error, "query planning failed");
        return Error::invalid(format!("query could not be planned: {root}"));
    }

    Error::internal(format!("datafusion collect: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_failures_raised_by_collect_are_client_errors() {
        let planning = DataFusionError::Context(
            "type_coercion".into(),
            Box::new(DataFusionError::Plan(
                "Cannot infer common argument type for comparison operation".into(),
            )),
        );
        assert!(matches!(
            collect_error(planning),
            Error::InvalidArgument(message) if message.contains("could not be planned")
        ));
        assert!(matches!(
            collect_error(DataFusionError::Execution("boom".into())),
            Error::Internal(_)
        ));
    }
}
