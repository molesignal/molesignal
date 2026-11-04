// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Physical-file execution for the DataFusion query engine.

use std::{collections::HashMap, sync::Arc, time::Instant};

use arrow::datatypes::{DataType, Schema as ArrowSchema, TimeUnit};
use datafusion::execution::object_store::ObjectStoreUrl;

use super::{
    DataFusionEngine,
    blooms::{self, FilterTerms},
    counts::Counted,
    dataset,
    paging::{self, Page},
    resolve_stream_type,
    selection::{exact_terms, row_selections, rows_to_read},
    shards,
    tables::{TableInput, collect_planned, plan_error},
    waves::time_order,
};
use crate::{
    domain::{
        masking::Masker,
        query::{QueryRequest, QueryResult, StreamHint},
        storage::DatasetTypeId,
        stream::{StreamIndexType, StreamRepository, StreamType as StreamTypeEnum},
    },
    infra::{
        query::{
            arrow_json::batches_to_json,
            catalog_source::StreamSnapshotSelection,
            current_views,
            parser::{WhereTree, equality_sets, extract_referenced_tables, parse_sample_hint},
            planner::ensure_stream_in_org,
            refutations::Refutations,
            scan_limit::ensure_within_scan_limit,
            skip_pruner,
            tantivy_pruner::{self, indexed_atoms, match_text_fields, rewrite_match_calls},
            udfs::{build_extract_pattern_udf, build_mask_udf, compile_patterns},
        },
        storage::{org_scoped_store::scoped_to_org, parquet::reader::ParquetReader},
    },
    shared::{Error, Result},
};

pub(super) async fn run(
    engine: &DataFusionEngine,
    req: QueryRequest,
    primary_dataset: Option<DatasetTypeId>,
) -> Result<QueryResult> {
    let started = Instant::now();

    let StreamHint { name, stream_type } = req.stream.clone().ok_or_else(|| {
        Error::invalid("query.stream hint is required; caller must name the target table")
    })?;

    // 提示写成当前版本视图的名字，说的就是视图所属的流。
    let name = current_views::base_stream(&name).map_or(name, str::to_string);

    if let Some(streams) = &engine.streams {
        ensure_stream_in_org(
            streams.as_ref(),
            &req.org_id,
            &name,
            stream_type,
            req.time_range,
        )
        .await?;
    }

    let (sample_cap, statement) = parse_sample_hint(&req.statement);

    validate_match_text_fields(
        engine.streams.as_deref(),
        &req,
        &name,
        stream_type,
        &statement,
    )
    .await?;

    let rewritten_sql = rewrite_match_calls(&statement);

    // 读当前版本视图的流不能按语句剪文件：不匹配的行可能是某个实体的最新版本，而它的旧版本匹配。
    let views = current_views::mentioned(&rewritten_sql);
    let viewed = views.iter().any(|view| view.base() == name);

    // 数据文件只能从请求组织自己的对象前缀读取，即使 Catalog 返回了越界的 key。
    let store = scoped_to_org(engine.object_store.clone(), &req.org_id);
    let reader = ParquetReader::new(store.clone());
    let ctx = crate::infra::query::analyzer::session_context(engine.max_result_rows);
    let object_store_url = ObjectStoreUrl::parse("molesignal://query")
        .map_err(|error| Error::internal(format!("object store URL: {error}")))?;
    ctx.runtime_env()
        .register_object_store(object_store_url.as_ref(), store.clone());
    ctx.register_udaf(crate::infra::query::udafs::approx_topk_udf());
    register_query_udfs(engine, &req, &rewritten_sql, &ctx).await?;

    let mut tables = vec![(name.clone(), stream_type, primary_dataset)];

    if let Some(streams) = &engine.streams {
        for reference in extract_referenced_tables(&rewritten_sql).unwrap_or_default() {
            // 当前版本视图读它所属的流。
            let table: &str =
                current_views::base_stream(&reference.name).unwrap_or(reference.name.as_str());

            if table == name || tables.iter().any(|(known, ..)| known == table) {
                continue;
            }

            if let Some(stream_type) =
                resolve_stream_type(streams.as_ref(), &req.org_id, table).await
            {
                ensure_stream_in_org(
                    streams.as_ref(),
                    &req.org_id,
                    table,
                    stream_type,
                    req.time_range,
                )
                .await?;
                tables.push((table.to_string(), stream_type, None));
            }
        }
    }

    let mut stream_definitions = Vec::with_capacity(tables.len());

    for (table_name, stream_type, _) in &tables {
        let definition = match &engine.streams {
            Some(streams) => Some(streams.get(&req.org_id, table_name, *stream_type).await?),
            None => None,
        };

        stream_definitions.push(definition);
    }

    // 只数行数的语句与 explorer 直方图：能用 Catalog 行数或 Tantivy 索引回答的文件不读数据。
    let mut counted = (sample_cap.is_none() && tables.len() == 1 && !viewed)
        .then(|| {
            Counted::plan(
                engine,
                &req,
                &rewritten_sql,
                &name,
                stream_definitions[0].as_ref(),
            )
        })
        .flatten();

    let mut catalog_snapshots = std::iter::repeat_with(|| None)
        .take(tables.len())
        .collect::<Vec<_>>();

    if let Some(source) = &engine.catalog_source
        && stream_definitions.iter().all(Option::is_some)
    {
        let selections = tables
            .iter()
            .zip(&stream_definitions)
            .map(|((_, stream_type, selected_dataset), definition)| {
                let dataset_types = match selected_dataset.clone() {
                    Some(dataset_type) => vec![dataset_type],
                    None => crate::domain::storage::logical_query_dataset_types(*stream_type)?,
                };

                Ok(StreamSnapshotSelection::new(
                    definition.clone().expect("all definitions checked above"),
                    dataset_types,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        catalog_snapshots = source
            .snapshot_streams(&selections, req.time_range)
            .await?
            .into_iter()
            .map(Some)
            .collect();
    }

    let mut scanned_rows = 0_u64;
    let mut main_table = None;

    // 剪枝的每一步证明条件树里自己看得懂的原子不成立，按文件累积，见 [`Refutations`]。
    let tree = match viewed {
        true => WhereTree::default(),
        false => WhereTree::parse(&rewritten_sql),
    };

    let definition = stream_definitions[0].as_ref();
    let shard_atoms = shards::Atoms::new(&tree);
    let filter_terms = FilterTerms::new(definition, &tree);
    let skip_atoms =
        definition.map(|definition| skip_pruner::Atoms::new(&tree, &definition.schema));
    let tantivy_atoms =
        definition.map_or_else(Vec::new, |definition| indexed_atoms(&tree, definition));

    // 只读命中行用的等值条件：整值字段上的，与 JSON 字段的键值。
    let exact_terms = match viewed {
        true => Vec::new(),
        false => exact_terms(&equality_sets(&rewritten_sql), definition)
            .into_iter()
            .chain(tantivy_pruner::key_value_conjuncts(&tree, &tantivy_atoms))
            .collect(),
    };

    let mut refutations = Refutations::new(tree);

    // 每一步能证明的原子都不成立也剔除不了文件时，哪一步都不必做。
    let prunable = refutations.could_drop(
        shard_atoms.mask()
            | filter_terms.mask()
            | skip_atoms.as_ref().map_or(0, skip_pruner::Atoms::mask)
            | tantivy_pruner::mask(&tantivy_atoms),
    );

    let tantivy_atoms = match prunable {
        true => tantivy_atoms,
        false => Vec::new(),
    };

    // 单表、不抽样、不由索引计数的语句先不剪枝：按时间取一页时只剪要读的文件，否则规划后
    // 一次剪完。
    let defer = tables.len() == 1 && sample_cap.is_none() && counted.is_none();

    for (index, (table_name, stream_type, selected_dataset)) in tables.iter().enumerate() {
        let stream_definition = stream_definitions[index].clone();

        let loaded = if let Some(snapshot) = catalog_snapshots[index].take() {
            dataset::LoadedDataset::from_catalog_snapshot(snapshot)
        } else {
            dataset::load(
                &engine.files,
                engine.catalog_source.as_ref(),
                &engine.explicit_tantivy_indexes,
                stream_definition.as_ref(),
                dataset::DatasetLoadSelection {
                    organization_id: &req.org_id,
                    stream_name: table_name,
                    stream_type: *stream_type,
                    dataset_type: selected_dataset.clone(),
                    time_range: req.time_range,
                },
            )
            .await?
        };

        let mut files = loaded.files;
        let tantivy_indexes = loaded.tantivy_indexes;
        let buffered_batches = loaded.buffered_batches;

        // 分片、文件级过滤器与文件统计先排除不可能命中的文件，再打开其余文件的索引。
        if table_name == &name && prunable {
            files = refutations.retain(files, |file| shard_atoms.refuted(file));

            files = blooms::prune(
                engine,
                files,
                &filter_terms,
                &mut refutations,
                &loaded.bloom_keys,
                &loaded.filter_keys,
            )
            .await;

            if let Some(skip_atoms) = &skip_atoms {
                files = refutations.retain(files, |file| skip_atoms.refuted(file));
            }
        }

        if !tantivy_atoms.is_empty()
            && !defer
            && table_name == &name
            && let Some(pruner) = &engine.tantivy_pruner
        {
            files = pruner
                .prune_counting(files, &tantivy_atoms, &mut refutations, &tantivy_indexes)
                .await
                .map_err(|error| Error::internal(format!("tantivy prune: {error}")))?
                .files;
        }

        if let Some(counted) = counted.as_mut()
            && table_name == &name
        {
            files = counted
                .take(engine, files, req.time_range, &tantivy_indexes)
                .await;
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

        scanned_rows = scanned_rows
            .saturating_add(files.iter().map(|file| file.rows).sum::<u64>())
            .saturating_add(
                buffered_batches
                    .iter()
                    .map(|batch| batch.num_rows() as u64)
                    .sum::<u64>(),
            );

        let schema: Arc<ArrowSchema> = match stream_definition {
            Some(definition) => {
                let definition = selected_dataset
                    .as_ref()
                    .map(|dataset_type| {
                        crate::infra::intake::physical_schema::project(&definition, dataset_type)
                    })
                    .unwrap_or(definition);

                crate::infra::storage::arrow_schema::to_arrow(&definition.schema)
            }
            None => match files.first() {
                Some(file) => {
                    reader
                        .schema_from_store(store.clone(), &file.object_key, file.size_bytes)
                        .await?
                }
                None => buffered_batches
                    .first()
                    .map(|batch| batch.schema())
                    .unwrap_or_else(|| {
                        Arc::new(ArrowSchema::new(vec![arrow::datatypes::Field::new(
                            "_timestamp",
                            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                            false,
                        )]))
                    }),
            },
        };

        let table = TableInput {
            name: table_name.clone(),
            schema,
            files,
            buffered: buffered_batches,
            dataset: selected_dataset.clone(),
            index_keys: tantivy_indexes,
            selections: HashMap::new(),
            views: views
                .iter()
                .filter(|view| view.base() == table_name.as_str())
                .copied()
                .collect(),
        };

        table.register(
            &ctx,
            engine,
            &table.files,
            &object_store_url,
            req.time_range,
        )?;

        if table_name == &name {
            main_table = Some(table);
        }
    }

    let planned = counted.as_ref().map_or(rewritten_sql.as_str(), |counted| {
        counted.statement(&rewritten_sql)
    });

    let mut dataframe = ctx.sql(planned).await.map_err(plan_error(planned))?;

    let order = (tables.len() == 1 && sample_cap.is_none() && counted.is_none() && !viewed)
        .then(|| time_order(dataframe.logical_plan()))
        .flatten();

    let batches = match (order, main_table) {
        (Some(order), Some(table)) if table.files.len() > 1 => {
            let page = Page {
                engine,
                ctx: &ctx,
                object_store_url: &object_store_url,
                time_range: req.time_range,
                limit: req.limit,
                sql: &rewritten_sql,
                atoms: if defer { &tantivy_atoms } else { &[] },
                refutations: &mut refutations,
                exact_terms: &exact_terms,
            };

            let (batches, scanned) = paging::read(page, order, table).await?;
            scanned_rows = scanned;
            batches
        }
        (_, main) => {
            // 剪枝推迟到这里的文件一次剪完；等值条件命中很少行的文件只读命中的行所在的页。
            // 文件或要读的行变了，主表重新注册、语句重新规划。
            if let Some(mut table) = main {
                let all = table.files.iter().map(|file| file.rows).sum::<u64>();
                let before = table.files.len();

                if defer
                    && !tantivy_atoms.is_empty()
                    && let Some(pruner) = &engine.tantivy_pruner
                {
                    table.files = pruner
                        .prune_counting(
                            std::mem::take(&mut table.files),
                            &tantivy_atoms,
                            &mut refutations,
                            &table.index_keys,
                        )
                        .await
                        .map_err(|error| Error::internal(format!("tantivy prune: {error}")))?
                        .files;
                }

                table.selections =
                    row_selections(engine, &table.files, &exact_terms, &table.index_keys).await;

                if table.files.len() != before || !table.selections.is_empty() {
                    scanned_rows = scanned_rows
                        .saturating_sub(all)
                        .saturating_add(rows_to_read(&table.files, &table.selections));

                    table.register(
                        &ctx,
                        engine,
                        &table.files,
                        &object_store_url,
                        req.time_range,
                    )?;
                    dataframe = ctx.sql(planned).await.map_err(plan_error(planned))?;
                }
            }

            ensure_within_scan_limit(scanned_rows, engine.max_scan_rows)?;

            let (schema, mut batches) = collect_planned(dataframe, req.limit).await?;

            if let Some(counted) = &counted {
                counted.add_to(&mut batches, schema)?;
            }

            batches
        }
    };

    let (columns, rows) = batches_to_json(&batches);

    Ok(QueryResult {
        columns,
        rows,
        scanned_rows,
        took_ms: started.elapsed().as_millis() as u64,
        federation: None,
    })
}

/// 校验 SQL 中所有 `MATCH_TEXT(field, ...)` 调用的字段前提（设计 D2/D3、spec
/// text-match-functions）：字段必须存在且显式/兼容配置为 `full_text`，
/// 否则查询失败并指明该字段未配置全文索引。
///
/// 校验落在 schema 上下文层（本函数由 `run` / `explain` 在 rewrite 前调用）；`streams` 为
/// None（引擎未注入 stream repo）时跳过——与既有无 schema 上下文的行为一致。
pub(super) async fn validate_match_text_fields(
    streams: Option<&dyn StreamRepository>,
    req: &QueryRequest,
    stream: &str,
    stream_type: StreamTypeEnum,
    sql: &str,
) -> Result<()> {
    let fields = match_text_fields(sql);

    if fields.is_empty() {
        return Ok(());
    }

    let Some(streams) = streams else {
        return Ok(());
    };

    let Ok(definition) = streams.get(&req.org_id, stream, stream_type).await else {
        return Ok(());
    };

    for field in fields {
        let configured = definition.schema.fields.iter().any(|def| {
            def.name == field && def.effective_index_type() == StreamIndexType::FullText
        });

        if !configured {
            return Err(Error::invalid(format!(
                "MATCH_TEXT: field `{field}` has no full-text index configured; \
                 configure `index_type = full_text` on a string field to enable full-text search"
            )));
        }
    }

    Ok(())
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
