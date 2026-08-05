// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Physical-file execution for the DataFusion query engine.

use std::{collections::HashSet, sync::Arc, time::Instant};

use arrow::datatypes::{DataType, Schema as ArrowSchema, TimeUnit};
use datafusion::{common::TableReference, execution::object_store::ObjectStoreUrl};

use super::{DataFusionEngine, dataset, resolve_stream_type, result};
use crate::{
    domain::{
        masking::Masker,
        query::{QueryRequest, QueryResult, StreamHint},
        storage::PhysicalDatasetKind,
        stream::StreamType as StreamTypeEnum,
    },
    infra::{
        query::{
            parquet_table::PrunedParquetTable,
            parser::{extract_equality_predicates, extract_referenced_tables, parse_sample_hint},
            planner::ensure_stream_in_org,
            tantivy_pruner::{MatchPredicate, extract_match_predicates},
            udfs::{build_extract_pattern_udf, build_mask_udf, compile_patterns},
        },
        storage::parquet::reader::ParquetReader,
    },
    shared::{Error, Result},
};

pub(super) async fn run(
    engine: &DataFusionEngine,
    req: QueryRequest,
    primary_dataset: Option<PhysicalDatasetKind>,
) -> Result<QueryResult> {
    let started = Instant::now();
    let StreamHint { name, stream_type } = req.stream.clone().ok_or_else(|| {
        Error::invalid("query.stream hint is required; caller must name the target table")
    })?;

    if let Some(streams) = &engine.streams {
        ensure_stream_in_org(streams.as_ref(), &req.org_id, &name, stream_type).await?;
    }

    let (sample_cap, statement) = parse_sample_hint(&req.statement);
    let (mut predicates, rewritten_sql) = extract_match_predicates(&statement);
    add_exact_predicates(
        engine,
        &req,
        &name,
        stream_type,
        &statement,
        &mut predicates,
    )
    .await;

    let store = engine.object_store.clone();
    let reader = ParquetReader::new(store.clone());
    let ctx = crate::infra::query::analyzer::session_context_with_guard(engine.max_result_rows);
    let object_store_url = ObjectStoreUrl::parse("molesignal://query")
        .map_err(|error| Error::internal(format!("object store URL: {error}")))?;
    ctx.runtime_env()
        .register_object_store(object_store_url.as_ref(), store.clone());
    ctx.register_udaf(crate::infra::query::udafs::approx_topk_udf());
    register_query_udfs(engine, &req, &rewritten_sql, &ctx).await?;

    let mut tables = vec![(name.clone(), stream_type, primary_dataset)];
    if let Some(streams) = &engine.streams {
        for reference in extract_referenced_tables(&rewritten_sql).unwrap_or_default() {
            if reference.name == name {
                continue;
            }
            if let Some(stream_type) =
                resolve_stream_type(streams.as_ref(), &req.org_id, &reference.name).await
            {
                ensure_stream_in_org(streams.as_ref(), &req.org_id, &reference.name, stream_type)
                    .await?;
                tables.push((reference.name, stream_type, None));
            }
        }
    }

    let mut scanned_rows = 0_u64;
    for (table_name, stream_type, selected_dataset) in &tables {
        let mut files = dataset::load_files(
            &engine.files,
            &req.org_id,
            table_name,
            *stream_type,
            *selected_dataset,
            req.time_range,
        )
        .await?;
        if !predicates.is_empty()
            && table_name == &name
            && let Some(pruner) = &engine.tantivy_pruner
        {
            files = pruner
                .prune(files, &predicates)
                .await
                .map_err(|error| Error::internal(format!("tantivy prune: {error}")))?;
        }
        if let Some(cap) = sample_cap
            && table_name == &name
        {
            let mut candidate_rows = 0_u64;
            files.retain(|file| {
                if candidate_rows >= cap {
                    return false;
                }
                candidate_rows = candidate_rows.saturating_add(file.rows);
                true
            });
        }
        scanned_rows = scanned_rows.saturating_add(files.iter().map(|file| file.rows).sum::<u64>());

        let stream_definition = if let Some(streams) = &engine.streams {
            streams
                .get(&req.org_id, table_name, *stream_type)
                .await
                .ok()
        } else {
            None
        };
        let schema: Arc<ArrowSchema> = match stream_definition {
            Some(definition) => {
                let definition = selected_dataset
                    .map(|kind| crate::infra::ingester::physical_schema::project(&definition, kind))
                    .unwrap_or(definition);
                crate::infra::storage::arrow_schema::to_arrow(&definition.schema)
            }
            None => match files.first() {
                Some(file) => {
                    reader
                        .schema_from_store(store.clone(), &file.object_key, file.size_bytes)
                        .await?
                }
                None => Arc::new(ArrowSchema::new(vec![arrow::datatypes::Field::new(
                    "_timestamp",
                    DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                    false,
                )])),
            },
        };
        let table = PrunedParquetTable::new(
            schema,
            &files,
            object_store_url.clone(),
            req.time_range,
            *selected_dataset,
        );
        ctx.register_table(TableReference::bare(table_name.clone()), Arc::new(table))
            .map_err(|error| Error::internal(format!("datafusion register: {error}")))?;
    }

    let dataframe = ctx.sql(&rewritten_sql).await.map_err(|error| {
        tracing::warn!(%error, sql = %rewritten_sql, "query planning failed");
        Error::invalid(
            "query could not be planned: check the SQL syntax and that every referenced field exists in the stream",
        )
    })?;
    let dataframe = if let Some(limit) = req.limit {
        dataframe
            .limit(0, Some(limit))
            .map_err(|error| Error::internal(format!("datafusion limit: {error}")))?
    } else {
        dataframe
    };
    let batches = dataframe
        .collect()
        .await
        .map_err(|error| Error::internal(format!("datafusion collect: {error}")))?;
    let (columns, rows) = result::batches_to_json(&batches);
    Ok(QueryResult {
        columns,
        rows,
        scanned_rows,
        took_ms: started.elapsed().as_millis() as u64,
        federation: None,
    })
}

async fn add_exact_predicates(
    engine: &DataFusionEngine,
    req: &QueryRequest,
    stream: &str,
    stream_type: StreamTypeEnum,
    statement: &str,
    predicates: &mut Vec<MatchPredicate>,
) {
    let exact = extract_equality_predicates(statement);
    if exact.is_empty() {
        return;
    }
    let Some(streams) = &engine.streams else {
        return;
    };
    let Ok(definition) = streams.get(&req.org_id, stream, stream_type).await else {
        return;
    };
    let indexed = definition
        .schema
        .fields
        .iter()
        .filter(|field| field.indexed && field.exact)
        .map(|field| field.name.as_str())
        .collect::<HashSet<_>>();
    predicates.extend(
        exact
            .into_iter()
            .filter(|(column, _)| indexed.contains(column.as_str()))
            .map(|(field, term)| MatchPredicate { field, term }),
    );
}

async fn register_query_udfs(
    engine: &DataFusionEngine,
    req: &QueryRequest,
    statement: &str,
    ctx: &datafusion::prelude::SessionContext,
) -> Result<()> {
    if let Some(service) = &engine.field_keys {
        let keys = service.decrypt_map(&req.org_id).await?;
        ctx.register_udf(crate::infra::query::udfs::build_decrypt_udf(keys));
    }
    if statement.contains("extract_pattern(")
        && let Some(repository) = &engine.log_patterns
    {
        let patterns = repository.list(&req.org_id).await.unwrap_or_default();
        let rows = patterns
            .into_iter()
            .map(|pattern| (pattern.regex, pattern.category, pattern.priority))
            .collect();
        ctx.register_udf(build_extract_pattern_udf(compile_patterns(rows)));
    }
    if statement.contains("mask(")
        && let Some(repository) = &engine.regex_patterns
    {
        let patterns = repository.list(&req.org_id).await.unwrap_or_default();
        let masker = Masker::compile(
            patterns
                .into_iter()
                .map(|pattern| (pattern.pattern, pattern.replacement)),
        );
        ctx.register_udf(build_mask_udf(Arc::new(masker)));
    }
    Ok(())
}
