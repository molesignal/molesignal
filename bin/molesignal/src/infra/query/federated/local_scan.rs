// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 联邦 coordinator 的本集群 Parquet 扫描。

use std::sync::Arc;

use arrow::array::RecordBatch;
use datafusion::{
    common::TableReference, execution::object_store::ObjectStoreUrl, prelude::SessionContext,
};
use object_store::ObjectStore;

use crate::{
    domain::{
        query::{QueryRequest, StreamHint},
        storage::ParquetFileMetaRepository,
        stream::StreamRepository,
    },
    infra::{query::parquet_table::PrunedParquetTable, storage::parquet::reader::ParquetReader},
    shared::{Error, Result},
};

#[tracing::instrument(
    name = "query.federation.local",
    skip_all,
    fields(otel.kind = "internal", molesignal.query.stage = "local_scan")
)]
pub(super) async fn run(
    files: &Arc<dyn ParquetFileMetaRepository>,
    object_store: &Arc<dyn ObjectStore>,
    streams: Option<&Arc<dyn StreamRepository>>,
    request: &QueryRequest,
    stream: &StreamHint,
) -> Result<Vec<RecordBatch>> {
    let lookups = crate::domain::storage::logical_query_datasets(stream.stream_type)
        .iter()
        .map(|dataset_kind| {
            files.find_dataset(
                &request.org_id,
                &stream.name,
                stream.stream_type,
                *dataset_kind,
                request.time_range,
            )
        });
    let mut metas = futures::future::try_join_all(lookups)
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    metas.sort_by(|left, right| {
        right
            .time_range
            .end
            .cmp(&left.time_range.end)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    if metas.is_empty() {
        return Ok(Vec::new());
    }

    let reader = ParquetReader::new(object_store.clone());
    let schema = match streams {
        Some(streams) => match streams
            .get(&request.org_id, &stream.name, stream.stream_type)
            .await
        {
            Ok(definition) => crate::infra::storage::arrow_schema::to_arrow(&definition.schema),
            Err(_) => {
                reader
                    .schema_from_store(
                        object_store.clone(),
                        &metas[0].object_key,
                        metas[0].size_bytes,
                    )
                    .await?
            }
        },
        None => {
            reader
                .schema_from_store(
                    object_store.clone(),
                    &metas[0].object_key,
                    metas[0].size_bytes,
                )
                .await?
        }
    };
    let context = SessionContext::new();
    let store_url = ObjectStoreUrl::parse("molesignal://federation-local")
        .map_err(|error| Error::internal(format!("object store URL: {error}")))?;
    context
        .runtime_env()
        .register_object_store(store_url.as_ref(), object_store.clone());
    context
        .register_table(
            TableReference::bare(stream.name.clone()),
            Arc::new(PrunedParquetTable::new(
                schema,
                &metas,
                store_url,
                request.time_range,
                None,
            )),
        )
        .map_err(|error| Error::internal(format!("federation local register: {error}")))?;
    context
        .sql(&format!(
            "SELECT * FROM \"{}\"",
            crate::infra::query::escape_sql_ident(&stream.name)
        ))
        .await
        .map_err(|error| Error::internal(format!("federation local plan: {error}")))?
        .collect()
        .await
        .map_err(|error| Error::internal(format!("federation local collect: {error}")))
}
