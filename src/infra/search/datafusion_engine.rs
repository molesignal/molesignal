// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DataFusion-backed SQL `QueryEngine`。
//!
//! 当前实现直接把 ParquetFileMeta/Tantivy 裁剪后的对象清单交给 `ParquetExec`：
//!
//! 1. 解析 `QueryRequest` 拿到目标 stream（由 `StreamHint` 强制提供）；
//! 2. 经 `ParquetFileMetaRepository::find` 拉时间窗内的 parquet_file_meta 列表（含分区裁剪）；
//! 3. 注册显式候选文件的 [`PrunedParquetTable`]；
//! 4. projection / filter / limit 由 DataFusion 下推到 parquet 扫描；
//! 5. 仅最终结果物化为 `QueryResult`。

use std::sync::Arc;

use async_trait::async_trait;
use datafusion::{common::TableReference, datasource::MemTable};
use object_store::ObjectStore;

use crate::{
    domain::{
        masking::Masker,
        query::{QueryEngine, QueryRequest, QueryResult, StreamHint},
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::{StreamRepository, StreamType as StreamTypeEnum},
    },
    infra::{
        persistence::repositories::{
            log_patterns::LogPatternRepository, regex_patterns::RegexPatternRepository,
        },
        query::{
            parser::{extract_referenced_tables, parse_sample_hint},
            planner::ensure_stream_in_org,
            tantivy_pruner::{TantivyPruner, extract_match_predicates},
            udfs::{build_extract_pattern_udf, build_mask_udf, compile_patterns},
        },
    },
    shared::{Error, Result, ids::Id},
};

mod dataset;
mod execute;
mod result;

pub struct DataFusionEngine {
    files: Arc<dyn ParquetFileMetaRepository>,
    object_store: Arc<dyn ObjectStore>,
    tantivy_pruner: Option<Arc<TantivyPruner>>,
    streams: Option<Arc<dyn StreamRepository>>,
    /// 可选 log_patterns repo；非空时 SQL 可用 `extract_pattern(message)`。
    log_patterns: Option<Arc<dyn LogPatternRepository>>,
    /// 可选 regex_patterns repo；非空时 SQL 含 `mask(col)` 按 org 加载脱敏规则注册 UDF。
    regex_patterns: Option<Arc<dyn RegexPatternRepository>>,
    /// 结果行安全上限（`ResultRowGuard` AnalyzerRule）；0 = 关闭。
    max_result_rows: usize,
    /// 字段加密 DEK 服务；非空时 SQL 可用 `decrypt(col)`（执行期按 org 预载 DEK 还原密文）。
    field_keys: Option<Arc<crate::infra::cipher::FieldKeyService>>,
}

impl DataFusionEngine {
    pub fn new(
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            files,
            object_store,
            tantivy_pruner: None,
            streams: None,
            log_patterns: None,
            regex_patterns: None,
            max_result_rows: 0,
            field_keys: None,
        }
    }

    /// 结果行安全上限（`ResultRowGuard`）；0 = 关闭。
    pub fn with_max_result_rows(mut self, max_rows: usize) -> Self {
        self.max_result_rows = max_rows;
        self
    }

    pub fn with_tantivy_pruner(mut self, pruner: Arc<TantivyPruner>) -> Self {
        self.tantivy_pruner = Some(pruner);
        self
    }

    /// 注入 StreamRepository → execute 前会调 [`ensure_stream_in_org`] 校验 streamHint
    /// 在当前 org 下存在。
    pub fn with_streams(mut self, streams: Arc<dyn StreamRepository>) -> Self {
        self.streams = Some(streams);
        self
    }

    pub fn with_log_patterns(mut self, patterns: Arc<dyn LogPatternRepository>) -> Self {
        self.log_patterns = Some(patterns);
        self
    }

    /// 注入 regex_patterns repo；非空时 SQL `mask(col)` 可用（按 org 加载脱敏规则）。
    pub fn with_regex_patterns(mut self, patterns: Arc<dyn RegexPatternRepository>) -> Self {
        self.regex_patterns = Some(patterns);
        self
    }

    /// 注入字段加密 DEK 服务；非空时 SQL `decrypt(col)` 可用。
    pub fn with_field_keys(mut self, svc: Arc<crate::infra::cipher::FieldKeyService>) -> Self {
        self.field_keys = Some(svc);
        self
    }

    /// 暴露 DEK 服务给包装层（distributed coordinator 复用本地引擎的服务注册 `decrypt`）。
    pub fn field_keys(&self) -> Option<&Arc<crate::infra::cipher::FieldKeyService>> {
        self.field_keys.as_ref()
    }

    /// 暴露 regex_patterns repo 给包装层（distributed coordinator 复用本地引擎的 repo 注册 `mask`）。
    pub fn regex_patterns(&self) -> Option<&Arc<dyn RegexPatternRepository>> {
        self.regex_patterns.as_ref()
    }

    /// 暴露 log_patterns repo 给包装层（distributed coordinator 复用本地引擎的 repo 注册 `extract_pattern`）。
    pub fn log_patterns(&self) -> Option<&Arc<dyn LogPatternRepository>> {
        self.log_patterns.as_ref()
    }

    /// 供分布式 coordinator 复用同一份归属校验：它自建 `SessionContext` 跑 final SQL，
    /// 不经过本引擎的 `execute`，拿不到这里的 `ensure_stream_in_org`。
    pub fn streams(&self) -> Option<&Arc<dyn StreamRepository>> {
        self.streams.as_ref()
    }
}

#[async_trait]
impl QueryEngine for DataFusionEngine {
    #[tracing::instrument(
        name = "query.datafusion",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.engine = "datafusion")
    )]
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        execute::run(self, req, None).await
    }

    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<QueryResult> {
        execute::run(self, req, Some(dataset_kind)).await
    }

    /// search inspector：注册 schema-only 空表后规划，返回优化后逻辑计划文本（不读数据）。
    async fn explain(&self, req: QueryRequest) -> Result<String> {
        let StreamHint { name, stream_type } = req.stream.clone().ok_or_else(|| {
            Error::invalid("query.stream hint is required; caller must name the target table")
        })?;
        if let Some(streams) = &self.streams {
            ensure_stream_in_org(streams.as_ref(), &req.org_id, &name, stream_type).await?;
        }
        let (_sample, statement) = parse_sample_hint(&req.statement);
        let (_preds, rewritten_sql) = extract_match_predicates(&statement);

        let ctx = crate::infra::query::analyzer::session_context_with_guard(self.max_result_rows);
        ctx.register_udaf(crate::infra::query::udafs::approx_topk_udf());
        if let Some(svc) = &self.field_keys {
            let keys = svc.decrypt_map(&req.org_id).await?;
            ctx.register_udf(crate::infra::query::udfs::build_decrypt_udf(keys));
        }
        if rewritten_sql.contains("mask(")
            && let Some(repo) = &self.regex_patterns
        {
            let pats = repo.list(&req.org_id).await.unwrap_or_default();
            let masker = Masker::compile(pats.into_iter().map(|p| (p.pattern, p.replacement)));
            ctx.register_udf(build_mask_udf(Arc::new(masker)));
        }
        if rewritten_sql.contains("extract_pattern(")
            && let Some(repo) = &self.log_patterns
        {
            let pats = repo.list(&req.org_id).await.unwrap_or_default();
            let rows: Vec<(String, String, i32)> = pats
                .into_iter()
                .map(|p| (p.regex, p.category, p.priority))
                .collect();
            ctx.register_udf(build_extract_pattern_udf(compile_patterns(rows)));
        }

        // 注册 schema-only 空表（仅规划，不读 parquet）。
        let Some(streams) = &self.streams else {
            return Err(Error::invalid("explain requires a stream catalog"));
        };
        let mut tables: Vec<(String, StreamTypeEnum)> = vec![(name.clone(), stream_type)];
        for tref in extract_referenced_tables(&rewritten_sql).unwrap_or_default() {
            if tref.name == name {
                continue;
            }
            if let Some(st) = resolve_stream_type(streams.as_ref(), &req.org_id, &tref.name).await {
                // 与 execute 一致：引用到不可查询的 stream 时明确拒绝（plan 阶段也不放行）。
                ensure_stream_in_org(streams.as_ref(), &req.org_id, &tref.name, st).await?;
                tables.push((tref.name, st));
            }
        }
        for (tname, st) in &tables {
            let Some(def) = streams.get(&req.org_id, tname, *st).await.ok() else {
                continue;
            };
            let schema = crate::infra::storage::arrow_schema::to_arrow(&def.schema);
            let mem = MemTable::try_new(schema, vec![vec![]])
                .map_err(|e| Error::internal(format!("explain memtable: {e}")))?;
            ctx.register_table(TableReference::bare(tname.clone()), Arc::new(mem))
                .map_err(|e| Error::internal(format!("explain register: {e}")))?;
        }

        let plan = ctx
            .sql(&rewritten_sql)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, sql = %rewritten_sql, "explain planning failed");
                Error::invalid("query could not be planned: check the SQL syntax and that every referenced field exists in the stream")
            })?
            .into_optimized_plan()
            .map_err(|e| Error::internal(format!("optimize plan: {e}")))?;
        Ok(format!("{plan}"))
    }
}

/// 查 `(org_id, name)` 在哪个 StreamType 下存在：依次试 Logs / Metrics / Traces / Extend。
/// 都不存在返 None，JOIN planner 会把该表跳过（DataFusion 之后报 "table not found"）。
async fn resolve_stream_type(
    streams: &dyn StreamRepository,
    org_id: &Id,
    name: &str,
) -> Option<StreamTypeEnum> {
    for st in [
        StreamTypeEnum::Logs,
        StreamTypeEnum::Metrics,
        StreamTypeEnum::Traces,
        StreamTypeEnum::Extend,
    ] {
        if streams.get(org_id, name, st).await.is_ok() {
            return Some(st);
        }
    }
    None
}
