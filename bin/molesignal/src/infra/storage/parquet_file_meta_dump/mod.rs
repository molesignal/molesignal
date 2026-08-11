// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ParquetFileMeta dump 服务（infra 侧 SQL-heavy 实现）。
//!
//! bootstrap 的 `parquet_file_meta_dumper` worker 调用本服务的 `run_tick`；本模块负责：
//! 1. 扫主表，发现 cold partition（按 daily / hourly 粒度 GROUP BY）。
//! 2. 对每个 partition：拿 PG advisory transactional lock 防并发；持锁内 SELECT 行 →
//!    serialize columnar parquet → PUT 对象 → tx { INSERT dump + INSERT stats +
//!    DELETE 主表 }。
//! 3. 指标计数（partitions_written / rows_written / partitions_skipped{empty|locked|error}）。
//!
//! 失败语义（与现行 spec 一致）：
//! - upload 失败 → 跳过，下一 tick 重试，`skipped{reason="error"}+1`
//! - tx 失败   → 同上；对象成为孤儿，下一 tick 覆盖
//! - lock 拿不到 → `skipped{reason="locked"}+1`，本 tick 不动该 partition
//! - 任何成功都满足 (a) PUT 在 DB mutation 前 (b) advisory lock 持有 (c) INSERT
//!   dump + INSERT stats + DELETE 主表 在同一 tx 内。
//!
//! `delete_by_time_range`（change `parquet-file-meta-dump-columnar`）：retention/合规
//! 删除走 dump 路径的精确粒度入口；按 design D5 走 "保留行重写 + 老 dump
//! mark deleted" 流程。

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use object_store::ObjectStore;
use prometheus::{IntCounter, IntCounterVec};
use sqlx::{PgPool, Row};

use self::writer::{DumpAggregate, dump_object_key, put_dump, rewrite_object_key, serialize_dump};
use crate::{
    config::ParquetFileMetaDumpSettings,
    domain::{
        storage::{
            ParquetFileMetaDumpRow, ParquetFileMetaDumpStats, PartitionLevel, PhysicalDatasetKind,
        },
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        metrics::{register_int_counter, register_int_counter_vec},
        time::{TimeRange, TimestampMicros},
    },
};

const DAY_MICROS: i64 = 24 * 3600 * 1_000_000;
const HOUR_MICROS: i64 = 3600 * 1_000_000;

pub mod reader;
pub mod writer;

pub struct ParquetFileMetaDumpService {
    pool: PgPool,
    object_store: Arc<dyn ObjectStore>,
    settings: ParquetFileMetaDumpSettings,
    /// 进程内 dump 缓存句柄；service 写入 / 删除 / 重写 dump 时同步失效对应
    /// `(org, stream, stream_type, partition_level, partition_key)` entry。
    /// `None` → invalidate 全部跳过（TTL 兜底），用于 wire 未注入或测试场景。
    dump_cache: Option<Arc<crate::infra::caching::ParquetFileMetaDumpCache>>,
}

#[derive(Debug, Default, Clone)]
pub struct TickStats {
    pub partitions_processed: u32,
    pub partitions_skipped: u32,
    pub rows_dumped: u64,
}

#[derive(Debug, Default, Clone)]
pub struct DeleteStats {
    pub partitions_rewritten: u32,
    pub partitions_dropped: u32,
    pub rows_removed: u64,
}

impl ParquetFileMetaDumpService {
    pub fn new(
        pool: PgPool,
        object_store: Arc<dyn ObjectStore>,
        settings: ParquetFileMetaDumpSettings,
    ) -> Self {
        Self {
            pool,
            object_store,
            settings,
            dump_cache: None,
        }
    }

    /// 注入 dump cache 让 dump 写入 / 删除路径同步失效进程内缓存
    /// （spec scenario `caching/ParquetFileMeta Dump In-Process Cache` 的 invalidate 路径）。
    pub fn with_dump_cache(
        mut self,
        cache: Arc<crate::infra::caching::ParquetFileMetaDumpCache>,
    ) -> Self {
        self.dump_cache = Some(cache);
        self
    }

    /// 失效一个 `(org, stream, stream_type, partition_level, partition_key)` 入口。
    /// 用在 dump_one_partition / delete_by_time_range 的 tx commit 后。
    async fn invalidate_cache(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        partition_level: PartitionLevel,
        partition_key: &str,
    ) {
        if let Some(cache) = self.dump_cache.as_ref() {
            cache
                .invalidate_partition(
                    org_id,
                    stream,
                    stream_type,
                    dataset_kind,
                    partition_level,
                    partition_key,
                )
                .await;
        }
    }

    pub fn settings(&self) -> &ParquetFileMetaDumpSettings {
        &self.settings
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.settings.interval_secs.max(1)))
    }

    /// 对象删除 sweep（change `parquet-file-meta-dump-columnar` task 8.x）。
    ///
    /// 消费 [`ParquetFileMetaDumpRepository::pending_object_deletes`]（`deleted = TRUE` 但 parquet
    /// 仍在 object_store 的 dump 行）：删掉对象后硬删该行。返回成功清理的对象数。
    ///
    /// 不实装时，冷分区被 `mark_deleted` 后旧 `dump.parquet` 永不删除，object_store
    /// 上的空间随重写次数累积泄漏（成本/容量问题）。功能性查询不受影响，故独立成 sweep。
    #[tracing::instrument(
        name = "worker.parquet_file_meta_delete",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "parquet_file_meta_delete")
    )]
    pub async fn run_object_delete_sweep(&self, limit: u32) -> Result<u64> {
        // ObjectStoreExt 提供 `Arc<dyn ObjectStore>` 上 dyn-可派发的 delete（base trait 的
        // RPITIT 版本不可 dyn 调用）；与 compactor.rs 同。
        use object_store::{ObjectStoreExt as _, path::Path as ObjPath};

        use crate::domain::storage::ParquetFileMetaDumpRepository;

        if limit == 0 {
            return Ok(0);
        }
        let repo =
            crate::infra::persistence::repositories::parquet_file_meta::dump::PgParquetFileMetaDumpRepository::new(
                self.pool.clone(),
            );
        let pending = repo.pending_object_deletes(limit).await?;
        let mut purged = 0u64;
        for row in pending {
            match self
                .object_store
                .delete(&ObjPath::from(row.object_key.as_str()))
                .await
            {
                Ok(()) => {}
                // 对象已不在（之前部分删过 / 外部清理）→ 仍删行，避免无限重试。
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => {
                    tracing::warn!(
                        object_key = %row.object_key,
                        error = %e,
                        "parquet_file_meta_dump object-delete sweep: store delete failed; retry next tick"
                    );
                    continue;
                }
            }
            repo.delete(&row.id).await?;
            purged += 1;
        }
        Ok(purged)
    }

    /// 跑一轮 dump：返回已写分区数 / 跳过数 / 已写行数。`enabled=false` 或
    /// `max_partitions_per_tick=0` 时直接 return 空 stats。
    #[tracing::instrument(
        name = "worker.parquet_file_meta_dump",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "parquet_file_meta_dump")
    )]
    pub async fn run_tick(&self) -> Result<TickStats> {
        if !self.settings.enabled || self.settings.max_partitions_per_tick == 0 {
            return Ok(TickStats::default());
        }
        let level = config_partition_level_to_domain(self.settings.partition_level);
        let now_us = TimestampMicros::now().0;
        let cutoff_us = now_us - i64::from(self.settings.cold_after_days) * DAY_MICROS;
        let partitions = self
            .scan_cold_partitions(level, cutoff_us, self.settings.max_partitions_per_tick)
            .await?;
        let mut stats = TickStats::default();
        for part in partitions {
            match self.dump_one_partition(&part, cutoff_us).await {
                Ok(DumpOutcome::Written(rows)) => {
                    stats.partitions_processed += 1;
                    stats.rows_dumped += rows;
                    metrics().partitions_written.inc();
                    metrics().rows_written.inc_by(rows);
                }
                Ok(DumpOutcome::Empty) => {
                    stats.partitions_skipped += 1;
                    metrics()
                        .partitions_skipped
                        .with_label_values(&["empty"])
                        .inc();
                }
                Ok(DumpOutcome::Locked) => {
                    stats.partitions_skipped += 1;
                    metrics()
                        .partitions_skipped
                        .with_label_values(&["locked"])
                        .inc();
                }
                Err(e) => {
                    stats.partitions_skipped += 1;
                    metrics()
                        .partitions_skipped
                        .with_label_values(&["error"])
                        .inc();
                    tracing::warn!(
                        org_id = %part.org_id.0,
                        stream = %part.stream,
                        partition_key = %part.partition_key,
                        error = %e,
                        "parquet_file_meta_dump partition skipped"
                    );
                }
            }
        }
        Ok(stats)
    }

    async fn scan_cold_partitions(
        &self,
        level: PartitionLevel,
        cutoff_us: i64,
        limit: u32,
    ) -> Result<Vec<PartitionKey>> {
        // GROUP BY 列由 partition_level 决定。daily 走 date，hourly 走
        // date_trunc('hour', ...)。两种模式下 partition_key 都按需 format 后填回。
        let rows = match level {
            PartitionLevel::Daily => {
                sqlx::query(
                    "SELECT org_id, stream, stream_type, dataset_kind,
                            (to_timestamp(time_start_micros / 1000000.0) AT TIME ZONE 'UTC')::date AS bucket_date
                     FROM parquet_file_meta
                     WHERE deleted = FALSE
                       AND time_end_micros < $1
                     GROUP BY org_id, stream, stream_type, dataset_kind, bucket_date
                     ORDER BY bucket_date, org_id, stream, stream_type, dataset_kind
                     LIMIT $2",
                )
                .bind(cutoff_us)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Error::internal(format!("parquet_file_meta_dump scan: {e}")))?
            }
            PartitionLevel::Hourly => {
                sqlx::query(
                    "SELECT org_id, stream, stream_type, dataset_kind,
                            date_trunc('hour', to_timestamp(time_start_micros / 1000000.0) AT TIME ZONE 'UTC')::timestamp AS bucket_hour
                     FROM parquet_file_meta
                     WHERE deleted = FALSE
                       AND time_end_micros < $1
                     GROUP BY org_id, stream, stream_type, dataset_kind, bucket_hour
                     ORDER BY bucket_hour, org_id, stream, stream_type, dataset_kind
                     LIMIT $2",
                )
                .bind(cutoff_us)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Error::internal(format!("parquet_file_meta_dump scan: {e}")))?
            }
        };
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let stream_type: String = row
                .try_get("stream_type")
                .map_err(|e| Error::internal(format!("scan stream_type: {e}")))?;
            let dataset_kind = row
                .try_get::<String, _>("dataset_kind")
                .map_err(|e| Error::internal(format!("scan dataset_kind: {e}")))?
                .parse::<PhysicalDatasetKind>()?;
            let (partition_key, date_str) = match level {
                PartitionLevel::Daily => {
                    let d: chrono::NaiveDate = row
                        .try_get("bucket_date")
                        .map_err(|e| Error::internal(format!("scan bucket_date: {e}")))?;
                    let s = d.format("%Y-%m-%d").to_string();
                    (s.clone(), s)
                }
                PartitionLevel::Hourly => {
                    let h: chrono::NaiveDateTime = row
                        .try_get("bucket_hour")
                        .map_err(|e| Error::internal(format!("scan bucket_hour: {e}")))?;
                    let pkey = h.format("%Y-%m-%d-%H").to_string();
                    let date_str = h.format("%Y-%m-%d").to_string();
                    (pkey, date_str)
                }
            };
            out.push(PartitionKey {
                org_id: Id::from_string(
                    row.try_get::<String, _>("org_id")
                        .map_err(|e| Error::internal(format!("scan org_id: {e}")))?,
                ),
                stream: row
                    .try_get::<String, _>("stream")
                    .map_err(|e| Error::internal(format!("scan stream: {e}")))?,
                stream_type: stream_type_from_str(&stream_type)?,
                dataset_kind,
                partition_level: level,
                partition_key,
                date_for_parquet: date_str,
            });
        }
        Ok(out)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "parquet_file_meta")
    )]
    async fn dump_one_partition(&self, part: &PartitionKey, cutoff_us: i64) -> Result<DumpOutcome> {
        let (start_us, end_us) = match part.partition_level {
            PartitionLevel::Daily => {
                let s = day_start_micros(&part.partition_key)?;
                (s, s + DAY_MICROS)
            }
            PartitionLevel::Hourly => {
                let s = hour_start_micros(&part.partition_key)?;
                (s, s + HOUR_MICROS)
            }
        };

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err_internal)?;

        // Advisory transactional lock: prevent two compactor instances from
        // dumping the same partition concurrently. Key into `pg_try_advisory_xact_lock`
        // takes a bigint, so we hash a stable composite into i64 via PG hashtext +
        // bitor with a level-specific high bit so daily/hourly buckets never collide.
        let lock_key = format!(
            "{}|{}|{}|{}|{}|{}",
            part.org_id.0,
            part.stream,
            stream_type_to_str(part.stream_type),
            part.dataset_kind.as_str(),
            part.partition_level.as_str(),
            part.partition_key
        );
        let acquired: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(&lock_key)
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlx_err_internal)?;
        if !acquired {
            // tx rolls back on drop (no mutations performed).
            return Ok(DumpOutcome::Locked);
        }

        // Pull every live parquet_file_meta row of this partition that's already cold.
        let rows = sqlx::query(
            "SELECT id, org_id, stream, stream_type, dataset_kind, object_key,
                    time_start_micros, time_end_micros,
                    rows, size_bytes, min_values, max_values, deleted
             FROM parquet_file_meta
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = $4
               AND deleted = FALSE
               AND time_start_micros >= $5 AND time_start_micros < $6
               AND time_end_micros < $7",
        )
        .bind(&part.org_id.0)
        .bind(&part.stream)
        .bind(stream_type_to_str(part.stream_type))
        .bind(part.dataset_kind.as_str())
        .bind(start_us)
        .bind(end_us)
        .bind(cutoff_us)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err_internal)?;

        if rows.is_empty() {
            // tx rolls back: lock released; counted as `skipped{empty}` upstream.
            return Ok(DumpOutcome::Empty);
        }

        let metas: Vec<crate::domain::storage::ParquetFileMeta> = rows
            .into_iter()
            .map(row_to_parquet_file_meta)
            .collect::<Result<Vec<_>>>()?;
        let ids: Vec<String> = metas.iter().map(|m| m.id.0.clone()).collect();
        let rows_count = metas.len() as u64;

        // Serialize + PUT. PUT is done BEFORE any DB mutation per design contract.
        let updated_at = TimestampMicros::now();
        let bytes = serialize_dump(&metas, &part.date_for_parquet, updated_at)?;
        let size_bytes = bytes.len() as i64;
        let key = dump_object_key(
            &part.org_id,
            &part.stream,
            part.stream_type,
            part.dataset_kind,
            part.partition_level,
            &part.partition_key,
        );
        put_dump(self.object_store.as_ref(), &key, bytes).await?;

        // Compute aggregates from the materialized rows (cheap; rows still owned).
        let agg = DumpAggregate::from_rows(&metas);

        let dump_row = ParquetFileMetaDumpRow {
            id: Id::new(),
            org_id: part.org_id.clone(),
            stream: part.stream.clone(),
            stream_type: part.stream_type,
            dataset_kind: part.dataset_kind,
            date: part.date_for_parquet.clone(),
            object_key: key.clone(),
            rows_in_dump: rows_count as u32,
            created_at: updated_at,
            partition_level: part.partition_level,
            partition_key: part.partition_key.clone(),
            deleted: false,
            min_ts_micros: agg.min_ts_micros,
            max_ts_micros: agg.max_ts_micros,
            size_bytes,
            updated_at_micros: updated_at.0,
        };
        let stats_row = ParquetFileMetaDumpStats {
            object_key: key,
            rows_total: agg.rows,
            files_total: agg.rows,
            time_start_micros: agg.min_ts_micros,
            time_end_micros: agg.max_ts_micros,
            storage_size_bytes: size_bytes,
            updated_at_micros: updated_at.0,
        };

        // INSERT dump + INSERT stats + DELETE main, all under the held lock + same tx.
        insert_dump_tx(&mut tx, &dump_row).await?;
        insert_stats_tx(&mut tx, &stats_row).await?;
        sqlx::query("DELETE FROM parquet_file_meta WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err_internal)?;
        tx.commit().await.map_err(sqlx_err_internal)?;

        // tx commit 后立刻失效 dump cache 的对应 partition entry：
        // re-dump 覆盖时旧 Arc<Vec<ParquetFileMeta>> 不再代表 object_store 上的真值
        // （spec scenario "Cache invalidated when new dump is written for the same partition"）。
        self.invalidate_cache(
            &part.org_id,
            &part.stream,
            part.stream_type,
            part.dataset_kind,
            part.partition_level,
            &part.partition_key,
        )
        .await;

        Ok(DumpOutcome::Written(rows_count))
    }

    /// retention / 合规删除走 dump 路径的精确入口（spec scenario 见
    /// `storage/ParquetFileMeta Dump Range Deletion`）。按 design D5 流程实现。
    pub async fn delete_by_time_range(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        target_range: TimeRange,
    ) -> Result<DeleteStats> {
        let mut stats = DeleteStats::default();

        // SELECT FOR UPDATE everything overlapping `target_range`. We open a SHORT
        // tx just for the dump-row SELECT; per-dump rewrite work runs OUTSIDE the
        // outer tx so we don't hold a tx open across object_store IO. Each rewrite
        // opens its own tx via insert_rewrite() / mark_deleted() in the repository
        // layer; if the process crashes mid-loop the next call picks up unfinished
        // partitions (deleted=false survivors still overlap target_range).
        let candidates: Vec<(String, String, String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT object_key, dataset_kind, partition_level, partition_key, date::text,
                    min_ts_micros, max_ts_micros
             FROM parquet_file_meta_dump
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = $4
               AND deleted = FALSE
               AND max_ts_micros >= $5
               AND min_ts_micros < $6",
        )
        .bind(&org_id.0)
        .bind(stream)
        .bind(stream_type_to_str(stream_type))
        .bind(dataset_kind.as_str())
        .bind(target_range.start.0)
        .bind(target_range.end.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err_internal)?;

        for (object_key, dataset_kind, level_str, partition_key, date_str, _min_ts, _max_ts) in
            candidates
        {
            let dataset_kind = dataset_kind.parse::<PhysicalDatasetKind>()?;
            let partition_level = parse_partition_level(&level_str)?;
            // Load every row of the dump (no time-range filter — we need to know
            // which rows survive vs which get deleted).
            let all_rows = crate::infra::storage::parquet_file_meta_dump::reader::read_dump(
                self.object_store.clone(),
                &object_key,
            )
            .await?;
            let (to_delete, to_keep): (Vec<_>, Vec<_>) = all_rows.into_iter().partition(|fm| {
                row_fully_inside_range(fm.time_range.start.0, fm.time_range.end.0, target_range)
            });
            if to_delete.is_empty() {
                // Nothing to do.
                continue;
            }
            stats.rows_removed += to_delete.len() as u64;
            if to_keep.is_empty() {
                // Full overlap: mark old row deleted + drop stats.
                use crate::domain::storage::ParquetFileMetaDumpRepository;
                let repo =
                    crate::infra::persistence::repositories::parquet_file_meta::dump::PgParquetFileMetaDumpRepository::new(
                        self.pool.clone(),
                    );
                repo.mark_deleted(&object_key).await?;
                sqlx::query("DELETE FROM parquet_file_meta_dump_stats WHERE object_key = $1")
                    .bind(&object_key)
                    .execute(&self.pool)
                    .await
                    .map_err(sqlx_err_internal)?;
                metrics().delete_dropped.inc();
                stats.partitions_dropped += 1;
            } else {
                // Partial overlap: rewrite `to_keep` to a new `.r{n}.parquet`.
                let next_seq = self
                    .next_rewrite_seq(org_id, stream, stream_type, dataset_kind, &partition_key)
                    .await?;
                let new_key = rewrite_object_key(
                    org_id,
                    stream,
                    stream_type,
                    dataset_kind,
                    &partition_key,
                    next_seq,
                );
                let updated_at = TimestampMicros::now();
                let bytes = serialize_dump(&to_keep, &date_str, updated_at)?;
                let size_bytes = bytes.len() as i64;
                put_dump(self.object_store.as_ref(), &new_key, bytes).await?;
                let agg = DumpAggregate::from_rows(&to_keep);
                let new_row = ParquetFileMetaDumpRow {
                    id: Id::new(),
                    org_id: org_id.clone(),
                    stream: stream.to_string(),
                    stream_type,
                    dataset_kind,
                    date: date_str.clone(),
                    object_key: new_key.clone(),
                    rows_in_dump: to_keep.len() as u32,
                    created_at: updated_at,
                    partition_level,
                    partition_key: partition_key.clone(),
                    deleted: false,
                    min_ts_micros: agg.min_ts_micros,
                    max_ts_micros: agg.max_ts_micros,
                    size_bytes,
                    updated_at_micros: updated_at.0,
                };
                let new_stats = ParquetFileMetaDumpStats {
                    object_key: new_key,
                    rows_total: agg.rows,
                    files_total: agg.rows,
                    time_start_micros: agg.min_ts_micros,
                    time_end_micros: agg.max_ts_micros,
                    storage_size_bytes: size_bytes,
                    updated_at_micros: updated_at.0,
                };
                use crate::domain::storage::ParquetFileMetaDumpRepository;
                let repo =
                    crate::infra::persistence::repositories::parquet_file_meta::dump::PgParquetFileMetaDumpRepository::new(
                        self.pool.clone(),
                    );
                repo.insert_rewrite(&object_key, new_row, new_stats).await?;
                metrics().delete_rewritten.inc();
                stats.partitions_rewritten += 1;
            }
            // Old object is left in the bucket; async cleanup sweep (follow-up)
            // consumes pending_object_deletes().
            //
            // Cache invalidation：partition_key 在整删 / 部分重写两种 case 都
            // 失效，让下一次冷查 miss + 重新从 PG + object_store 装载。
            // 对应 spec scenario：
            //   - "Cache invalidated when dump is marked deleted"
            //   - "Cache invalidated when new dump is written for the same partition"
            self.invalidate_cache(
                org_id,
                stream,
                stream_type,
                dataset_kind,
                partition_level,
                &partition_key,
            )
            .await;
        }
        Ok(stats)
    }

    /// 找出某 `partition_key` 当前最大的 rewrite seq，然后 +1。第一次重写返 1。
    /// 实现：从 object_key 后缀 `.r{n}.parquet` 解析；查所有该 partition 的 dump 行
    /// （含 deleted）取最大。
    async fn next_rewrite_seq(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        partition_key: &str,
    ) -> Result<u32> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT object_key FROM parquet_file_meta_dump
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = $4 AND partition_key = $5",
        )
        .bind(&org_id.0)
        .bind(stream)
        .bind(stream_type_to_str(stream_type))
        .bind(dataset_kind.as_str())
        .bind(partition_key)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err_internal)?;
        let mut max_seq: u32 = 0;
        for (key,) in rows {
            if let Some(n) = parse_rewrite_seq(&key) {
                max_seq = max_seq.max(n);
            }
        }
        Ok(max_seq + 1)
    }
}

#[derive(Debug, Clone)]
struct PartitionKey {
    org_id: Id,
    stream: String,
    stream_type: StreamType,
    dataset_kind: PhysicalDatasetKind,
    partition_level: PartitionLevel,
    partition_key: String,
    /// 写到 dump parquet 的 `date` 列；daily 时与 partition_key 一致，
    /// hourly 时是 `YYYY-MM-DD`（不含 hour）。
    date_for_parquet: String,
}

#[derive(Debug)]
enum DumpOutcome {
    Written(u64),
    Empty,
    Locked,
}

fn config_partition_level_to_domain(cfg: crate::config::PartitionLevel) -> PartitionLevel {
    match cfg {
        crate::config::PartitionLevel::Daily => PartitionLevel::Daily,
        crate::config::PartitionLevel::Hourly => PartitionLevel::Hourly,
    }
}

fn parse_partition_level(s: &str) -> Result<PartitionLevel> {
    match s {
        "daily" => Ok(PartitionLevel::Daily),
        "hourly" => Ok(PartitionLevel::Hourly),
        other => Err(Error::internal(format!("unknown partition_level: {other}"))),
    }
}

fn stream_type_to_str(t: StreamType) -> &'static str {
    match t {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

fn stream_type_from_str(s: &str) -> Result<StreamType> {
    match s {
        "logs" => Ok(StreamType::Logs),
        "metrics" => Ok(StreamType::Metrics),
        "traces" => Ok(StreamType::Traces),
        "profiles" => Ok(StreamType::Profiles),
        "extend" => Ok(StreamType::Extend),
        other => Err(Error::internal(format!("unknown stream_type: {other}"))),
    }
}

fn sqlx_err_internal(e: sqlx::Error) -> Error {
    Error::internal(format!("parquet_file_meta_dump sql: {e}"))
}

fn day_start_micros(date: &str) -> Result<i64> {
    let nd = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::invalid(format!("invalid partition date '{date}': {e}")))?;
    let dt = nd
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| Error::internal("invalid time"))?;
    Ok(
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
            .timestamp_micros(),
    )
}

fn hour_start_micros(partition_key: &str) -> Result<i64> {
    // chrono NaiveDateTime requires minutes/seconds; "YYYY-MM-DD-HH" is too short.
    // Split on the last '-' to recover the hour, then construct NaiveDateTime
    // explicitly at HH:00:00.
    let dash = partition_key.rfind('-').ok_or_else(|| {
        Error::invalid(format!(
            "invalid hourly partition_key '{partition_key}': missing hour"
        ))
    })?;
    let (date_part, hour_part) = partition_key.split_at(dash);
    let hour_str = &hour_part[1..]; // skip the dash
    let nd = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").map_err(|e| {
        Error::invalid(format!(
            "invalid hourly partition_key '{partition_key}' date '{date_part}': {e}"
        ))
    })?;
    let hour: u32 = hour_str.parse().map_err(|e| {
        Error::invalid(format!(
            "invalid hourly partition_key '{partition_key}' hour '{hour_str}': {e}"
        ))
    })?;
    let dt = nd.and_hms_opt(hour, 0, 0).ok_or_else(|| {
        Error::invalid(format!("invalid hourly partition_key hour: '{hour_str}'"))
    })?;
    Ok(
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
            .timestamp_micros(),
    )
}

fn row_fully_inside_range(start: i64, end: i64, range: TimeRange) -> bool {
    start >= range.start.0 && end < range.end.0
}

/// 从 object_key 末段抓 rewrite seq；non-rewrite key (`…/{partition_key}.parquet`)
/// 返回 None；rewrite key (`…/{partition_key}.r{n}.parquet`) 返回 Some(n)。
fn parse_rewrite_seq(key: &str) -> Option<u32> {
    let stem = key.rsplit('/').next()?;
    let body = stem.strip_suffix(".parquet")?;
    let dot = body.rfind('.')?;
    let suffix = &body[dot + 1..];
    if !suffix.starts_with('r') {
        return None;
    }
    suffix[1..].parse::<u32>().ok()
}

async fn insert_dump_tx(
    tx: &mut sqlx::TracedTransaction<'_>,
    row: &ParquetFileMetaDumpRow,
) -> Result<()> {
    let date = chrono::NaiveDate::parse_from_str(
        row.partition_key.get(0..10).unwrap_or(&row.date),
        "%Y-%m-%d",
    )
    .map_err(|e| Error::invalid(format!("invalid partition_key date: {e}")))?;
    sqlx::query(
        "INSERT INTO parquet_file_meta_dump
         (id, org_id, stream, stream_type, dataset_kind, partition_level, partition_key,
          object_key, deleted, rows_in_dump, size_bytes,
          min_ts_micros, max_ts_micros, date,
          created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE, $9, $10, $11, $12, $13, $14, $15)
         ON CONFLICT (org_id, stream, stream_type, dataset_kind, partition_level, partition_key)
           WHERE deleted = FALSE
         DO UPDATE SET
             object_key        = EXCLUDED.object_key,
             rows_in_dump      = EXCLUDED.rows_in_dump,
             size_bytes        = EXCLUDED.size_bytes,
             min_ts_micros     = EXCLUDED.min_ts_micros,
             max_ts_micros     = EXCLUDED.max_ts_micros,
             date              = EXCLUDED.date,
             updated_at_micros = EXCLUDED.updated_at_micros",
    )
    .bind(&row.id.0)
    .bind(&row.org_id.0)
    .bind(&row.stream)
    .bind(stream_type_to_str(row.stream_type))
    .bind(row.dataset_kind.as_str())
    .bind(row.partition_level.as_str())
    .bind(&row.partition_key)
    .bind(&row.object_key)
    .bind(row.rows_in_dump as i32)
    .bind(row.size_bytes)
    .bind(row.min_ts_micros)
    .bind(row.max_ts_micros)
    .bind(date)
    .bind(row.created_at.0)
    .bind(row.updated_at_micros)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err_internal)?;
    Ok(())
}

async fn insert_stats_tx(
    tx: &mut sqlx::TracedTransaction<'_>,
    stats: &ParquetFileMetaDumpStats,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO parquet_file_meta_dump_stats
         (object_key, rows_total, files_total,
          time_start_micros, time_end_micros,
          storage_size_bytes, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (object_key) DO UPDATE SET
             rows_total         = EXCLUDED.rows_total,
             files_total        = EXCLUDED.files_total,
             time_start_micros  = EXCLUDED.time_start_micros,
             time_end_micros    = EXCLUDED.time_end_micros,
             storage_size_bytes = EXCLUDED.storage_size_bytes,
             updated_at_micros  = EXCLUDED.updated_at_micros",
    )
    .bind(&stats.object_key)
    .bind(stats.rows_total)
    .bind(stats.files_total)
    .bind(stats.time_start_micros)
    .bind(stats.time_end_micros)
    .bind(stats.storage_size_bytes)
    .bind(stats.updated_at_micros)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err_internal)?;
    Ok(())
}

fn row_to_parquet_file_meta(
    row: sqlx::postgres::PgRow,
) -> Result<crate::domain::storage::ParquetFileMeta> {
    use sqlx::types::Json;

    use crate::domain::storage::ParquetFileMeta;
    let stream_type: String = row
        .try_get("stream_type")
        .map_err(|e| Error::internal(format!("parquet_file_meta stream_type: {e}")))?;
    let min: Json<serde_json::Value> = row
        .try_get("min_values")
        .map_err(|e| Error::internal(format!("parquet_file_meta min_values: {e}")))?;
    let max: Json<serde_json::Value> = row
        .try_get("max_values")
        .map_err(|e| Error::internal(format!("parquet_file_meta max_values: {e}")))?;
    let min = match min.0 {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    let max = match max.0 {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    let rows_n: i64 = row
        .try_get("rows")
        .map_err(|e| Error::internal(format!("parquet_file_meta rows: {e}")))?;
    let size_bytes: i64 = row
        .try_get("size_bytes")
        .map_err(|e| Error::internal(format!("parquet_file_meta size_bytes: {e}")))?;
    Ok(ParquetFileMeta {
        id: Id::from_string(
            row.try_get::<String, _>("id")
                .map_err(|e| Error::internal(format!("parquet_file_meta id: {e}")))?,
        ),
        org_id: Id::from_string(
            row.try_get::<String, _>("org_id")
                .map_err(|e| Error::internal(format!("parquet_file_meta org_id: {e}")))?,
        ),
        stream: row
            .try_get("stream")
            .map_err(|e| Error::internal(format!("parquet_file_meta stream: {e}")))?,
        stream_type: stream_type_from_str(&stream_type)?,
        dataset_kind: row
            .try_get::<String, _>("dataset_kind")
            .map_err(|e| Error::internal(format!("parquet_file_meta dataset_kind: {e}")))?
            .parse()?,
        object_key: row
            .try_get("object_key")
            .map_err(|e| Error::internal(format!("parquet_file_meta object_key: {e}")))?,
        time_range: TimeRange::new(
            TimestampMicros(
                row.try_get("time_start_micros")
                    .map_err(|e| Error::internal(format!("parquet_file_meta time_start: {e}")))?,
            ),
            TimestampMicros(
                row.try_get("time_end_micros")
                    .map_err(|e| Error::internal(format!("parquet_file_meta time_end: {e}")))?,
            ),
        ),
        rows: rows_n as u64,
        size_bytes: size_bytes as u64,
        min_values: min,
        max_values: max,
        deleted: row
            .try_get("deleted")
            .map_err(|e| Error::internal(format!("parquet_file_meta deleted: {e}")))?,
    })
}

struct Metrics {
    partitions_written: IntCounter,
    rows_written: IntCounter,
    partitions_skipped: IntCounterVec,
    delete_rewritten: IntCounter,
    delete_dropped: IntCounter,
}

/// 仅供测试：触发 service 侧 metric family 注册（用 OnceLock 幂等）。同时为 vec
/// 类型 metric "预热"一个零计数 label，让 prometheus 的 text-encoder 在第一次
/// scrape 就能输出对应行。
pub fn register_metrics_for_test() -> &'static () {
    let m = metrics();
    for reason in ["empty", "locked", "error", "duplicate_id"] {
        m.partitions_skipped.with_label_values(&[reason]);
    }
    &()
}

fn metrics() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| Metrics {
        partitions_written: register_int_counter(
            "parquet_file_meta_dump_partitions_written_total",
            "parquet_file_meta dump partitions written to object store",
        ),
        rows_written: register_int_counter(
            "parquet_file_meta_dump_rows_written_total",
            "ParquetFileMeta rows serialized into dumps",
        ),
        partitions_skipped: register_int_counter_vec(
            "parquet_file_meta_dump_partitions_skipped_total",
            "parquet_file_meta dump partitions skipped, by reason",
            &["reason"],
        ),
        delete_rewritten: register_int_counter(
            "parquet_file_meta_dump_delete_partitions_rewritten_total",
            "dump partitions partially rewritten by delete_by_time_range",
        ),
        delete_dropped: register_int_counter(
            "parquet_file_meta_dump_delete_partitions_dropped_total",
            "dump partitions fully dropped by delete_by_time_range",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_start_micros_round_trips_utc_date() {
        // 2026-05-27 00:00:00 UTC = 1779840000 seconds = 1779840000000000 micros
        let us = day_start_micros("2026-05-27").unwrap();
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp_micros(us).unwrap();
        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-05-27 00:00:00"
        );
    }

    #[test]
    fn hour_start_micros_round_trips_utc_hour() {
        let us = hour_start_micros("2026-05-27-13").unwrap();
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp_micros(us).unwrap();
        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-05-27 13:00:00"
        );
    }

    #[test]
    fn stream_type_round_trip_covers_all_variants() {
        for st in [
            StreamType::Logs,
            StreamType::Metrics,
            StreamType::Traces,
            StreamType::Extend,
        ] {
            let s = stream_type_to_str(st);
            let back = stream_type_from_str(s).unwrap();
            assert_eq!(back, st);
        }
        assert!(stream_type_from_str("bogus").is_err());
    }

    /// task 5.6：验证 `with_dump_cache` 注入后，service 的 `invalidate_cache`
    /// helper 在被调用时把对应 partition 的 cache entry drop。直接用 in-memory cache
    /// 验证 invalidate hook 真的接到了 cache，不需要 PG / object_store。
    #[tokio::test]
    async fn invalidate_cache_drops_partition_entry() {
        use std::sync::Arc;

        use object_store::memory::InMemory;

        use crate::{
            config::ParquetFileMetaDumpCacheSettings,
            domain::storage::ParquetFileMeta,
            infra::caching::parquet_file_meta::dump::{DumpCacheKey, ParquetFileMetaDumpCache},
            shared::time::{TimeRange, TimestampMicros},
        };

        let cache = Arc::new(ParquetFileMetaDumpCache::new(
            &ParquetFileMetaDumpCacheSettings {
                capacity: 10,
                ttl_secs: 60,
            },
        ));
        // seed cache
        let org_id = Id::from_string("org-x");
        let stream = "app";
        let stream_type = StreamType::Logs;
        let dataset_kind = PhysicalDatasetKind::Raw;
        let level = PartitionLevel::Daily;
        let pkey = "2026-01-15";
        let key = DumpCacheKey::new(&org_id, stream, stream_type, dataset_kind, level, pkey);
        cache
            .insert(
                key.clone(),
                Arc::new(vec![ParquetFileMeta {
                    id: Id::from_string("fm-1"),
                    org_id: org_id.clone(),
                    stream: stream.into(),
                    stream_type,
                    dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
                    object_key: "obj".into(),
                    time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
                    rows: 0,
                    size_bytes: 0,
                    min_values: serde_json::Map::new(),
                    max_values: serde_json::Map::new(),
                    deleted: false,
                }]),
            )
            .await;
        assert!(cache.get(&key).await.is_some(), "cache must hold the entry");

        // 构造 service：PgPool 走 connect_lazy（不真连），invalidate_cache 只走 cache 不打 DB
        let pool =
            sqlx::PgPool::connect_lazy("postgres://noop:noop@127.0.0.1:0/noop").expect("lazy pool");
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let svc =
            ParquetFileMetaDumpService::new(pool, store, ParquetFileMetaDumpSettings::default())
                .with_dump_cache(cache.clone());
        svc.invalidate_cache(&org_id, stream, stream_type, dataset_kind, level, pkey)
            .await;

        assert!(
            cache.get(&key).await.is_none(),
            "invalidate_cache must drop the partition entry"
        );
    }

    /// `with_dump_cache` 未注入时 invalidate_cache 是 no-op（不 panic）。
    #[tokio::test]
    async fn invalidate_cache_without_dump_cache_is_noop() {
        use std::sync::Arc;

        use object_store::memory::InMemory;
        let pool =
            sqlx::PgPool::connect_lazy("postgres://noop:noop@127.0.0.1:0/noop").expect("lazy pool");
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let svc =
            ParquetFileMetaDumpService::new(pool, store, ParquetFileMetaDumpSettings::default());
        // 不应当 panic
        svc.invalidate_cache(
            &Id::from_string("org"),
            "stream",
            StreamType::Logs,
            PhysicalDatasetKind::Raw,
            PartitionLevel::Daily,
            "2026-01-15",
        )
        .await;
    }

    #[test]
    fn parse_rewrite_seq_handles_known_shapes() {
        assert_eq!(
            parse_rewrite_seq("orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.parquet"),
            None
        );
        assert_eq!(
            parse_rewrite_seq("orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.r1.parquet"),
            Some(1)
        );
        assert_eq!(
            parse_rewrite_seq(
                "orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.r42.parquet"
            ),
            Some(42)
        );
        assert_eq!(
            parse_rewrite_seq(
                "orgA/_parquet_file_meta_dump/logs/raw/stream/2026-01-15.foo.parquet"
            ),
            None
        );
    }
}
