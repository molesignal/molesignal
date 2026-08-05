// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compactor：周期合并小 parquet + retention 扫描。
//!
//! - [`Compactor::sweep_one`]：对 `(org, stream, stream_type, date)` 内的"小文件"
//!   按 time_start 排序 + 贪心累计 < `target_mb` 的相邻组（≥2 个）合并：
//!   `parquet_reader.read_all` → `arrow::compute::concat_batches` → `parquet_writer.flush`
//!   → `parquet_file_meta_repo.replace(merged_ids, vec![new_meta])`。任一失败 → `object_store.delete`
//!   清新对象 + `compactor_failures_total{reason}` 计数；`delete` 自身失败仅 warn，
//!   下一轮 retention sweep 兜底。
//! - [`Compactor::retention_sweep`]：对单 stream 按有效 retention 把过期 ParquetFileMeta
//!   通过 `parquet_file_meta_repo.replace(ids, vec![])` mark deleted，再 `object_store.delete`。

use std::sync::{Arc, OnceLock};

use arrow::compute::concat_batches;
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use prometheus::{IntCounter, IntCounterVec};

use super::parquet::{
    reader::ParquetReader,
    writer::{ParquetWriter, is_downsampled_key},
};
use crate::{
    config::CompactorSettings,
    domain::{
        storage::{ParquetFileMeta, ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::{StreamDefinition, StreamType},
    },
    shared::{
        Error, Result,
        ids::Id,
        metrics::{register_int_counter, register_int_counter_vec},
        time::{TimeRange, TimestampMicros},
    },
};

mod cleanup;
mod datasets;
mod downsample;
mod partition;

use cleanup::delete_file_outputs;
use datasets::for_stream_type as dataset_kinds_for;
use partition::{build_groups, validate_group};

/// `compactor_failures_total{reason}`、`compactor_merged_groups_total`。
static FAILURES: OnceLock<IntCounterVec> = OnceLock::new();
static MERGED: OnceLock<IntCounter> = OnceLock::new();
static RETENTION_DELETED: OnceLock<IntCounter> = OnceLock::new();
static DOWNSAMPLED: OnceLock<IntCounter> = OnceLock::new();

fn failures() -> &'static IntCounterVec {
    FAILURES.get_or_init(|| {
        register_int_counter_vec(
            "compactor_failures_total",
            "compactor failures by reason",
            &["reason"],
        )
    })
}
fn merged() -> &'static IntCounter {
    MERGED.get_or_init(|| {
        register_int_counter(
            "compactor_merged_groups_total",
            "compactor groups merged successfully",
        )
    })
}
fn retention_deleted() -> &'static IntCounter {
    RETENTION_DELETED.get_or_init(|| {
        register_int_counter(
            "compactor_retention_deleted_total",
            "files marked deleted by retention sweep",
        )
    })
}
fn downsampled() -> &'static IntCounter {
    DOWNSAMPLED.get_or_init(|| {
        register_int_counter(
            "compactor_downsampled_groups_total",
            "date partitions downsampled successfully",
        )
    })
}

pub struct Compactor {
    parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
    reader: Arc<ParquetReader>,
    writer: Arc<ParquetWriter>,
    object_store: Arc<dyn ObjectStore>,
    settings: CompactorSettings,
    /// Tantivy result cache：compactor replace / 标删时，对被移除文件对应的
    /// 与被替换 Parquet 对应的 `.ttv` sidecar cache 主动失效。
    tantivy_result_cache: Option<Arc<crate::infra::caching::TantivyResultCache>>,
    /// Tantivy footer cache：失效语义同上。
    tantivy_footer_cache: Option<Arc<crate::infra::caching::TantivyFooterCache>>,
}

impl Compactor {
    pub fn new(
        parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
        reader: Arc<ParquetReader>,
        writer: Arc<ParquetWriter>,
        object_store: Arc<dyn ObjectStore>,
        settings: CompactorSettings,
    ) -> Self {
        Self {
            parquet_file_meta,
            reader,
            writer,
            object_store,
            settings,
            tantivy_result_cache: None,
            tantivy_footer_cache: None,
        }
    }

    pub fn with_tantivy_result_cache(
        mut self,
        cache: Arc<crate::infra::caching::TantivyResultCache>,
    ) -> Self {
        self.tantivy_result_cache = Some(cache);
        self
    }

    pub fn with_tantivy_footer_cache(
        mut self,
        cache: Arc<crate::infra::caching::TantivyFooterCache>,
    ) -> Self {
        self.tantivy_footer_cache = Some(cache);
        self
    }

    /// 收集 `object_keys` 对应的 tantivy index_object_keys 并同步失效两层缓存。
    /// 无 cache 注入时 no-op；invalidate 错误仅 warn，不阻塞合并主路径。
    async fn invalidate_tantivy_caches(&self, object_keys: &[String]) {
        if object_keys.is_empty() {
            return;
        }
        if self.tantivy_result_cache.is_none() && self.tantivy_footer_cache.is_none() {
            return;
        }
        // change `tantivy-puffin-migration`: key_for 现在返 Option（不规范 parquet
        // key → 无 sidecar）；过滤掉 None 的，剩下的全部按新 .ttv 命名 invalidate。
        let index_object_keys: Vec<String> = object_keys
            .iter()
            .filter_map(|k| crate::infra::search::tantivy_index::TantivyArchive::key_for(k))
            .collect();
        if let Some(rc) = self.tantivy_result_cache.as_ref() {
            rc.invalidate_index_object_keys(&index_object_keys).await;
        }
        if let Some(fc) = self.tantivy_footer_cache.as_ref() {
            fc.invalidate_index_object_keys(&index_object_keys).await;
        }
    }

    pub fn settings(&self) -> &CompactorSettings {
        &self.settings
    }

    /// 对 `(org, stream, stream_type)` 在 `date_range` 内的小文件做合并。
    /// 返回成功合并的 group 数。
    #[tracing::instrument(
        name = "worker.compactor",
        parent = None,
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.worker.name = "compactor",
            molesignal.stream.type = ?stream.stream_type
        )
    )]
    pub async fn sweep_one(
        &self,
        stream: &StreamDefinition,
        date_range: TimeRange,
    ) -> Result<usize> {
        let mut total = 0;
        for dataset_kind in dataset_kinds_for(stream.stream_type) {
            total += self
                .sweep_dataset(stream, *dataset_kind, date_range)
                .await?;
        }
        Ok(total)
    }

    async fn sweep_dataset(
        &self,
        stream: &StreamDefinition,
        dataset_kind: PhysicalDatasetKind,
        date_range: TimeRange,
    ) -> Result<usize> {
        let target_bytes = (self.settings.target_mb as u64).saturating_mul(1024 * 1024);
        let candidates = self
            .parquet_file_meta
            .find_dataset(
                &stream.org_id,
                &stream.name,
                stream.stream_type,
                dataset_kind,
                date_range,
            )
            .await?;

        // 过滤小文件 + 时间顺序。降采样产物（`.ds.parquet`）不卷入小文件合并：合并会换成
        // 普通 key、丢失标记，导致下轮被重复降采样。
        let smalls = candidates
            .into_iter()
            .filter(|f| {
                !f.deleted && f.size_bytes < target_bytes && !is_downsampled_key(&f.object_key)
            })
            .collect::<Vec<_>>();
        let (groups, invalid) = build_groups(smalls, target_bytes);
        for file in invalid {
            failures().with_label_values(&["cross_hour_file"]).inc();
            tracing::error!(
                object_key = %file.object_key,
                start = file.time_range.start.0,
                end = file.time_range.end.0,
                "compactor refused legacy file crossing a UTC hour"
            );
        }

        let mut merged_groups = 0usize;
        for group in groups {
            match self.merge_group(stream, &group).await {
                Ok(true) => {
                    merged_groups += 1;
                    merged().inc();
                }
                Ok(false) => {
                    // 本组含幽灵文件，已就地修复 parquet_file_meta，不计入合并计数。
                }
                Err(e) => {
                    tracing::warn!(
                        stream = %stream.name,
                        group_size = group.len(),
                        error = %e,
                        "compactor merge group failed; will retry next sweep"
                    );
                }
            }
        }
        Ok(merged_groups)
    }

    /// 真正合并一组：read_all → concat → flush → replace；失败时 delete 新对象。
    /// 返回 `Ok(true)` 表示完成一次实际合并；`Ok(false)` 表示本组含幽灵文件、
    /// 已就地把幽灵 parquet_file_meta 标删但没产生合并对象（caller 不应把它计入"已合并"计数）。
    #[tracing::instrument(
        name = "compactor.merge",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.compactor.file_count = group.len()
        )
    )]
    async fn merge_group(
        &self,
        stream: &StreamDefinition,
        group: &[ParquetFileMeta],
    ) -> Result<bool> {
        let dataset_kind = validate_group(group)?;
        // 路由：本组所有文件都属于同一 (org, stream)，store 单次解析。
        let store = self.object_store.clone();

        // 1. 读所有源 parquet → 拼一个 batch
        //    幽灵文件（parquet_file_meta 行在，对象已不在）会触发 NotFound：单独把这条
        //    parquet_file_meta 标删后跳过本组，避免一坏文件让整批永远 retry。
        let mut all_batches = Vec::new();
        let mut ghost_ids: Vec<Id> = Vec::new();
        for f in group {
            match self
                .reader
                .read_all_from_store(store.clone(), &f.object_key)
                .await
            {
                Ok(batches) => all_batches.extend(batches),
                Err(Error::NotFound(_)) => {
                    failures().with_label_values(&["ghost_file"]).inc();
                    tracing::warn!(
                        stream = %stream.name,
                        object_key = %f.object_key,
                        file_id = %f.id.0,
                        "compactor parquet_file_meta references missing object; marking deleted"
                    );
                    ghost_ids.push(f.id.clone());
                }
                Err(e) => {
                    // 把已知的幽灵文件也一并 mark deleted，避免下轮 sweep 再次卷入
                    if !ghost_ids.is_empty() {
                        let ghost_object_keys: Vec<String> = group
                            .iter()
                            .filter(|f| ghost_ids.iter().any(|gid| gid == &f.id))
                            .map(|f| f.object_key.clone())
                            .collect();
                        if let Err(re) = self.parquet_file_meta.mark_deleted(&ghost_ids).await {
                            failures().with_label_values(&["ghost_mark_deleted"]).inc();
                            tracing::warn!(
                                stream = %stream.name,
                                error = %re,
                                "failed to mark ghost parquet_file_meta deleted"
                            );
                        } else {
                            self.invalidate_tantivy_caches(&ghost_object_keys).await;
                        }
                    }
                    return Err(e);
                }
            }
        }

        // 把幽灵 parquet_file_meta 标删（事务）；放在读循环结束后统一一次提交。
        if !ghost_ids.is_empty() {
            let ghost_object_keys: Vec<String> = group
                .iter()
                .filter(|f| ghost_ids.iter().any(|gid| gid == &f.id))
                .map(|f| f.object_key.clone())
                .collect();
            if let Err(e) = self.parquet_file_meta.mark_deleted(&ghost_ids).await {
                failures().with_label_values(&["ghost_mark_deleted"]).inc();
                tracing::warn!(
                    stream = %stream.name,
                    error = %e,
                    "failed to mark ghost parquet_file_meta deleted"
                );
                return Err(e);
            }
            self.invalidate_tantivy_caches(&ghost_object_keys).await;
            // 本组有至少一个幽灵 → 剩余的文件即便都读到了也凑不成两个有效文件的合并，
            // 直接放弃本组；下一轮 sweep 会基于纯净的 parquet_file_meta 重新分组。
            return Ok(false);
        }

        if all_batches.is_empty() {
            return Err(Error::internal("compactor group produced 0 batches"));
        }
        let physical_stream =
            crate::infra::ingester::physical_schema::project(stream, dataset_kind);
        let schema = crate::infra::storage::arrow_schema::to_arrow(&physical_stream.schema);
        let aligned = all_batches
            .iter()
            .map(|batch| {
                crate::infra::storage::arrow_schema::align_batch_to_schema(batch, &schema)
                    .map_err(|error| Error::internal(format!("compactor align schema: {error}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let merged_batch = concat_batches(&schema, &aligned)
            .map_err(|e| Error::internal(format!("concat_batches: {e}")))?;

        // 2. 写新 parquet + 重建对应 Tantivy sidecar。
        let (new_meta, new_archive) = self
            .writer
            .flush_dataset_with_index_to_store(
                store.as_ref(),
                &physical_stream,
                dataset_kind,
                merged_batch,
            )
            .await?;
        let new_key = new_meta.object_key.clone();
        let new_index_key = new_archive.map(|archive| archive.object_key);

        // 3. parquet_file_meta replace（事务原子）
        let merged_ids: Vec<Id> = group.iter().map(|f| f.id.clone()).collect();
        let merged_object_keys: Vec<String> = group.iter().map(|f| f.object_key.clone()).collect();
        if let Err(e) = self
            .parquet_file_meta
            .replace(&merged_ids, vec![new_meta])
            .await
        {
            failures().with_label_values(&["replace"]).inc();
            // 清理已上传但未挂载的新对象
            if let Err(de) = store.delete(&Path::from(new_key.clone())).await {
                failures().with_label_values(&["cleanup_delete"]).inc();
                tracing::warn!(
                    object_key = %new_key,
                    error = %de,
                    "compactor cleanup delete failed; retention sweep will reclaim"
                );
            }
            if let Some(index_key) = new_index_key
                && let Err(delete_error) = store.delete(&Path::from(index_key.clone())).await
            {
                failures().with_label_values(&["cleanup_delete"]).inc();
                tracing::warn!(
                    object_key = %index_key,
                    error = %delete_error,
                    "compactor sidecar cleanup failed"
                );
            }
            return Err(e);
        }
        self.invalidate_tantivy_caches(&merged_object_keys).await;

        // 4. 删旧 object（best-effort：失败仅 warn）
        for f in group {
            delete_file_outputs(store.as_ref(), f).await;
        }
        Ok(true)
    }

    /// 按 stream retention 或全局 fallback 把过期 ParquetFileMeta 标删 + 删对象。
    /// 返回已删除的 file 数。
    pub async fn retention_sweep(&self, stream: &StreamDefinition) -> Result<usize> {
        let now_us = TimestampMicros::now().0;
        let retention_days = stream.effective_retention_days(self.settings.retention_days);
        let cutoff = now_us - (retention_days as i64) * 24 * 3600 * 1_000_000;
        // profiles 归档 blob 是 object store 旁路对象、不在 parquet_file_meta 内，需在 retention
        // 时单独清理（storage spec：到期同时清理 parquet 元数据与归档 blob）。放在
        // parquet_file_meta sweep 的早退之前，确保即使本轮无过期 parquet 也会清理过期归档。
        if stream.stream_type == StreamType::Profiles {
            match crate::infra::profiles::sweep_expired_archives(
                &self.object_store,
                &stream.org_id,
                cutoff,
            )
            .await
            {
                Ok(n) if n > 0 => {
                    tracing::debug!(org_id = %stream.org_id.0, removed = n, "profiles archive retention sweep")
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(org_id = %stream.org_id.0, error = %e, "profiles archive retention sweep failed")
                }
            }
        }
        // 下界用 epoch 0：retention sweep 只关心 end ≤ cutoff 的文件，下界给任何
        // ≤ cutoff 的合理值都可以。曾经用 i64::MIN，但 PgParquetFileMetaRepository::find
        // 在启用 cold-tier dump 时会把起点转成 chrono::NaiveDate 绑给 PG DATE 列，
        // i64::MIN 越界会兜底成 NaiveDate::MIN（-262143-01-01），超出 PG DATE
        // 范围 → "date out of range"。
        let range = TimeRange::new(TimestampMicros(0), TimestampMicros(cutoff));
        let mut candidates = Vec::new();
        for dataset_kind in dataset_kinds_for(stream.stream_type) {
            candidates.extend(
                self.parquet_file_meta
                    .find_dataset(
                        &stream.org_id,
                        &stream.name,
                        stream.stream_type,
                        *dataset_kind,
                        range,
                    )
                    .await?,
            );
        }
        let to_delete: Vec<ParquetFileMeta> = candidates
            .into_iter()
            .filter(|f| f.time_range.end.0 <= cutoff && !f.deleted)
            .collect();
        if to_delete.is_empty() {
            return Ok(0);
        }
        let ids: Vec<Id> = to_delete.iter().map(|f| f.id.clone()).collect();
        let retention_object_keys: Vec<String> =
            to_delete.iter().map(|f| f.object_key.clone()).collect();
        // 幂等标删：并发 sweep 已经标掉一部分不算错误，实际标删数不影响后续对象清理。
        self.parquet_file_meta.mark_deleted(&ids).await?;
        self.invalidate_tantivy_caches(&retention_object_keys).await;
        let n = to_delete.len();
        // 用 stream.org_id 解析 store；retention sweep 内所有文件属同一 org。
        let store = self.object_store.clone();
        for f in to_delete {
            delete_file_outputs(store.as_ref(), &f).await;
        }
        retention_deleted().inc_by(n as u64);
        Ok(n)
    }

    /// 把 metrics 流中早于 `downsample_after_days` 的数据降采样到 `downsample_interval_secs`
    /// 时间桶（按 UTC 小时分区分组，每组聚合成一个 `.ds.parquet` 产物，replace 掉该组旧文件）。
    /// 关闭（after_days/interval 为 0）或非 metrics 流 → no-op。返回处理的分区组数。
    ///
    /// 收敛：某日分区已只剩单个 `.ds` 文件、无原始文件 → 跳过；故稳定后不再重复处理。
    pub async fn downsample_sweep(&self, stream: &StreamDefinition) -> Result<usize> {
        downsample::sweep(self, stream).await
    }
}

// ---- 让 Compactor 也接受任意 StreamType（spec 6.2 `(org, stream, stream_type, date)`） ----

impl Compactor {
    pub async fn sweep_one_by_key(
        &self,
        org_id: &Id,
        stream_name: &str,
        stream_type: StreamType,
        date_range: TimeRange,
        // 临时用 stream_def 复用：caller 通常已有 StreamDefinition 在手
        stream_def_lookup: impl FnOnce() -> Result<StreamDefinition>,
    ) -> Result<usize> {
        let _ = (org_id, stream_name, stream_type);
        let def = stream_def_lookup()?;
        self.sweep_one(&def, date_range).await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use arrow::array::{Int64Array, RecordBatch, TimestampMicrosecondArray};
    use object_store::local::LocalFileSystem;
    use serde_json::Map;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Retention, Schema},
        infra::storage::arrow_schema::to_arrow,
    };

    // ---- 简易 in-mem ParquetFileMetaRepository ----
    struct InMemParquetFileMeta {
        inner: Mutex<HashMap<String, ParquetFileMeta>>, // key = id
    }
    impl Default for InMemParquetFileMeta {
        fn default() -> Self {
            Self {
                inner: Mutex::new(HashMap::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl ParquetFileMetaRepository for InMemParquetFileMeta {
        async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
            self.inner.lock().unwrap().insert(file.id.0.clone(), file);
            Ok(())
        }
        async fn find(
            &self,
            org_id: &Id,
            stream: &str,
            st: StreamType,
            range: TimeRange,
        ) -> Result<Vec<ParquetFileMeta>> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .values()
                .filter(|f| {
                    !f.deleted
                        && f.dataset_kind == PhysicalDatasetKind::Raw
                        && &f.org_id == org_id
                        && f.stream == stream
                        && f.stream_type == st
                        && f.time_range.end.0 >= range.start.0
                        && f.time_range.start.0 <= range.end.0
                })
                .cloned()
                .collect())
        }
        async fn find_dataset(
            &self,
            org_id: &Id,
            stream: &str,
            st: StreamType,
            dataset_kind: PhysicalDatasetKind,
            range: TimeRange,
        ) -> Result<Vec<ParquetFileMeta>> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .values()
                .filter(|file| {
                    !file.deleted
                        && file.dataset_kind == dataset_kind
                        && &file.org_id == org_id
                        && file.stream == stream
                        && file.stream_type == st
                        && file.time_range.end.0 >= range.start.0
                        && file.time_range.start.0 < range.end.0
                })
                .cloned()
                .collect())
        }
        async fn replace(&self, merged_ids: &[Id], new_files: Vec<ParquetFileMeta>) -> Result<()> {
            let mut guard = self.inner.lock().unwrap();
            // 与 PG 实装同语义：任一源文件已不存活就整体放弃，不写新文件。
            let live = merged_ids
                .iter()
                .filter(|id| guard.get(&id.0).is_some_and(|f| !f.deleted))
                .count();
            if live != merged_ids.len() {
                return Err(Error::conflict(format!(
                    "parquet_file_meta replace: {live} of {} source files were live",
                    merged_ids.len()
                )));
            }
            for id in merged_ids {
                if let Some(f) = guard.get_mut(&id.0) {
                    f.deleted = true;
                }
            }
            for f in new_files {
                guard.insert(f.id.0.clone(), f);
            }
            Ok(())
        }
        async fn mark_deleted(&self, ids: &[Id]) -> Result<usize> {
            let mut guard = self.inner.lock().unwrap();
            let mut marked = 0;
            for id in ids {
                if let Some(f) = guard.get_mut(&id.0)
                    && !f.deleted
                {
                    f.deleted = true;
                    marked += 1;
                }
            }
            Ok(marked)
        }
    }

    fn sample_stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-x"),
            name: "app".into(),
            stream_type: StreamType::Logs,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "val".into(),
                    data_type: FieldType::Int64,
                    nullable: false,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn small_batch(start_us: i64, n: usize) -> RecordBatch {
        let schema = to_arrow(&sample_stream().schema);
        let ts = TimestampMicrosecondArray::from(
            (0..n).map(|i| start_us + i as i64).collect::<Vec<_>>(),
        )
        .with_timezone("UTC");
        let val = Int64Array::from((0..n).map(|i| i as i64).collect::<Vec<_>>());
        RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(val)]).unwrap()
    }

    async fn write_small(
        writer: &ParquetWriter,
        repo: &InMemParquetFileMeta,
        stream: &StreamDefinition,
        start_us: i64,
        rows: usize,
    ) -> ParquetFileMeta {
        let batch = small_batch(start_us, rows);
        let mut meta = writer.flush(stream, batch).await.unwrap();
        // 强行把 size_bytes 设小（绕过 target_mb 默认 512MiB 的阈值场景）
        meta.size_bytes = 1024;
        repo.insert(meta.clone()).await.unwrap();
        meta
    }

    #[tokio::test]
    async fn sweep_invalidates_tantivy_caches_for_merged_archives() {
        use crate::{
            config::{TantivyFooterCacheSettings, TantivyResultCacheSettings},
            infra::caching::{TantivyFooterCache, TantivyResultCache, TantivyResultKey},
        };

        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();
        let mut metas = Vec::new();
        for i in 0..3 {
            metas.push(write_small(&writer, &repo, &stream, 1_000_000 + i * 1000, 5).await);
        }
        let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        }));
        let footer_cache = Arc::new(TantivyFooterCache::new(&TantivyFooterCacheSettings {
            capacity: 100,
            ttl_secs: 60,
        }));
        // 模拟 pruner 早些时候已经把这些 archive 缓存进了 result cache。
        for meta in &metas {
            let Some(index_object_key) =
                crate::infra::search::tantivy_index::TantivyArchive::key_for(&meta.object_key)
            else {
                continue;
            };
            result_cache
                .insert(TantivyResultKey::new(&index_object_key, "f", "t"), 1)
                .await;
        }
        // 与 footer cache：用占位 footer 入 cache（只关心 invalidate 行为）。
        for meta in &metas {
            let Some(index_object_key) =
                crate::infra::search::tantivy_index::TantivyArchive::key_for(&meta.object_key)
            else {
                continue;
            };
            let mut sb = tantivy::schema::Schema::builder();
            sb.add_text_field("f", tantivy::schema::TEXT);
            let puffin_meta = crate::tantivy::PuffinMeta {
                blobs: Vec::new(),
                properties: Default::default(),
            };
            footer_cache
                .insert(
                    index_object_key,
                    Arc::new(crate::infra::search::tantivy_index::TantivyFooter {
                        puffin_meta: Arc::new(puffin_meta),
                        footer_payload_bytes: bytes::Bytes::from_static(b"x"),
                        schema: sb.build(),
                        atomic_files: Arc::new(Default::default()),
                        object_size: 0,
                    }),
                )
                .await;
        }

        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            CompactorSettings::default(),
        )
        .with_tantivy_result_cache(result_cache.clone())
        .with_tantivy_footer_cache(footer_cache.clone());
        compactor
            .sweep_one(
                &stream,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();

        // moka invalidate_entries_if 是 async best-effort，挪动一下 housekeeping。
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        for meta in &metas {
            let Some(index_object_key) =
                crate::infra::search::tantivy_index::TantivyArchive::key_for(&meta.object_key)
            else {
                continue;
            };
            assert!(
                result_cache
                    .get(&TantivyResultKey::new(&index_object_key, "f", "t"))
                    .await
                    .is_none(),
                "result cache entry for merged archive must be invalidated"
            );
            assert!(
                footer_cache.get(&index_object_key).await.is_none(),
                "footer cache entry for merged archive must be invalidated"
            );
        }
    }

    #[tokio::test]
    async fn sweep_merges_consecutive_small_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();
        for i in 0..5 {
            let _ = write_small(&writer, &repo, &stream, 1_000_000 + i * 1000, 10).await;
        }
        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            CompactorSettings::default(),
        );
        let merged_groups = compactor
            .sweep_one(
                &stream,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        assert_eq!(merged_groups, 1, "5 small files → 1 merged group");

        let active = repo
            .find(
                &stream.org_id,
                &stream.name,
                stream.stream_type,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        // 5 旧 marked deleted + 1 new active
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].rows, 50, "merged batch contains all rows");
    }

    #[tokio::test]
    async fn sweep_single_file_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();
        let _ = write_small(&writer, &repo, &stream, 1_000_000, 10).await;

        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            CompactorSettings::default(),
        );
        let merged_groups = compactor
            .sweep_one(
                &stream,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        assert_eq!(merged_groups, 0);
    }

    #[tokio::test]
    async fn replace_rejects_group_whose_file_was_concurrently_compacted() {
        // 多副本部署：另一个实例已经把本组里的 b 合并掉并标删，而本实例还拿着 sweep
        // 开始时的旧快照。若 replace 放行，本实例的产物会与对方的产物同时存活，
        // 两份都含 b 的行 —— 查询侧行数翻倍。
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();

        let a = write_small(&writer, &repo, &stream, 1_000_000, 5).await;
        let b = write_small(&writer, &repo, &stream, 2_000_000, 5).await;
        assert_eq!(
            repo.mark_deleted(std::slice::from_ref(&b.id))
                .await
                .unwrap(),
            1
        );

        let mut new_meta = a.clone();
        new_meta.id = Id::new();
        new_meta.object_key = "merged-by-loser.parquet".into();
        let err = repo
            .replace(&[a.id.clone(), b.id.clone()], vec![new_meta.clone()])
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "expected conflict, got {err:?}"
        );

        let live = repo
            .find(
                &stream.org_id,
                &stream.name,
                stream.stream_type,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        let ids: Vec<&str> = live.iter().map(|f| f.id.0.as_str()).collect();
        assert!(
            ids.contains(&a.id.0.as_str()),
            "a 不应被冲突的 replace 标删"
        );
        assert!(
            !ids.contains(&new_meta.id.0.as_str()),
            "冲突时新文件不得落库，否则就是重复数据"
        );
    }

    #[tokio::test]
    async fn mark_deleted_is_idempotent_and_reports_actual_count() {
        // 幽灵清理与 retention 是幂等路径：并发实例已标删过一部分时不能报错，
        // 只如实返回本次实际标删数。
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();

        let a = write_small(&writer, &repo, &stream, 1_000_000, 5).await;
        let b = write_small(&writer, &repo, &stream, 2_000_000, 5).await;
        let ids = vec![a.id.clone(), b.id.clone()];

        assert_eq!(repo.mark_deleted(&ids).await.unwrap(), 2);
        assert_eq!(
            repo.mark_deleted(&ids).await.unwrap(),
            0,
            "重复标删返 0 而非报错"
        );
        assert_eq!(repo.mark_deleted(&[]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn replace_failure_cleans_up_new_object() {
        // 让 replace 失败：用 panicking repo
        struct FailRepo {
            base: InMemParquetFileMeta,
        }
        #[async_trait::async_trait]
        impl ParquetFileMetaRepository for FailRepo {
            async fn insert(&self, f: ParquetFileMeta) -> Result<()> {
                self.base.insert(f).await
            }
            async fn find(
                &self,
                org: &Id,
                stream: &str,
                st: StreamType,
                r: TimeRange,
            ) -> Result<Vec<ParquetFileMeta>> {
                self.base.find(org, stream, st, r).await
            }
            async fn replace(&self, _ids: &[Id], _new: Vec<ParquetFileMeta>) -> Result<()> {
                Err(Error::internal("simulated replace failure"))
            }
            async fn mark_deleted(&self, ids: &[Id]) -> Result<usize> {
                self.base.mark_deleted(ids).await
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let inner = InMemParquetFileMeta::default();
        let stream = sample_stream();
        // 直接写 2 个文件入 inner
        let m1 = {
            let batch = small_batch(1_000, 5);
            let mut m = writer.flush(&stream, batch).await.unwrap();
            m.size_bytes = 1024;
            inner.insert(m.clone()).await.unwrap();
            m
        };
        let m2 = {
            let batch = small_batch(2_000, 5);
            let mut m = writer.flush(&stream, batch).await.unwrap();
            m.size_bytes = 1024;
            inner.insert(m.clone()).await.unwrap();
            m
        };
        let repo: Arc<FailRepo> = Arc::new(FailRepo { base: inner });
        let compactor = Compactor::new(
            repo as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store.clone(),
            CompactorSettings::default(),
        );
        let merged_groups = compactor
            .sweep_one(
                &stream,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        assert_eq!(
            merged_groups, 0,
            "replace failed → group not counted as merged"
        );
        // 旧两个对象仍在；新对象应被清理 — 用 ObjectStore::list 数 .parquet 后缀对象
        use futures::TryStreamExt;
        let mut stream_list = store.list(None);
        let mut count = 0usize;
        while let Some(obj) = stream_list.try_next().await.unwrap() {
            if obj.location.as_ref().ends_with(".parquet") {
                count += 1;
            }
        }
        assert_eq!(
            count, 2,
            "only original 2 parquet objects survive; merged-then-cleaned"
        );
        let _ = (m1, m2);
    }

    // 避免 unused import 告警
    fn _silence() {
        let _ = Map::<String, serde_json::Value>::new();
    }

    /// 降采样：metrics 流中早于阈值的多个原始文件 → 聚合成单个 `.ds` 文件；二次 sweep 收敛跳过。
    #[tokio::test]
    async fn downsample_sweep_collapses_old_metrics_and_converges() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let mut stream = sample_stream();
        stream.stream_type = StreamType::Metrics;
        // 4 个老文件（epoch 附近，远早于 cutoff），各 10 行、同一小时桶、无维度列。
        for i in 0..4 {
            let _ = write_small(&writer, &repo, &stream, 1_000_000 + i * 1000, 10).await;
        }
        let settings = CompactorSettings {
            downsample_after_days: 1,
            downsample_interval_secs: 3600,
            ..CompactorSettings::default()
        };
        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            settings,
        );
        let processed = compactor.downsample_sweep(&stream).await.unwrap();
        assert_eq!(processed, 1, "one date partition downsampled");

        let full = TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX));
        let active = repo
            .find_dataset(
                &stream.org_id,
                &stream.name,
                stream.stream_type,
                PhysicalDatasetKind::MetricRollup,
                full,
            )
            .await
            .unwrap();
        assert_eq!(
            active.len(),
            1,
            "4 raw files collapsed to 1 downsampled file"
        );
        assert!(
            is_downsampled_key(&active[0].object_key),
            "output carries the .ds marker"
        );
        // 同一小时桶、无维度 → 1 行（全部 val 取 avg）。
        assert_eq!(
            active[0].rows, 1,
            "all rows fall into a single hourly bucket"
        );

        // 收敛：二次 sweep 不再处理（单个 .ds 文件、无原始）。
        let again = compactor.downsample_sweep(&stream).await.unwrap();
        assert_eq!(
            again, 0,
            "converged: downsampled partition is not reprocessed"
        );
    }

    /// 非 metrics 流 / 关闭时 downsample_sweep 是 no-op。
    #[tokio::test]
    async fn downsample_sweep_noop_when_disabled_or_non_metrics() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream(); // Logs
        for i in 0..3 {
            let _ = write_small(&writer, &repo, &stream, 1_000_000 + i * 1000, 10).await;
        }
        // 开启降采样但流是 Logs → no-op。
        let settings = CompactorSettings {
            downsample_after_days: 1,
            downsample_interval_secs: 3600,
            ..CompactorSettings::default()
        };
        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            settings,
        );
        assert_eq!(compactor.downsample_sweep(&stream).await.unwrap(), 0);
    }

    /// 幽灵文件兜底：parquet_file_meta 行存在但对象不在 → 整组应被跳过且幽灵 id 被标删，
    /// 而不是每次 sweep 都返回错误。
    #[tokio::test]
    async fn sweep_skips_ghost_files_and_marks_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = Arc::new(ParquetWriter::new(store.clone()));
        let reader = Arc::new(ParquetReader::new(store.clone()));
        let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
        let stream = sample_stream();
        // 写两个文件，然后把对象从 store 里删掉，制造幽灵
        let m1 = write_small(&writer, &repo, &stream, 1_000_000, 10).await;
        let m2 = write_small(&writer, &repo, &stream, 1_001_000, 10).await;
        store
            .delete(&Path::from(m1.object_key.clone()))
            .await
            .unwrap();
        store
            .delete(&Path::from(m2.object_key.clone()))
            .await
            .unwrap();

        let compactor = Compactor::new(
            repo.clone() as Arc<dyn ParquetFileMetaRepository>,
            reader,
            writer,
            store,
            CompactorSettings::default(),
        );
        // 第一次 sweep 应该 ok（无 panic / Err）但不合并任何 group
        let merged = compactor
            .sweep_one(
                &stream,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        assert_eq!(merged, 0, "ghost-only group is not counted as merged");

        // 关键：两条幽灵 parquet_file_meta 应被标删；下一轮 sweep 不会再卷入
        let active = repo
            .find(
                &stream.org_id,
                &stream.name,
                stream.stream_type,
                TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
            )
            .await
            .unwrap();
        assert!(
            active.is_empty(),
            "all ghost parquet_file_meta rows must be marked deleted"
        );
    }
}
